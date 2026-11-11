//! Stillshot's windowed UI: a tray-resident settings window plus one
//! borderless overlay per monitor when a freeze is active.
//!
//! How a frame runs (eframe 0.37):
//!
//! * [`eframe::App::logic`] runs every tick, whether or not the settings
//!   window is visible. Hotkeys, the tray menu, timeouts and window visibility
//!   live there, so the app keeps working from the tray.
//! * [`eframe::App::ui`] draws the settings window and, during a freeze, the
//!   overlays. Each overlay is shown with exactly ONE
//!   `show_viewport_immediate` call per frame. Every such call takes that
//!   window's pending input, clears it, paints and presents. A second call in
//!   the same frame presents a second picture and gets no input at all.

use crate::annotate::{Annotation, Color, Document, Pt, SWATCHES, Shape, Tool, simplify_stroke};
use crate::capture::{self, Desktop};
use crate::clipboard;
use crate::config::{self, Config};
use crate::desktop;
use crate::gate::{self, CaptureGate};
use crate::geom::output_scale;
use crate::geom::{RectF, constrain_line, constrain_square};
use crate::hotkey::{self, print_screen_modifiers};
use crate::icon;
use crate::log::log;
use crate::paint;
use crate::platform;
use crate::save;
use anyhow::Result;
use eframe::egui::{self, Color32, Key, Pos2, Rect, TextureHandle, Vec2, ViewportId};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

static CAPTURE_REQUESTED: AtomicBool = AtomicBool::new(false);
static PANIC_QUIT: AtomicBool = AtomicBool::new(false);
static PANIC_HOTKEY_ID: AtomicU32 = AtomicU32::new(u32::MAX);
static MENU_EVENTS: Mutex<Vec<MenuEvent>> = Mutex::new(Vec::new());
static UI_CTX: OnceLock<egui::Context> = OnceLock::new();

/// How often the logic ticks while a freeze is up. This is also the overlay
/// frame cap: immediate viewports are painted from the root frame, and vsync
/// is off on Windows.
const SESSION_TICK: Duration = Duration::from_millis(16);
/// How often the logic ticks while idle in the tray.
const IDLE_TICK: Duration = Duration::from_millis(100);
/// Ask for keyboard focus at most this many times per freeze.
const FOCUS_ATTEMPTS: u8 = 3;
const FOCUS_RETRY: Duration = Duration::from_millis(250);
/// Once the overlay has had focus and lost it for this long, it closes.
const FOCUS_LOSS_GRACE: Duration = Duration::from_millis(400);

pub fn request_capture() {
    CAPTURE_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(ctx) = UI_CTX.get() {
        ctx.request_repaint();
    }
}

pub fn request_panic_quit() {
    PANIC_QUIT.store(true, Ordering::SeqCst);
    if let Some(ctx) = UI_CTX.get() {
        ctx.request_repaint();
    }
}

pub struct Launch {
    pub show_settings: bool,
    pub capture_now: bool,
}

pub struct App {
    config: Config,
    show_settings: bool,
    quit: bool,
    status: String,
    hotkey_error: Option<String>,
    recording: bool,
    manager: GlobalHotKeyManager,
    registered: Option<HotKey>,
    panic_hotkey: Option<HotKey>,
    gate: CaptureGate,
    /// What we last told the settings window to be. Commands go out on change only.
    root_visible: Option<bool>,
    tray: Option<TrayIcon>,
    tray_capture: MenuId,
    tray_settings: MenuId,
    tray_quit: MenuId,
    session: Option<Session>,
    textures: Textures,
    hover_monitor: usize,
    /// `slot_monitor` is known to be right.
    matched: bool,
    /// Overlay window `slot` (the windowing layer's monitor index) shows
    /// `desktop.monitors[slot_monitor[slot]]`.
    slot_monitor: Vec<usize>,
    /// The overlay on the monitor under the cursor. It gets keyboard focus.
    focus_slot: usize,
    /// When the overlays were last painted in this freeze.
    last_overlay_frame: Option<Instant>,
    /// A "Save as" waiting for the overlay windows to go away.
    pending_save: Option<PendingSave>,
    key_stamp: u64,
    stamp: u64,
}

struct PendingSave {
    image: RgbaImage,
    waited_frames: u8,
}

struct Textures {
    monitors: Vec<Option<TextureHandle>>,
    crop: Option<TextureHandle>,
    crop_gen: u64,
}

pub(crate) struct Session {
    pub desktop: Desktop,
    pub selection: Option<Selection>,
    pub drag: Option<(Pos2, Pos2)>,
    pub draft: Option<Draft>,
    pub tool: Tool,
    pub color: Color,
    pub stroke: f32,
    pub text: Option<TextDraft>,
    pub toolbar: Option<RectF>,
    pub toast_until: Option<Instant>,
    pub toast: String,
    pub error: Option<String>,
    pub saved_path: Option<std::path::PathBuf>,
    pub show_custom_color: bool,
    pub render_gen: u64,
    pub opened: Instant,
    pub logged_paint: bool,
    /// An overlay window has had keyboard focus at some point.
    pub had_focus: bool,
    pub unfocused_since: Option<Instant>,
    pub focus_attempts: u8,
    pub next_focus_at: Instant,
}

pub(crate) struct Selection {
    pub rect: RectF,
    pub scale: f32,
    pub base: RgbaImage,
    pub rendered: RgbaImage,
    pub doc: Document,
}

pub(crate) struct TextDraft {
    pub at: Pt,
    pub body: String,
}

pub(crate) enum Draft {
    Stroke(Vec<Pt>),
    Line(Pt, Pt),
    Arrow(Pt, Pt),
    Rect(Pt, Pt),
    Ellipse(Pt, Pt),
    Blur(Pt, Pt),
    Pixelate(Pt, Pt),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        let _ = UI_CTX.set(cc.egui_ctx.clone());
        install_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);
        let (config, first_run) = config::load();
        if first_run {
            let _ = config::save(&config);
        }
        let manager = GlobalHotKeyManager::new().expect("global hotkey manager");
        let (tray, tray_capture, tray_settings, tray_quit) = build_tray();
        let mut app = Self {
            config,
            show_settings: launch.show_settings
                || first_run
                || !platform::screen_recording_granted(),
            quit: false,
            status: String::new(),
            hotkey_error: None,
            recording: false,
            manager,
            registered: None,
            panic_hotkey: None,
            gate: CaptureGate::new(),
            root_visible: Some(launch.show_settings),
            tray,
            tray_capture,
            tray_settings,
            tray_quit,
            session: None,
            textures: Textures {
                monitors: Vec::new(),
                crop: None,
                crop_gen: 0,
            },
            hover_monitor: 0,
            matched: false,
            slot_monitor: Vec::new(),
            focus_slot: 0,
            last_overlay_frame: None,
            pending_save: None,
            key_stamp: 0,
            stamp: 0,
        };
        // The keyboard hook runs on its own thread and wakes us through this.
        let wake_ctx = cc.egui_ctx.clone();
        platform::set_waker(Box::new(move || wake_ctx.request_repaint()));
        platform::start_keyboard_hook();
        if let Err(err) = platform::set_launch_at_login(app.config.launch_at_login) {
            log(format!("launch at login: {err}"));
        }
        if let Err(err) = app.apply_hotkey() {
            app.hotkey_error = Some(err);
        }
        spawn_event_threads(cc.egui_ctx.clone());
        app.ensure_panic_hotkey();
        if launch.capture_now {
            log("ignoring --capture on launch; press the shortcut");
        }
        app
    }

    fn ensure_panic_hotkey(&mut self) {
        if self.panic_hotkey.is_some() {
            return;
        }
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyQ);
        let id = hotkey.id();
        match self.manager.register(hotkey) {
            Ok(()) => {
                PANIC_HOTKEY_ID.store(id, Ordering::SeqCst);
                self.panic_hotkey = Some(hotkey);
                log("panic hotkey Ctrl+Alt+Q registered");
            }
            Err(err) => {
                log(format!(
                    "panic hotkey Ctrl+Alt+Q was not registered ({err}); the key poll still works on Windows"
                ));
            }
        }
    }

    fn apply_hotkey(&mut self) -> Result<(), String> {
        if let Some(hotkey) = self.registered.take() {
            let _ = self.manager.unregister(hotkey);
        }
        platform::remove_print_screen_hook();
        let shortcut = self.config.shortcut();
        if cfg!(target_os = "windows") && shortcut.is_print_screen() {
            let (ctrl, alt, shift, meta) = print_screen_modifiers(&shortcut);
            platform::install_print_screen_hook(ctrl, alt, shift, meta);
            log(format!("using the Print Screen hook for {shortcut}"));
            return Ok(());
        }
        let hotkey = shortcut.global()?;
        self.manager
            .register(hotkey)
            .map_err(|err| err.to_string())?;
        self.registered = Some(hotkey);
        log(format!("registered hotkey {shortcut}"));
        Ok(())
    }

    fn begin_capture(&mut self) {
        let now = Instant::now();
        if self.session.is_some() || !self.gate.allow(now) {
            return;
        }
        // Set this before the grab so Print Screen auto-repeat cannot queue
        // another capture on the thread that owns the keyboard hook.
        platform::set_overlay_active(true);
        if !platform::screen_recording_granted() {
            platform::set_overlay_active(false);
            self.show_settings = true;
            self.status =
                "Screen Recording permission is required before Stillshot can freeze the display."
                    .into();
            platform::request_screen_recording();
            self.gate.on_closed(Instant::now());
            return;
        }
        let started = Instant::now();
        platform::begin_overlay_watch();
        match capture::capture_desktop() {
            Ok(desktop) => {
                let n = desktop.monitors.len();
                log(format!(
                    "overlay opening after {} ms of capture",
                    started.elapsed().as_millis()
                ));
                self.textures = Textures {
                    monitors: vec![None; n],
                    crop: None,
                    crop_gen: 0,
                };
                let (slot_monitor, matched) = initial_slot_map(&desktop);
                self.focus_slot = cursor_slot(&desktop, &slot_monitor);
                self.hover_monitor = slot_monitor.get(self.focus_slot).copied().unwrap_or(0);
                self.slot_monitor = slot_monitor;
                self.matched = matched;
                self.last_overlay_frame = None;
                log(format!(
                    "overlay slots -> monitors {:?} (matched={}), focus slot {}",
                    self.slot_monitor, self.matched, self.focus_slot
                ));
                self.gate.on_opened();
                self.session = Some(Session {
                    desktop,
                    selection: None,
                    drag: None,
                    draft: None,
                    tool: Tool::Select,
                    color: SWATCHES[0],
                    stroke: 4.0,
                    text: None,
                    toolbar: None,
                    toast_until: None,
                    toast: String::new(),
                    error: None,
                    saved_path: None,
                    show_custom_color: false,
                    render_gen: 0,
                    opened: started,
                    logged_paint: false,
                    had_focus: false,
                    unfocused_since: None,
                    focus_attempts: 0,
                    next_focus_at: Instant::now(),
                });
            }
            Err(err) => {
                log(format!("capture failed: {err}"));
                platform::set_overlay_active(false);
                platform::end_overlay_watch();
                self.gate.on_closed(Instant::now());
                self.status = format!("{err}");
                self.show_settings = true;
            }
        }
    }

    fn close_session(&mut self) {
        let overlays = self.slot_monitor.len();
        let was_open = self.session.take().is_some();
        self.textures = Textures {
            monitors: Vec::new(),
            crop: None,
            crop_gen: 0,
        };
        self.matched = false;
        self.last_overlay_frame = None;
        if was_open {
            self.gate.on_closed(Instant::now());
            platform::set_overlay_active(false);
            platform::end_overlay_watch();
            if let Some(ctx) = UI_CTX.get() {
                // Take the windows off screen now. They are destroyed on the
                // next frame, when `ui` no longer shows them.
                for slot in 0..overlays {
                    ctx.send_viewport_cmd_to(
                        overlay_id(slot),
                        egui::ViewportCommand::Visible(false),
                    );
                }
                ctx.request_repaint_of(ViewportId::ROOT);
            }
            log("overlay closed");
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.session.is_some() {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            [0.09, 0.09, 0.10, 1.0]
        }
    }

    /// Runs every tick, with the settings window visible or not.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let _keep_tray_alive = &self.tray;
        self.stamp = self.stamp.wrapping_add(1);
        self.pump_events();
        if self.quit {
            self.close_session();
            ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Close);
            return;
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            // The close button hides the settings window; the app stays in the tray.
            ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::CancelClose);
            self.show_settings = false;
        }
        self.run_pending_save(ctx);
        self.sync_settings_window(ctx);
        if self.session.is_some() {
            // Immediate overlays are drawn inside the root frame. A hidden
            // settings window does not run `ui` unless asked, and then the
            // freeze never sees the mouse. Keep asking for the whole capture.
            ctx.send_viewport_cmd_to(
                ViewportId::ROOT,
                egui::ViewportCommand::RequestPaintWhileHidden,
            );
            ctx.request_repaint_after(SESSION_TICK);
        } else {
            ctx.request_repaint_after(IDLE_TICK);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.quit {
            return;
        }
        let ctx = ui.ctx().clone();
        if self.show_settings {
            self.settings_ui(ui);
        }
        if self.session.is_some() {
            self.show_overlays(&ctx);
        }
    }
}

impl App {
    fn pump_events(&mut self) {
        if PANIC_QUIT.swap(false, Ordering::SeqCst) || platform::panic_quit_pressed() {
            log("panic hotkey Ctrl+Alt+Q — closing overlays and quitting");
            self.close_session();
            self.quit = true;
            return;
        }
        if self.session.is_some() {
            let opened = self.session.as_ref().map(|s| s.opened).unwrap_or_else(Instant::now);
            if platform::dismiss_pressed() {
                log("dismissing overlay from Esc or right-click");
                self.close_session();
            } else if gate::overlay_expired(opened, Instant::now()) {
                log("overlay auto-closed after 60s");
                self.close_session();
            }
        } else {
            // Keep the edge detectors in sync so a key held before the overlay
            // opens does not immediately dismiss it.
            let _ = platform::dismiss_pressed();
        }
        // Always consume triggers. Launch and key-repeat must not open a freeze.
        let triggered = CAPTURE_REQUESTED.swap(false, Ordering::SeqCst)
            || platform::take_capture_request()
            || platform::print_screen_pressed();
        if triggered {
            self.begin_capture();
        }
        let events =
            std::mem::take(&mut *MENU_EVENTS.lock().unwrap_or_else(|err| err.into_inner()));
        for event in events {
            if event.id == self.tray_capture {
                request_capture();
            } else if event.id == self.tray_settings {
                self.show_settings = true;
            } else if event.id == self.tray_quit {
                log("quit from the tray menu");
                self.close_session();
                self.quit = true;
            }
        }
    }

    /// Show or hide the settings window, but only when that changes.
    ///
    /// The settings window is left alone during a freeze. Hiding or showing a
    /// window moves activation around, and the overlay needs the keyboard.
    fn sync_settings_window(&mut self, ctx: &egui::Context) {
        let want = self.show_settings;
        if self.root_visible == Some(want) || self.session.is_some() {
            return;
        }
        ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Visible(want));
        if want {
            ctx.send_viewport_cmd_to(ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        self.root_visible = Some(want);
    }

    fn show_overlays(&mut self, ctx: &egui::Context) {
        let n = self.slot_monitor.len();
        if n == 0 {
            return;
        }
        // Do not sleep here. This runs on the UI thread, and a sleep stalls
        // mouse input for the whole freeze. `logic` already caps the tick.
        let first_frame = self.last_overlay_frame.is_none();
        self.last_overlay_frame = Some(Instant::now());
        self.match_overlay_windows(ctx);

        let mut any_focused = false;
        for slot in 0..n {
            if self.session.is_none() {
                // An earlier overlay closed the freeze during this frame.
                break;
            }
            let id = overlay_id(slot);
            if first_frame {
                // Undo the hide from the previous freeze if the window survived it.
                ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Visible(true));
            }
            let builder = self.overlay_builder(slot);
            // Exactly one immediate pass per overlay per frame. See the module docs.
            ctx.show_viewport_immediate(id, builder, |ui, _class| {
                let ctx = ui.ctx().clone();
                if ctx.input(|i| i.viewport().focused) == Some(true) {
                    any_focused = true;
                }
                self.paint_overlay(&ctx, slot);
            });
        }
        self.update_focus(ctx, any_focused);
    }

    fn overlay_builder(&self, slot: usize) -> egui::ViewportBuilder {
        // `with_monitor` makes a borderless fullscreen window on that display,
        // so the windowing layer owns the rectangle and the DPI. No explicit
        // position or size: those fight the fullscreen state.
        //
        // Topmost on Windows, so the frozen picture also covers the taskbar and
        // the overlay cannot open underneath another window. What keeps that
        // safe: Esc, Win and Alt+Tab close it from the keyboard hook without
        // needing focus, losing focus closes it, right-click closes it, and the
        // 60 second timeout and watchdog still apply.
        let level = if cfg!(target_os = "windows") {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        // This must return the same builder every frame of a freeze. A changed
        // builder makes egui patch or recreate the window.
        egui::ViewportBuilder::default()
            .with_title("Stillshot")
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(level)
            .with_taskbar(false)
            .with_has_shadow(false)
            .with_visible(true)
            .with_active(slot == self.focus_slot)
            .with_monitor(slot)
            .with_clamp_size_to_monitor_size(false)
    }

    /// Fallback for platforms where the slot order is not known up front:
    /// pair overlay windows with captures by position, using the window
    /// rectangles the previous frame reported. Until that succeeds, slot `i`
    /// shows capture `i`.
    fn match_overlay_windows(&mut self, ctx: &egui::Context) {
        if self.matched {
            return;
        }
        let n = self.slot_monitor.len();
        let rects: Vec<Option<Rect>> = ctx.input(|i| {
            (0..n)
                .map(|slot| {
                    i.raw
                        .viewports
                        .get(&overlay_id(slot))
                        .and_then(|info| info.outer_rect)
                        .filter(|rect| rect.width() > 32.0 && rect.height() > 32.0)
                })
                .collect()
        });
        if n == 0 || rects.iter().any(|rect| rect.is_none()) {
            return;
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| {
            let ra = rects[*a].unwrap();
            let rb = rects[*b].unwrap();
            ra.min
                .y
                .total_cmp(&rb.min.y)
                .then(ra.min.x.total_cmp(&rb.min.x))
        });
        // Captures are sorted the same way (top-to-bottom, left-to-right).
        for (image, slot) in order.into_iter().enumerate() {
            self.slot_monitor[slot] = image;
        }
        self.matched = true;
        log(format!(
            "matched {n} overlay windows to monitors by position: {:?}",
            self.slot_monitor
        ));
    }

    /// Give one overlay the keyboard, once, and close the freeze when the
    /// overlay has had the keyboard and lost it.
    fn update_focus(&mut self, ctx: &egui::Context, any_focused: bool) {
        let focus_id = overlay_id(self.focus_slot);
        let now = Instant::now();
        let mut lost = false;
        if let Some(session) = &mut self.session {
            if any_focused {
                if !session.had_focus {
                    log(format!(
                        "overlay has keyboard focus {} ms after the shortcut",
                        session.opened.elapsed().as_millis()
                    ));
                }
                session.had_focus = true;
                session.unfocused_since = None;
            } else if session.had_focus {
                // Something else took the foreground. A topmost overlay must
                // not sit on top of it.
                let since = *session.unfocused_since.get_or_insert(now);
                lost = now.duration_since(since) >= FOCUS_LOSS_GRACE;
            } else if session.focus_attempts < FOCUS_ATTEMPTS && now >= session.next_focus_at {
                // One window, a few spaced attempts. Asking every overlay on
                // every frame makes them steal the foreground from each other.
                session.focus_attempts += 1;
                session.next_focus_at = now + FOCUS_RETRY;
                ctx.send_viewport_cmd_to(focus_id, egui::ViewportCommand::Focus);
            }
        }
        if lost {
            log("overlay lost keyboard focus; closing it");
            self.close_session();
        }
    }

    fn paint_overlay(&mut self, ctx: &egui::Context, slot: usize) {
        let monitor_index = self.slot_monitor.get(slot).copied().unwrap_or(slot);
        self.ensure_monitor_texture(ctx, monitor_index);
        self.ensure_crop_texture(ctx);
        if let Some(session) = &mut self.session {
            if !session.logged_paint {
                session.logged_paint = true;
                let view = ctx.viewport_rect();
                log(format!(
                    "first overlay paint {} ms after the shortcut woke us (capture was {} ms), viewport {:.0}x{:.0} points at {:.2} px/pt",
                    session.opened.elapsed().as_millis(),
                    session.desktop.capture_ms,
                    view.width(),
                    view.height(),
                    ctx.pixels_per_point()
                ));
            }
        }
        crate::overlay::paint(self, ctx, monitor_index);
    }

    fn ensure_monitor_texture(&mut self, ctx: &egui::Context, monitor_index: usize) {
        if self
            .textures
            .monitors
            .get(monitor_index)
            .and_then(|t| t.as_ref())
            .is_some()
        {
            return;
        }
        let Some(image) = self
            .session
            .as_ref()
            .and_then(|s| s.desktop.monitors.get(monitor_index))
            .map(|m| m.image.clone())
        else {
            return;
        };
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );
        let handle = ctx.load_texture(
            format!("freeze-{monitor_index}"),
            color,
            egui::TextureOptions::LINEAR,
        );
        if monitor_index >= self.textures.monitors.len() {
            self.textures
                .monitors
                .resize_with(monitor_index + 1, || None);
        }
        self.textures.monitors[monitor_index] = Some(handle);
    }

    fn ensure_crop_texture(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.session else { return };
        let Some(selection) = &session.selection else {
            return;
        };
        if self.textures.crop_gen == session.render_gen && self.textures.crop.is_some() {
            return;
        }
        let image = selection.rendered.clone();
        let generation = session.render_gen;
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );
        self.textures.crop = Some(ctx.load_texture("crop", color, egui::TextureOptions::LINEAR));
        self.textures.crop_gen = generation;
    }
}

fn overlay_id(slot: usize) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("stillshot-overlay", slot))
}

/// Which capture each overlay slot shows, and whether that is certain.
///
/// Slot `i` is the windowing layer's monitor `i`. On Windows that order is
/// `EnumDisplayMonitors`, which [`platform::monitor_rects`] repeats, so the
/// pairing is known before the first frame and no overlay ever starts out
/// showing another monitor's picture.
fn initial_slot_map(desktop: &Desktop) -> (Vec<usize>, bool) {
    let n = desktop.monitors.len();
    let identity: Vec<usize> = (0..n).collect();
    if n <= 1 {
        return (identity, true);
    }
    let rects = platform::monitor_rects();
    if rects.len() != n {
        return (identity, false);
    }
    let mut map = Vec::with_capacity(n);
    for (x, y, _, _) in rects {
        let found = desktop
            .monitors
            .iter()
            .position(|m| m.raw_x == x && m.raw_y == y);
        match found {
            Some(image) if !map.contains(&image) => map.push(image),
            _ => return (identity, false),
        }
    }
    (map, true)
}

/// The overlay slot on the monitor under the mouse cursor, or 0.
fn cursor_slot(desktop: &Desktop, slot_monitor: &[usize]) -> usize {
    let Some((cx, cy)) = platform::cursor_position() else {
        return 0;
    };
    desktop
        .monitors
        .iter()
        .position(|m| {
            cx >= m.raw_x
                && cy >= m.raw_y
                && cx < m.raw_x + m.raw_w as i32
                && cy < m.raw_y + m.raw_h as i32
        })
        .and_then(|image| slot_monitor.iter().position(|m| *m == image))
        .unwrap_or(0)
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/Inter-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());
    ctx.set_fonts(fonts);
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgb(22, 23, 26);
    visuals.panel_fill = Color32::from_rgb(22, 23, 26);
    visuals.extreme_bg_color = Color32::from_rgb(14, 15, 18);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 42, 48);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 60, 68);
    visuals.widgets.active.bg_fill = Color32::from_rgb(255, 77, 46);
    visuals.selection.bg_fill = Color32::from_rgb(255, 77, 46);
    visuals.override_text_color = Some(Color32::from_rgb(236, 237, 241));
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    });
}

fn spawn_event_threads(ctx: egui::Context) {
    std::thread::spawn(move || {
        loop {
            match GlobalHotKeyEvent::receiver().recv() {
                Ok(event) if event.state == HotKeyState::Pressed => {
                    if event.id == PANIC_HOTKEY_ID.load(Ordering::SeqCst) {
                        request_panic_quit();
                    } else {
                        request_capture();
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    let menu_ctx = ctx;
    std::thread::spawn(move || {
        loop {
            match MenuEvent::receiver().recv() {
                Ok(event) => {
                    MENU_EVENTS
                        .lock()
                        .unwrap_or_else(|err| err.into_inner())
                        .push(event);
                    menu_ctx.request_repaint();
                }
                Err(_) => break,
            }
        }
    });
}

fn build_tray() -> (Option<TrayIcon>, MenuId, MenuId, MenuId) {
    let capture = MenuItem::new("Take screenshot", true, None);
    let settings = MenuItem::new("Settings…", true, None);
    let quit = MenuItem::new("Quit Stillshot", true, None);
    let capture_id = capture.id().clone();
    let settings_id = settings.id().clone();
    let quit_id = quit.id().clone();
    let menu = Menu::new();
    let _ = menu.append(&capture);
    let _ = menu.append(&settings);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit);
    let (rgba, w, h) = icon::icon_rgba(32);
    let icon = match tray_icon::Icon::from_rgba(rgba, w, h) {
        Ok(icon) => icon,
        Err(err) => {
            log(format!("tray icon: {err}"));
            return (None, capture_id, settings_id, quit_id);
        }
    };
    // Linux uses the KSNI backend (its own D-Bus thread), so the icon can be
    // created on the UI thread next to the winit loop. Windows and macOS need
    // that same thread because their tray APIs follow the process event loop.
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Stillshot — drag a region, it copies")
        .with_icon(icon)
        .build()
        .map_err(|err| log(format!("tray: {err}")))
        .ok();
    (tray, capture_id, settings_id, quit_id)
}

impl App {
    pub(crate) fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }
    pub(crate) fn texture(&self, monitor: usize) -> Option<&TextureHandle> {
        self.textures.monitors.get(monitor).and_then(|t| t.as_ref())
    }
    pub(crate) fn crop_texture(&self) -> Option<&TextureHandle> {
        self.textures.crop.as_ref()
    }
    pub(crate) fn handle_keys(&mut self, ctx: &egui::Context, monitor_index: usize) {
        if self.key_stamp == self.stamp {
            return;
        }
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        // Release, not press: the whole click stays with the overlay, so the
        // button-up cannot open a context menu in the app underneath.
        let right = ctx.input(|i| i.pointer.secondary_released());
        if escape || right {
            self.key_stamp = self.stamp;
            log(if right {
                "right-click closed the overlay"
            } else {
                "Esc closed the overlay"
            });
            self.close_session();
            return;
        }
        let typing = self.session.as_ref().is_some_and(|s| s.text.is_some());
        let enter = ctx.input(|i| i.key_pressed(Key::Enter));
        let command_c = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::C));
        let command_s = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::S));
        let undo =
            ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::Z));
        let redo = ctx.input(|i| {
            (i.modifiers.command && i.modifiers.shift && i.key_pressed(Key::Z))
                || (i.modifiers.command && i.key_pressed(Key::Y))
        });
        let double = ctx.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
        });
        if enter || command_c || command_s || undo || redo || double {
            self.key_stamp = self.stamp;
        }
        if typing {
            if enter {
                self.commit_text();
            }
            return;
        }
        if undo {
            self.undo();
            return;
        }
        if redo {
            self.redo();
            return;
        }
        if command_s {
            self.save_as();
            return;
        }
        if command_c || enter {
            self.confirm_and_close(monitor_index);
            return;
        }
        // The second press of a double-click has already started a (tiny)
        // drag by the time its release reports the double-click.
        let no_real_drag = |s: &Session| {
            s.drag
                .is_none_or(|(a, b)| (a.x - b.x).abs() < 3.0 && (a.y - b.y).abs() < 3.0)
        };
        if double
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.selection.is_none() && no_real_drag(s))
        {
            self.copy_whole_monitor(monitor_index);
            self.close_session();
            return;
        }
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.selection.is_some() && s.text.is_none())
        {
            self.tool_shortcuts(ctx);
        }
    }

    fn tool_shortcuts(&mut self, ctx: &egui::Context) {
        let key = ctx.input(|i| {
            if i.modifiers.command || i.modifiers.alt {
                return None;
            }
            [
                (Key::P, Tool::Pen),
                (Key::M, Tool::Marker),
                (Key::L, Tool::Line),
                (Key::A, Tool::Arrow),
                (Key::R, Tool::Rect),
                (Key::O, Tool::Ellipse),
                (Key::T, Tool::Text),
                (Key::B, Tool::Blur),
                (Key::X, Tool::Pixelate),
                (Key::I, Tool::Eyedropper),
            ]
            .into_iter()
            .find(|(key, _)| i.key_pressed(*key))
            .map(|(_, tool)| tool)
        });
        if let Some(tool) = key {
            if let Some(session) = &mut self.session {
                session.tool = tool;
                session.draft = None;
            }
        }
        let delta = ctx.input(|i| {
            if i.key_pressed(Key::OpenBracket) {
                Some(-1.0)
            } else if i.key_pressed(Key::CloseBracket) {
                Some(1.0)
            } else {
                None
            }
        });
        if let Some(delta) = delta {
            if let Some(session) = &mut self.session {
                session.stroke = (session.stroke + delta).clamp(1.0, 24.0);
            }
        }
    }

    pub(crate) fn pointer(&mut self, ctx: &egui::Context, monitor_index: usize, local: Pos2) {
        self.hover_monitor = monitor_index;
        let global = self.to_global(monitor_index, local);
        let (pressed, down, released, shift, origin) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.modifiers.shift,
                i.pointer.press_origin(),
            )
        });
        let typing = self.session.as_ref().is_some_and(|s| s.text.is_some());
        if pressed && !typing && !self.hits_toolbar(global) {
            let start = origin
                .map(|pos| self.to_global(monitor_index, pos))
                .unwrap_or(global);
            self.on_press(start);
        }
        if (down || released)
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.drag.is_some() || s.draft.is_some())
        {
            self.on_drag(global, shift);
        }
        if released {
            self.on_release();
        }
    }

    fn hits_toolbar(&self, global: Pos2) -> bool {
        self.session
            .as_ref()
            .and_then(|s| s.toolbar)
            .is_some_and(|rect| rect.contains(global.x, global.y))
    }

    fn on_press(&mut self, global: Pos2) {
        let tool = self
            .session
            .as_ref()
            .map(|s| s.tool)
            .unwrap_or(Tool::Select);
        let inside = self
            .session
            .as_ref()
            .and_then(|s| s.selection.as_ref())
            .is_some_and(|sel| sel.rect.contains(global.x, global.y));
        if inside && tool == Tool::Eyedropper {
            self.pick_color(global);
            return;
        }
        if inside && tool == Tool::Text {
            self.begin_text(global);
            return;
        }
        if inside && tool.draws() {
            let pt = self.to_crop(global);
            if let Some(session) = &mut self.session {
                session.draft = Some(match tool {
                    Tool::Pen | Tool::Marker => Draft::Stroke(vec![pt]),
                    Tool::Line => Draft::Line(pt, pt),
                    Tool::Arrow => Draft::Arrow(pt, pt),
                    Tool::Rect => Draft::Rect(pt, pt),
                    Tool::Ellipse => Draft::Ellipse(pt, pt),
                    Tool::Blur => Draft::Blur(pt, pt),
                    Tool::Pixelate => Draft::Pixelate(pt, pt),
                    _ => return,
                });
            }
            return;
        }
        if let Some(session) = &mut self.session {
            session.selection = None;
            session.draft = None;
            session.tool = Tool::Select;
            session.drag = Some((global, global));
            session.toolbar = None;
        }
        self.textures.crop = None;
    }

    fn on_drag(&mut self, global: Pos2, shift: bool) {
        let Some(session) = &mut self.session else {
            return;
        };
        if let Some(drag) = &mut session.drag {
            drag.1 = global;
            return;
        }
        let Some(selection) = &session.selection else {
            return;
        };
        let mut pt = Pt {
            x: (global.x - selection.rect.x) * selection.scale,
            y: (global.y - selection.rect.y) * selection.scale,
        };
        if let Some(draft) = &mut session.draft {
            match draft {
                Draft::Stroke(points) => points.push(pt),
                Draft::Line(a, b) | Draft::Arrow(a, b) => {
                    if shift {
                        let (x, y) = constrain_line((a.x, a.y), (pt.x, pt.y));
                        pt = Pt { x, y };
                    }
                    *b = pt;
                }
                Draft::Rect(a, b)
                | Draft::Ellipse(a, b)
                | Draft::Blur(a, b)
                | Draft::Pixelate(a, b) => {
                    if shift {
                        let (x, y) = constrain_square((a.x, a.y), (pt.x, pt.y));
                        pt = Pt { x, y };
                    }
                    *b = pt;
                }
            }
        }
    }

    fn on_release(&mut self) {
        let drag = self.session.as_mut().and_then(|s| s.drag.take());
        let draft = self.session.as_mut().and_then(|s| s.draft.take());
        if let Some((a, b)) = drag {
            let rect = RectF::from_points((a.x, a.y), (b.x, b.y));
            if rect.w >= 3.0 && rect.h >= 3.0 {
                self.commit_selection(rect);
            }
        } else if let Some(draft) = draft {
            self.commit_draft(draft);
        }
    }

    fn commit_selection(&mut self, rect: RectF) {
        let Some(session) = &self.session else { return };
        let image = desktop::crop(&session.desktop.monitors, rect);
        let placed: Vec<_> = session
            .desktop
            .monitors
            .iter()
            .map(|m| m.placed())
            .collect();
        let scale = output_scale(&placed, rect).max(0.1);
        let copy = image.clone();
        if let Some(session) = &mut self.session {
            session.selection = Some(Selection {
                rect,
                scale,
                base: image.clone(),
                rendered: image,
                doc: Document::default(),
            });
            session.render_gen = session.render_gen.wrapping_add(1);
            session.tool = Tool::Select;
        }
        self.publish(&copy, true);
        self.toast("Copied");
    }

    fn commit_draft(&mut self, draft: Draft) {
        let Some(session) = &self.session else { return };
        let scale = session
            .selection
            .as_ref()
            .map(|sel| sel.scale)
            .unwrap_or(1.0);
        let stroke_px = session.stroke * scale;
        let color = session.color;
        let tool = session.tool;
        let annotation = match draft {
            Draft::Stroke(points) => {
                let points = simplify_stroke(&points);
                if points.is_empty() {
                    return;
                }
                let shape = if tool == Tool::Marker {
                    Shape::Marker { points }
                } else {
                    Shape::Pen { points }
                };
                Annotation {
                    shape,
                    color,
                    stroke: stroke_px,
                }
            }
            Draft::Line(a, b) => Annotation {
                shape: Shape::Line { a, b },
                color,
                stroke: stroke_px,
            },
            Draft::Arrow(a, b) => Annotation {
                shape: Shape::Arrow { a, b },
                color,
                stroke: stroke_px,
            },
            Draft::Rect(a, b) => Annotation {
                shape: Shape::Rect { a, b },
                color,
                stroke: stroke_px,
            },
            Draft::Ellipse(a, b) => Annotation {
                shape: Shape::Ellipse { a, b },
                color,
                stroke: stroke_px,
            },
            Draft::Blur(a, b) => Annotation {
                shape: Shape::Blur {
                    a,
                    b,
                    radius: stroke_px.max(2.0),
                },
                color,
                stroke: stroke_px,
            },
            Draft::Pixelate(a, b) => Annotation {
                shape: Shape::Pixelate {
                    a,
                    b,
                    block: (stroke_px * 2.0).max(4.0),
                },
                color,
                stroke: stroke_px,
            },
        };
        self.push_annotation(annotation);
    }

    fn begin_text(&mut self, global: Pos2) {
        let Some(pt) = self.crop_point(global) else {
            return;
        };
        if let Some(session) = &mut self.session {
            session.text = Some(TextDraft {
                at: pt,
                body: String::new(),
            });
        }
    }

    fn commit_text(&mut self) {
        let (at, body, size, color, stroke) = {
            let Some(session) = &self.session else { return };
            let Some(text) = &session.text else { return };
            let scale = session.selection.as_ref().map(|s| s.scale).unwrap_or(1.0);
            (
                text.at,
                text.body.clone(),
                (16.0 + session.stroke) * scale,
                session.color,
                session.stroke * scale,
            )
        };
        if let Some(session) = &mut self.session {
            session.text = None;
        }
        if body.trim().is_empty() {
            return;
        }
        self.push_annotation(Annotation {
            shape: Shape::Text {
                at,
                text: body,
                size,
            },
            color,
            stroke,
        });
    }

    fn push_annotation(&mut self, annotation: Annotation) {
        let image = {
            let Some(session) = &mut self.session else {
                return;
            };
            let Some(selection) = &mut session.selection else {
                return;
            };
            selection.doc.push(annotation);
            selection.rendered = paint::render(&selection.base, selection.doc.items());
            session.render_gen = session.render_gen.wrapping_add(1);
            selection.rendered.clone()
        };
        self.publish(&image, false);
        self.toast("Copied");
    }

    fn undo(&mut self) {
        self.step_history(true);
    }

    fn redo(&mut self) {
        self.step_history(false);
    }

    fn step_history(&mut self, undo: bool) {
        let image = {
            let Some(session) = &mut self.session else {
                return;
            };
            let Some(selection) = &mut session.selection else {
                return;
            };
            let changed = if undo {
                selection.doc.undo()
            } else {
                selection.doc.redo()
            };
            if !changed {
                return;
            }
            selection.rendered = paint::render(&selection.base, selection.doc.items());
            session.render_gen = session.render_gen.wrapping_add(1);
            selection.rendered.clone()
        };
        self.publish(&image, false);
        self.toast("Copied");
    }

    fn confirm_and_close(&mut self, monitor: usize) {
        if self.session.as_ref().is_some_and(|s| s.selection.is_some()) {
            if let Some(image) = self
                .session
                .as_ref()
                .and_then(|s| s.selection.as_ref().map(|sel| sel.rendered.clone()))
            {
                self.publish(&image, false);
            }
            self.close_session();
        } else {
            self.copy_whole_monitor(monitor);
            self.close_session();
        }
    }

    fn copy_whole_monitor(&mut self, index: usize) {
        let Some(image) = self
            .session
            .as_ref()
            .and_then(|s| s.desktop.monitors.get(index).map(|m| m.image.clone()))
        else {
            return;
        };
        self.publish(&image, true);
        self.toast("Copied screen");
    }

    fn publish(&mut self, image: &RgbaImage, new_file: bool) {
        let started = Instant::now();
        match clipboard::copy_image(image) {
            Ok(()) => log(format!(
                "clipboard {}x{} in {} ms ({})",
                image.width(),
                image.height(),
                started.elapsed().as_millis(),
                clipboard::format_names().join(", ")
            )),
            Err(err) => {
                log(format!("clipboard failed: {err}"));
                if let Some(session) = &mut self.session {
                    session.error = Some(format!("Couldn't copy to the clipboard: {err}"));
                }
                return;
            }
        }
        if !self.config.save_to_disk {
            return;
        }
        let dir = save::resolve_dir(self.config.save_dir.as_deref());
        let png = match clipboard::encode_png(image) {
            Ok(png) => png,
            Err(err) => {
                log(format!("png for disk: {err}"));
                return;
            }
        };
        if new_file
            || self
                .session
                .as_ref()
                .and_then(|s| s.saved_path.as_ref())
                .is_none()
        {
            match save::write_png(&dir, &png) {
                Ok(path) => {
                    log(format!("saved {}", path.display()));
                    if let Some(session) = &mut self.session {
                        session.saved_path = Some(path);
                    }
                }
                Err(err) => log(format!("save failed: {err}")),
            }
        } else if let Some(path) = self.session.as_ref().and_then(|s| s.saved_path.clone()) {
            if let Err(err) = std::fs::write(&path, png) {
                log(format!("update saved file: {err}"));
            }
        }
    }

    fn toast(&mut self, text: &str) {
        if let Some(session) = &mut self.session {
            session.toast = text.to_string();
            session.toast_until = Some(Instant::now() + Duration::from_millis(1200));
        }
    }

    /// "Save as" from the overlay. The overlay is topmost and fullscreen, so a
    /// file dialog opened now would sit underneath it. Close the freeze first;
    /// [`Self::run_pending_save`] opens the dialog once the windows are gone.
    fn save_as(&mut self) {
        let Some(image) = self.session.as_ref().and_then(|s| {
            s.selection
                .as_ref()
                .map(|sel| sel.rendered.clone())
                .or_else(|| s.desktop.monitors.first().map(|m| m.image.clone()))
        }) else {
            return;
        };
        log("save as: closing the overlay before the file dialog opens");
        self.pending_save = Some(PendingSave {
            image,
            waited_frames: 0,
        });
        self.close_session();
    }

    fn run_pending_save(&mut self, ctx: &egui::Context) {
        if self.session.is_some() {
            return;
        }
        let Some(pending) = self.pending_save.as_mut() else {
            return;
        };
        if pending.waited_frames < 1 {
            // Give the overlay windows one frame to be destroyed.
            pending.waited_frames += 1;
            ctx.request_repaint();
            return;
        }
        let Some(pending) = self.pending_save.take() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save screenshot")
            .set_file_name(save::filename_for(chrono::Local::now()))
            .add_filter("PNG image", &["png"])
            .save_file()
        else {
            return;
        };
        let written = clipboard::encode_png(&pending.image)
            .and_then(|bytes| std::fs::write(&path, bytes).map_err(anyhow::Error::from));
        match written {
            Ok(()) => {
                log(format!("saved {}", path.display()));
                self.status = format!("Saved {}", path.display());
            }
            Err(err) => {
                log(format!("save as failed: {err}"));
                self.status = format!("Save failed: {err}");
            }
        }
    }

    fn pick_color(&mut self, global: Pos2) {
        let Some(session) = &self.session else { return };
        let color = sample_color(session, global);
        if let Some(session) = &mut self.session {
            session.color = color;
            session.tool = Tool::Pen;
        }
    }

    fn crop_point(&self, global: Pos2) -> Option<Pt> {
        let session = self.session.as_ref()?;
        let selection = session.selection.as_ref()?;
        Some(Pt {
            x: (global.x - selection.rect.x) * selection.scale,
            y: (global.y - selection.rect.y) * selection.scale,
        })
    }

    pub(crate) fn to_global(&self, monitor_index: usize, local: Pos2) -> Pos2 {
        let Some(monitor) = self
            .session
            .as_ref()
            .and_then(|s| s.desktop.monitors.get(monitor_index))
        else {
            return local;
        };
        Pos2::new(monitor.origin_x + local.x, monitor.origin_y + local.y)
    }

    fn to_crop(&self, global: Pos2) -> Pt {
        self.crop_point(global).unwrap_or(Pt { x: 0.0, y: 0.0 })
    }

    pub(crate) fn set_toolbar(&mut self, rect: Option<RectF>) {
        if let Some(session) = &mut self.session {
            session.toolbar = rect;
        }
    }

    pub(crate) fn set_tool(&mut self, tool: Tool) {
        if let Some(session) = &mut self.session {
            session.tool = tool;
            session.draft = None;
        }
    }

    pub(crate) fn set_color(&mut self, color: Color) {
        if let Some(session) = &mut self.session {
            session.color = color;
        }
    }

    pub(crate) fn set_stroke(&mut self, stroke: f32) {
        if let Some(session) = &mut self.session {
            session.stroke = stroke;
        }
    }

    pub(crate) fn toggle_custom_color(&mut self) {
        if let Some(session) = &mut self.session {
            session.show_custom_color = !session.show_custom_color;
        }
    }

    pub(crate) fn text_body_mut(&mut self) -> Option<&mut String> {
        self.session
            .as_mut()
            .and_then(|s| s.text.as_mut().map(|t| &mut t.body))
    }

    pub(crate) fn request_copy(&mut self) {
        let monitor = self.hover_monitor;
        self.confirm_and_close(monitor);
    }

    pub(crate) fn request_close(&mut self) {
        self.close_session();
    }

    pub(crate) fn request_undo(&mut self) {
        self.undo();
    }

    pub(crate) fn request_redo(&mut self) {
        self.redo();
    }

    pub(crate) fn request_save(&mut self) {
        self.save_as();
    }
}

fn sample_color(session: &Session, global: Pos2) -> Color {
    for monitor in &session.desktop.monitors {
        let rect = monitor.placed().rect();
        if !rect.contains(global.x, global.y) {
            continue;
        }
        let scale_x = monitor.placed().scale_x();
        let scale_y = monitor.placed().scale_y();
        let x = ((global.x - monitor.origin_x) * scale_x).round().max(0.0) as u32;
        let y = ((global.y - monitor.origin_y) * scale_y).round().max(0.0) as u32;
        if x < monitor.image.width() && y < monitor.image.height() {
            let pixel = monitor.image.get_pixel(x, y).0;
            return Color {
                r: pixel[0],
                g: pixel[1],
                b: pixel[2],
                a: 255,
            };
        }
    }
    session.color
}

impl App {
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading("Stillshot");
            ui.label("Freeze the screen, drag a region, and it's on the clipboard.");
            ui.add_space(12.0);
            ui.label(format!("Shortcut: {}", self.config.shortcut().display()));
            if self.recording {
                ui.colored_label(Color32::from_rgb(255, 77, 46), "Press the new shortcut… Esc cancels");
                self.capture_shortcut(&ctx);
            } else if ui.button("Change shortcut").clicked() {
                self.recording = true;
            }
            if let Some(err) = &self.hotkey_error {
                ui.colored_label(Color32::from_rgb(255, 120, 96), err);
            }
            ui.add_space(8.0);
            if ui.checkbox(&mut self.config.save_to_disk, "Save a copy to Pictures/Screenshots").changed() {
                let _ = config::save(&self.config);
            }
            let dir = save::resolve_dir(self.config.save_dir.as_deref());
            ui.label(dir.display().to_string());
            ui.horizontal(|ui| {
                if ui.button("Change folder").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.config.save_dir = Some(path);
                        let _ = config::save(&self.config);
                    }
                }
                if ui.button("Open folder").clicked() {
                    let _ = std::fs::create_dir_all(&dir);
                    platform::open_path(&dir);
                }
            });
            ui.add_space(8.0);
            if ui.checkbox(&mut self.config.launch_at_login, "Launch when I sign in").changed() {
                if let Err(err) = platform::set_launch_at_login(self.config.launch_at_login) {
                    self.status = err.to_string();
                    self.config.launch_at_login = platform::launch_at_login_enabled();
                }
                let _ = config::save(&self.config);
            }
            #[cfg(target_os = "macos")]
            {
                ui.add_space(8.0);
                ui.separator();
                ui.strong("Screen Recording");
                if platform::screen_recording_granted() {
                    ui.label("Permission granted.");
                } else {
                    ui.label("macOS needs Screen Recording permission so Stillshot can freeze menus, tooltips, and video frames. After you allow it, quit Stillshot and open it again.");
                    if ui.button("Grant access").clicked() {
                        let granted = platform::request_screen_recording();
                        self.status = if granted {
                            "If the overlay is still blank, quit Stillshot and open it once more.".into()
                        } else {
                            "Permission was not granted. You can turn it on in System Settings.".into()
                        };
                    }
                    if ui.button("Open System Settings").clicked() {
                        platform::open_screen_recording_settings();
                    }
                }
            }
            ui.add_space(12.0);
            if ui.button("Take a screenshot").clicked() {
                request_capture();
            }
            if !self.status.is_empty() {
                ui.add_space(8.0);
                ui.label(self.status.clone());
            }
            ui.add_space(16.0);
            ui.separator();
            ui.label("Drag a region and let go — it copies immediately. Annotate after that if you want; each finished mark updates the clipboard. Enter copies and closes. Esc, right-click, or waiting 60 seconds closes the overlay. Ctrl+Alt+Q quits Stillshot.");
        });
    }

    fn capture_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.recording = false;
            return;
        }
        let pressed = ctx.input(|i| {
            let mods = i.modifiers;
            let key = [
                Key::PrintScreen,
                Key::A,
                Key::B,
                Key::C,
                Key::D,
                Key::E,
                Key::F,
                Key::G,
                Key::H,
                Key::I,
                Key::J,
                Key::K,
                Key::L,
                Key::M,
                Key::N,
                Key::O,
                Key::P,
                Key::Q,
                Key::R,
                Key::S,
                Key::T,
                Key::U,
                Key::V,
                Key::W,
                Key::X,
                Key::Y,
                Key::Z,
                Key::Num0,
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
                Key::F1,
                Key::F2,
                Key::F3,
                Key::F4,
                Key::F5,
                Key::F6,
                Key::F7,
                Key::F8,
                Key::F9,
                Key::F10,
                Key::F11,
                Key::F12,
            ]
            .into_iter()
            .find(|key| i.key_pressed(*key));
            key.map(|key| (key, mods.command, mods.shift, mods.alt, mods.ctrl))
        });
        let Some((key, command, shift, alt, ctrl)) = pressed else {
            return;
        };
        let name = format!("{key:?}");
        let Some(code) = hotkey::code_name_from_egui(&name) else {
            self.hotkey_error = Some(format!("{name} can't be used as a global shortcut"));
            self.recording = false;
            return;
        };
        match hotkey::shortcut_from_parts(code, command, ctrl, shift, alt) {
            Ok(shortcut) => {
                self.config.set_shortcut(&shortcut);
                let _ = config::save(&self.config);
                self.hotkey_error = self.apply_hotkey().err();
                self.recording = false;
            }
            Err(err) => {
                self.hotkey_error = Some(err);
                self.recording = false;
            }
        }
    }
}
