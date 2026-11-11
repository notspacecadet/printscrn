//! Windows glue: single instance, DPI awareness, the Print Screen keyboard
//! hook, and the escape hatches that keep a topmost overlay from trapping the
//! desktop.
//!
//! The low-level keyboard hook lives on its own thread with its own message
//! loop. Windows calls a `WH_KEYBOARD_LL` hook on the thread that installed
//! it, and every keystroke in the whole system waits for that call. When the
//! hook sat on the UI thread, a screen grab or a PNG encode stalled the
//! keyboard for every app, and Windows silently removes a hook that keeps
//! timing out. The hook thread only flips atomics and wakes the UI.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT,
    VK_SNAPSHOT, VK_TAB,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, EnumWindows, GetCursorPos, GetMessageW,
    GetWindowThreadProcessId, KBDLLHOOKSTRUCT, MSG, PostMessageW, PostQuitMessage,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_CLOSE,
};

use crate::gate::{async_key_is_down, low_level_key_is_release, rising_edge};

/// `KBDLLHOOKSTRUCT.flags`: the event was injected by `SendInput`.
const LLKHF_INJECTED: u32 = 0x10;
/// `KBDLLHOOKSTRUCT.flags`: Alt is held.
const LLKHF_ALTDOWN: u32 = 0x20;

static INSTANCE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The hook thread exists (or is starting).
static HOOK_RUNNING: AtomicBool = AtomicBool::new(false);
/// Print Screen is the configured shortcut and should be swallowed.
static ARMED: AtomicBool = AtomicBool::new(false);
/// Print Screen was pressed; the UI thread takes this.
static PRESSED: AtomicBool = AtomicBool::new(false);
/// A freeze is on screen (or being grabbed).
static OVERLAY: AtomicBool = AtomicBool::new(false);
/// The hook saw a key that must close the overlay; the UI thread takes this.
static DISMISS: AtomicBool = AtomicBool::new(false);
/// The Esc key-down was swallowed, so its key-up is swallowed too.
static ESC_SWALLOWED: AtomicBool = AtomicBool::new(false);

static ESC_DOWN: AtomicBool = AtomicBool::new(false);
static RIGHT_DOWN: AtomicBool = AtomicBool::new(false);
static PANIC_DOWN: AtomicBool = AtomicBool::new(false);
static WANT_CTRL: AtomicBool = AtomicBool::new(false);
static WANT_ALT: AtomicBool = AtomicBool::new(false);
static WANT_SHIFT: AtomicBool = AtomicBool::new(false);
static WANT_META: AtomicBool = AtomicBool::new(false);

static WAKER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn enable_dpi_awareness() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn acquire_single_instance() -> bool {
    let name: Vec<u16> = "Local\\StillshotSingleton\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if handle.is_null() {
            crate::log::log("CreateMutexW failed; not starting a second hotkey handler");
            return false;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return false;
        }
        // Keep the handle alive for the process lifetime. Closing it would
        // drop the mutex and let a second Stillshot install another hook.
        INSTANCE.store(handle, Ordering::SeqCst);
        true
    }
}

/// Called from the hook thread whenever the UI thread has something to pick up.
pub fn set_waker(waker: Box<dyn Fn() + Send + Sync>) {
    let _ = WAKER.set(waker);
}

fn wake_ui() {
    if let Some(waker) = WAKER.get() {
        waker();
    }
}

pub fn set_overlay_active(active: bool) {
    if active {
        // A dismiss key from before this freeze must not close it.
        DISMISS.store(false, Ordering::SeqCst);
    }
    OVERLAY.store(active, Ordering::SeqCst);
}

/// True once per request to close the overlay: Esc, Win, Alt+Tab or
/// Ctrl+Shift+Esc seen by the hook, or (as a fallback that needs no focus and
/// no hook) Esc going down or the right mouse button coming back up.
pub fn dismiss_pressed() -> bool {
    let hooked = DISMISS.swap(false, Ordering::SeqCst);
    let escape_down = key_down(VK_ESCAPE);
    let escape = rising_edge(ESC_DOWN.swap(escape_down, Ordering::SeqCst), escape_down);
    // Close on release, not on press. Closing on press hands the button-up to
    // whatever is under the cursor once the overlay is gone, and that opens a
    // context menu there.
    let right_down = key_down(VK_RBUTTON);
    let right_released = RIGHT_DOWN.swap(right_down, Ordering::SeqCst) && !right_down;
    hooked || escape || right_released
}

/// Edge-triggered Ctrl+Alt+Q. This is not Ctrl+Shift+Esc and not Ctrl+Alt+Del.
pub fn panic_quit_pressed() -> bool {
    let down = key_down(VK_CONTROL) && key_down(VK_MENU) && key_down(0x51);
    rising_edge(PANIC_DOWN.swap(down, Ordering::SeqCst), down)
}

pub fn emergency_close_windows() {
    unsafe {
        EnumWindows(Some(enum_close), GetCurrentProcessId() as LPARAM);
        PostQuitMessage(0);
    }
}

unsafe extern "system" fn enum_close(hwnd: HWND, pid: LPARAM) -> i32 {
    unsafe {
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == pid as u32 {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
    }
    1
}

/// Monitor rectangles `(x, y, width, height)` in physical pixels, in
/// `EnumDisplayMonitors` order. winit lists monitors with the same call, so
/// index `i` here is the monitor `ViewportBuilder::with_monitor(i)` lands on.
pub fn monitor_rects() -> Vec<(i32, i32, i32, i32)> {
    let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(enum_monitor),
            &mut rects as *mut Vec<(i32, i32, i32, i32)> as LPARAM,
        );
    }
    rects
}

unsafe extern "system" fn enum_monitor(
    _monitor: HMONITOR,
    _dc: HDC,
    rect: *mut RECT,
    data: LPARAM,
) -> i32 {
    unsafe {
        if !rect.is_null() && data != 0 {
            let rects = &mut *(data as *mut Vec<(i32, i32, i32, i32)>);
            let r = &*rect;
            rects.push((r.left, r.top, r.right - r.left, r.bottom - r.top));
        }
    }
    1
}

/// Cursor position in physical pixels of the virtual desktop.
pub fn cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut point) };
    (ok != 0).then_some((point.x, point.y))
}

fn key_down(vk: u16) -> bool {
    unsafe { async_key_is_down(GetAsyncKeyState(vk as i32)) }
}

/// Start the keyboard hook thread if it is not running. Safe to call again.
pub fn start_keyboard_hook() {
    if HOOK_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("stillshot-keyboard-hook".into())
        .spawn(hook_thread);
    if let Err(err) = spawned {
        HOOK_RUNNING.store(false, Ordering::SeqCst);
        crate::log::log(format!("keyboard hook thread failed to start: {err}"));
    }
}

fn hook_thread() {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0);
        if hook.is_null() {
            HOOK_RUNNING.store(false, Ordering::SeqCst);
            crate::log::log(format!(
                "keyboard hook failed to install (error {})",
                GetLastError()
            ));
            return;
        }
        crate::log::log("keyboard hook installed on its own thread");
        // A low-level hook is only called while its thread pumps messages.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
        HOOK_RUNNING.store(false, Ordering::SeqCst);
        crate::log::log("keyboard hook thread ended");
    }
}

pub fn install_print_screen_hook(ctrl: bool, alt: bool, shift: bool, meta: bool) {
    WANT_CTRL.store(ctrl, Ordering::SeqCst);
    WANT_ALT.store(alt, Ordering::SeqCst);
    WANT_SHIFT.store(shift, Ordering::SeqCst);
    WANT_META.store(meta, Ordering::SeqCst);
    ARMED.store(true, Ordering::SeqCst);
    start_keyboard_hook();
}

/// Stop swallowing Print Screen. The hook thread stays: it also carries the
/// overlay's escape keys.
pub fn remove_print_screen_hook() {
    ARMED.store(false, Ordering::SeqCst);
}

pub fn take_print_screen() -> bool {
    PRESSED.swap(false, Ordering::SeqCst)
}

/// Runs on the hook thread for every key event in the system. It must return
/// fast: atomics and one wake-up, nothing else.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let vk = info.vkCode;
        let release = low_level_key_is_release(info.flags);
        let injected = (info.flags & LLKHF_INJECTED) != 0;
        let overlay = OVERLAY.load(Ordering::SeqCst);

        if vk == VK_SNAPSHOT as u32 && ARMED.load(Ordering::SeqCst) && modifiers_match() {
            // Swallow press and release so Snipping Tool does not also open.
            // Repeats while the overlay is up must not queue another capture.
            if !release && !overlay {
                PRESSED.store(true, Ordering::SeqCst);
                wake_ui();
            }
            return 1;
        }

        if vk == VK_ESCAPE as u32 && !injected {
            if release {
                if ESC_SWALLOWED.swap(false, Ordering::SeqCst) {
                    return 1;
                }
            } else if overlay {
                DISMISS.store(true, Ordering::SeqCst);
                wake_ui();
                // Plain Esc belongs to the overlay. Ctrl+Esc (Start) and
                // Ctrl+Shift+Esc (Task Manager) go on to Windows as well.
                if !held(VK_CONTROL) {
                    ESC_SWALLOWED.store(true, Ordering::SeqCst);
                    return 1;
                }
            }
        }

        if overlay && !release && !injected {
            // Keys that take the user somewhere else. The overlay is topmost,
            // so it has to get out of the way; the key itself is passed on.
            let win_key = (vk == VK_LWIN as u32 || vk == VK_RWIN as u32)
                && !WANT_META.load(Ordering::SeqCst);
            let alt_tab = vk == VK_TAB as u32 && (info.flags & LLKHF_ALTDOWN) != 0;
            if win_key || alt_tab {
                DISMISS.store(true, Ordering::SeqCst);
                wake_ui();
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn held(vk: u16) -> bool {
    unsafe { async_key_is_down(GetAsyncKeyState(vk as i32)) }
}

fn modifiers_match() -> bool {
    held(VK_CONTROL) == WANT_CTRL.load(Ordering::SeqCst)
        && held(VK_MENU) == WANT_ALT.load(Ordering::SeqCst)
        && held(VK_SHIFT) == WANT_SHIFT.load(Ordering::SeqCst)
        && (held(VK_LWIN) || held(VK_RWIN)) == WANT_META.load(Ordering::SeqCst)
}
