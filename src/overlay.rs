//! The frozen-frame overlay: the screenshot, the dimmed outside, the selection,
//! a live preview of the mark being drawn, and the annotation toolbar.

use crate::annotate::{Color, Pt, SWATCHES, Tool};
use crate::app::{App, Draft};
use crate::geom::RectF;
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2};

const CORAL: Color32 = Color32::from_rgb(255, 77, 46);
const INK: Color32 = Color32::from_rgb(236, 237, 241);

fn toolbar_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(22, 23, 28, 238)
}

pub fn paint(app: &mut App, ctx: &egui::Context, monitor_index: usize) {
    let screen = ctx.viewport_rect();
    draw_frozen(app, ctx, monitor_index, screen);
    draw_selection(app, ctx, monitor_index, screen);
    draw_draft(app, ctx, monitor_index);
    draw_hints(app, ctx, monitor_index, screen);
    draw_toolbar(app, ctx, monitor_index, screen);
    draw_text_editor(app, ctx, monitor_index);
    app.handle_keys(ctx, monitor_index);
    if let Some(local) = ctx.input(|i| i.pointer.hover_pos().or_else(|| i.pointer.interact_pos())) {
        app.pointer(ctx, monitor_index, local);
    } else if ctx.input(|i| i.pointer.primary_released()) {
        app.pointer(ctx, monitor_index, Pos2::ZERO);
    }
    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
}

fn draw_frozen(app: &App, ctx: &egui::Context, monitor_index: usize, screen: Rect) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    if let Some(texture) = app.texture(monitor_index) {
        painter.image(
            texture.id(),
            screen,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let has_selection = app
        .session()
        .is_some_and(|s| s.selection.is_some() || s.drag.is_some());
    if !has_selection {
        painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(28));
    }
}

fn draw_selection(app: &App, ctx: &egui::Context, monitor_index: usize, screen: Rect) {
    let Some(session) = app.session() else { return };
    let Some(monitor) = session.desktop.monitors.get(monitor_index) else {
        return;
    };
    let rect = if let Some(selection) = &session.selection {
        selection.rect
    } else if let Some((a, b)) = session.drag {
        RectF::from_points((a.x, a.y), (b.x, b.y))
    } else {
        return;
    };
    let Some(local) = global_to_local(
        monitor.origin_x,
        monitor.origin_y,
        monitor.point_w,
        monitor.point_h,
        rect,
    ) else {
        if session.selection.is_some() {
            dim_full(ctx, screen);
        }
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique(("dim", monitor_index)),
    ));
    dim_around(&painter, screen, local);
    if let Some(selection) = &session.selection {
        if let Some(texture) = app.crop_texture() {
            let u0 = ((rect.x - selection.rect.x) / selection.rect.w).clamp(0.0, 1.0);
            let v0 = ((rect.y - selection.rect.y) / selection.rect.h).clamp(0.0, 1.0);
            let u1 = ((rect.x + rect.w - selection.rect.x) / selection.rect.w).clamp(0.0, 1.0);
            let v1 = ((rect.y + rect.h - selection.rect.y) / selection.rect.h).clamp(0.0, 1.0);
            // `rect` here is the selection itself, so the UV is the full image.
            let _ = (u0, v0, u1, v1);
            painter.image(
                texture.id(),
                local,
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
    painter.rect_stroke(
        local,
        CornerRadius::ZERO,
        Stroke::new(1.5, Color32::WHITE),
        egui::StrokeKind::Inside,
    );
    painter.rect_stroke(
        local.expand(1.0),
        CornerRadius::ZERO,
        Stroke::new(1.0, Color32::from_black_alpha(160)),
        egui::StrokeKind::Outside,
    );
    for corner in [
        local.min,
        Pos2::new(local.max.x, local.min.y),
        Pos2::new(local.min.x, local.max.y),
        local.max,
    ] {
        painter.rect_filled(
            Rect::from_center_size(corner, Vec2::splat(7.0)),
            CornerRadius::ZERO,
            Color32::WHITE,
        );
    }
    if session.drag.is_some() {
        if let Some(selection_scale) = session
            .selection
            .as_ref()
            .map(|s| s.scale)
            .or(Some(monitor.placed().scale_x()))
        {
            let label = format!(
                "{} × {}",
                (rect.w * selection_scale).round() as i32,
                (rect.h * selection_scale).round() as i32
            );
            let pos = Pos2::new(local.min.x, (local.min.y - 28.0).max(8.0));
            paint_pill(&painter, pos, &label);
        }
    }
}

fn draw_draft(app: &App, ctx: &egui::Context, monitor_index: usize) {
    let Some(session) = app.session() else { return };
    let Some(draft) = &session.draft else { return };
    let Some(selection) = &session.selection else {
        return;
    };
    let Some(monitor) = session.desktop.monitors.get(monitor_index) else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique(("draft", monitor_index)),
    ));
    let color = to_color(session.color);
    let stroke = Stroke::new(session.stroke.max(1.0), color);
    let map = |pt: Pt| {
        crop_to_local(
            selection.rect,
            selection.scale,
            monitor.origin_x,
            monitor.origin_y,
            pt,
        )
    };
    match draft {
        Draft::Stroke(points) => {
            let mapped: Vec<Pos2> = points.iter().copied().map(map).collect();
            if mapped.len() >= 2 {
                painter.add(egui::Shape::line(mapped, stroke));
            } else if let Some(point) = mapped.first() {
                painter.circle_filled(*point, session.stroke * 0.5, color);
            }
        }
        Draft::Line(a, b) | Draft::Arrow(a, b) => {
            painter.line_segment([map(*a), map(*b)], stroke);
        }
        Draft::Rect(a, b) | Draft::Blur(a, b) | Draft::Pixelate(a, b) => {
            painter.rect_stroke(
                rect_from(*a, *b, map),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Draft::Ellipse(a, b) => {
            let rect = rect_from(*a, *b, map);
            painter.circle_stroke(rect.center(), rect.width().min(rect.height()) * 0.5, stroke);
        }
    }
}

fn draw_hints(app: &App, ctx: &egui::Context, _monitor_index: usize, screen: Rect) {
    let Some(session) = app.session() else { return };
    if session.selection.is_some() || session.drag.is_some() {
        if let Some(until) = session.toast_until {
            if until > std::time::Instant::now() {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique("toast"),
                ));
                let galley = painter.layout_no_wrap(
                    session.toast.clone(),
                    FontId::proportional(14.0),
                    Color32::WHITE,
                );
                let size = galley.size() + Vec2::new(28.0, 14.0);
                let rect = Rect::from_center_size(Pos2::new(screen.center().x, 36.0), size);
                painter.rect_filled(rect, CornerRadius::same(8), CORAL);
                painter.galley(
                    Pos2::new(rect.min.x + 14.0, rect.min.y + 7.0),
                    galley,
                    Color32::WHITE,
                );
            }
        }
        if let Some(error) = &session.error {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("err"),
            ));
            paint_pill(&painter, Pos2::new(16.0, 16.0), error);
        }
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("hint"),
    ));
    let text = "Drag a region — it copies when you let go";
    let sub = "Double-click or Enter copies this screen    ·    Esc or right-click closes";
    let galley =
        painter.layout_no_wrap(text.to_owned(), FontId::proportional(18.0), Color32::WHITE);
    let subgalley = painter.layout_no_wrap(
        sub.to_owned(),
        FontId::proportional(13.0),
        Color32::from_rgb(220, 220, 224),
    );
    let galley_size = galley.size();
    let sub_size = subgalley.size();
    let width = galley_size.x.max(sub_size.x) + 36.0;
    let height = galley_size.y + sub_size.y + 22.0;
    let rect = Rect::from_center_size(
        Pos2::new(screen.center().x, screen.max.y - 72.0),
        Vec2::new(width, height),
    );
    painter.rect_filled(
        rect,
        CornerRadius::same(12),
        Color32::from_rgba_unmultiplied(16, 16, 18, 210),
    );
    painter.galley(
        Pos2::new(rect.center().x - galley_size.x * 0.5, rect.min.y + 10.0),
        galley,
        Color32::WHITE,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - sub_size.x * 0.5,
            rect.min.y + 14.0 + galley_size.y,
        ),
        subgalley,
        Color32::WHITE,
    );
}

fn draw_toolbar(app: &mut App, ctx: &egui::Context, monitor_index: usize, screen: Rect) {
    let Some((local, tool, color, stroke, can_undo, can_redo, show_custom, origin_x, origin_y)) =
        (|| {
            let session = app.session()?;
            let selection = session.selection.as_ref()?;
            let monitor = session.desktop.monitors.get(monitor_index)?;
            let anchor_x = selection.rect.x;
            let anchor_y = selection.rect.bottom() + 10.0;
            let on_this = anchor_x >= monitor.origin_x
                && anchor_y >= monitor.origin_y
                && anchor_x < monitor.origin_x + monitor.point_w
                && anchor_y < monitor.origin_y + monitor.point_h + 80.0;
            if !on_this {
                return None;
            }
            Some((
                Pos2::new(anchor_x - monitor.origin_x, anchor_y - monitor.origin_y),
                session.tool,
                session.color,
                session.stroke,
                selection.doc.can_undo(),
                selection.doc.can_redo(),
                session.show_custom_color,
                monitor.origin_x,
                monitor.origin_y,
            ))
        })()
    else {
        return;
    };
    let response = egui::Area::new(egui::Id::unique("stillshot-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(
            local.x.clamp(8.0, (screen.width() - 640.0).max(8.0)),
            local.y.min(screen.height() - 64.0).max(8.0),
        ))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(toolbar_fill())
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                        for tool_id in [
                            Tool::Pen,
                            Tool::Marker,
                            Tool::Line,
                            Tool::Arrow,
                            Tool::Rect,
                            Tool::Ellipse,
                            Tool::Text,
                            Tool::Blur,
                            Tool::Pixelate,
                            Tool::Eyedropper,
                        ] {
                            if tool_button(ui, tool_id, tool == tool_id) {
                                app.set_tool(tool_id);
                            }
                        }
                        ui.add_space(6.0);
                        for swatch in SWATCHES {
                            if color_dot(ui, swatch, color == swatch) {
                                app.set_color(swatch);
                            }
                        }
                        if ui.button("RGB").clicked() {
                            app.toggle_custom_color();
                        }
                        ui.add_space(6.0);
                        let mut value = stroke;
                        let label = if tool == Tool::Text { "Size" } else { "Width" };
                        ui.label(label);
                        if ui
                            .add(egui::Slider::new(&mut value, 1.0..=24.0).show_value(false))
                            .changed()
                        {
                            app.set_stroke(value);
                        }
                        ui.add_space(6.0);
                        if ui
                            .add_enabled(can_undo, egui::Button::new("Undo"))
                            .clicked()
                        {
                            app.request_undo();
                        }
                        if ui
                            .add_enabled(can_redo, egui::Button::new("Redo"))
                            .clicked()
                        {
                            app.request_redo();
                        }
                        ui.add_space(6.0);
                        let copy =
                            egui::Button::new(egui::RichText::new("Copy").color(Color32::WHITE))
                                .fill(CORAL);
                        if ui.add(copy).clicked() {
                            app.request_copy();
                        }
                        if ui.button("Save").clicked() {
                            app.request_save();
                        }
                        if ui.button("Close").clicked() {
                            app.request_close();
                        }
                    });
                    if show_custom {
                        ui.add_space(4.0);
                        let mut r = color.r as f32;
                        let mut g = color.g as f32;
                        let mut b = color.b as f32;
                        ui.horizontal(|ui| {
                            ui.label("R");
                            ui.add(egui::Slider::new(&mut r, 0.0..=255.0).show_value(false));
                            ui.label("G");
                            ui.add(egui::Slider::new(&mut g, 0.0..=255.0).show_value(false));
                            ui.label("B");
                            ui.add(egui::Slider::new(&mut b, 0.0..=255.0).show_value(false));
                        });
                        app.set_color(Color {
                            r: r as u8,
                            g: g as u8,
                            b: b as u8,
                            a: 255,
                        });
                    }
                });
            ui.min_rect()
        });
    let toolbar = RectF {
        x: response.response.rect.min.x + origin_x,
        y: response.response.rect.min.y + origin_y,
        w: response.response.rect.width(),
        h: response.response.rect.height(),
    };
    app.set_toolbar(Some(toolbar));
}

fn draw_text_editor(app: &mut App, ctx: &egui::Context, monitor_index: usize) {
    let Some((local, mut body)) = (|| {
        let session = app.session()?;
        let text = session.text.as_ref()?;
        let selection = session.selection.as_ref()?;
        let monitor = session.desktop.monitors.get(monitor_index)?;
        let local = crop_to_local(
            selection.rect,
            selection.scale,
            monitor.origin_x,
            monitor.origin_y,
            text.at,
        );
        if local.x < -20.0
            || local.y < -20.0
            || local.x > monitor.point_w
            || local.y > monitor.point_h
        {
            return None;
        }
        Some((local, text.body.clone()))
    })() else {
        return;
    };
    egui::Area::new(egui::Id::unique("stillshot-text"))
        .order(egui::Order::Tooltip)
        .fixed_pos(local)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(Color32::from_rgba_unmultiplied(20, 20, 22, 220))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut body)
                            .hint_text("Label")
                            .desired_width(180.0)
                            .font(egui::TextStyle::Heading),
                    );
                    // Type straight away; no extra click into the field.
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                });
        });
    if let Some(slot) = app.text_body_mut() {
        *slot = body;
    }
}

fn tool_button(ui: &mut egui::Ui, tool: Tool, active: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let fill = if active {
        CORAL
    } else if response.hovered() {
        Color32::from_rgb(54, 56, 64)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    paint_icon(
        ui.painter(),
        rect,
        tool,
        if active { Color32::WHITE } else { INK },
    );
    let shortcut = tool
        .shortcut()
        .map(|ch| format!(" ({ch})"))
        .unwrap_or_default();
    response
        .on_hover_text(format!("{}{shortcut}", tool.label()))
        .clicked()
}

fn color_dot(ui: &mut egui::Ui, color: Color, active: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    if active {
        ui.painter()
            .circle_stroke(rect.center(), 9.0, Stroke::new(2.0, Color32::WHITE));
    }
    ui.painter()
        .circle_filled(rect.center(), 7.0, to_color(color));
    response.clicked()
}

fn paint_icon(painter: &egui::Painter, rect: Rect, tool: Tool, color: Color32) {
    let c = rect.center();
    let s = 7.0;
    let stroke = Stroke::new(1.6, color);
    match tool {
        Tool::Pen => {
            painter.line_segment([c + Vec2::new(-s, s), c + Vec2::new(s * 0.8, -s)], stroke);
        }
        Tool::Marker => {
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::new(16.0, 7.0)),
                CornerRadius::same(2),
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 170),
            );
        }
        Tool::Line => {
            painter.line_segment([c + Vec2::new(-s, s), c + Vec2::new(s, -s)], stroke);
        }
        Tool::Arrow => {
            painter.line_segment([c + Vec2::new(-s, 0.0), c + Vec2::new(s, 0.0)], stroke);
            painter.line_segment([c + Vec2::new(s, 0.0), c + Vec2::new(2.0, -5.0)], stroke);
            painter.line_segment([c + Vec2::new(s, 0.0), c + Vec2::new(2.0, 5.0)], stroke);
        }
        Tool::Rect => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(13.0)),
                CornerRadius::ZERO,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Tool::Ellipse => {
            painter.circle_stroke(c, 6.5, stroke);
        }
        Tool::Text => {
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                "T",
                FontId::proportional(15.0),
                color,
            );
        }
        Tool::Blur => {
            for i in 0..3 {
                painter.circle_stroke(
                    c + Vec2::new(i as f32 * 2.0 - 2.0, 0.0),
                    4.0 - i as f32,
                    Stroke::new(1.0, color),
                );
            }
        }
        Tool::Pixelate => {
            for y in 0..3 {
                for x in 0..3 {
                    if (x + y) % 2 == 0 {
                        let origin = c + Vec2::new(-6.0 + x as f32 * 4.0, -6.0 + y as f32 * 4.0);
                        painter.rect_filled(
                            Rect::from_min_size(origin, Vec2::splat(3.5)),
                            CornerRadius::ZERO,
                            color,
                        );
                    }
                }
            }
        }
        Tool::Eyedropper => {
            painter.line_segment([c + Vec2::new(-4.0, 5.0), c + Vec2::new(5.0, -6.0)], stroke);
        }
        Tool::Select => {}
    }
}

fn paint_pill(painter: &egui::Painter, pos: Pos2, text: &str) {
    let font = FontId::proportional(13.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let rect = Rect::from_min_size(pos, galley.size() + Vec2::new(16.0, 8.0));
    painter.rect_filled(
        rect,
        CornerRadius::same(6),
        Color32::from_rgba_unmultiplied(16, 16, 18, 220),
    );
    painter.galley(pos + Vec2::new(8.0, 4.0), galley, Color32::WHITE);
}

fn dim_full(ctx: &egui::Context, screen: Rect) {
    ctx.layer_painter(egui::LayerId::background()).rect_filled(
        screen,
        CornerRadius::ZERO,
        Color32::from_black_alpha(120),
    );
}

fn dim_around(painter: &egui::Painter, screen: Rect, hole: Rect) {
    let shade = Color32::from_black_alpha(130);
    painter.rect_filled(
        Rect::from_min_max(screen.min, Pos2::new(screen.max.x, hole.min.y)),
        CornerRadius::ZERO,
        shade,
    );
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(screen.min.x, hole.max.y), screen.max),
        CornerRadius::ZERO,
        shade,
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(screen.min.x, hole.min.y),
            Pos2::new(hole.min.x, hole.max.y),
        ),
        CornerRadius::ZERO,
        shade,
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(hole.max.x, hole.min.y),
            Pos2::new(screen.max.x, hole.max.y),
        ),
        CornerRadius::ZERO,
        shade,
    );
}

fn global_to_local(origin_x: f32, origin_y: f32, w: f32, h: f32, rect: RectF) -> Option<Rect> {
    let monitor = RectF {
        x: origin_x,
        y: origin_y,
        w,
        h,
    };
    let inter = rect.intersection(monitor)?;
    Some(Rect::from_min_max(
        Pos2::new(inter.x - origin_x, inter.y - origin_y),
        Pos2::new(inter.right() - origin_x, inter.bottom() - origin_y),
    ))
}

fn crop_to_local(sel: RectF, scale: f32, origin_x: f32, origin_y: f32, pt: Pt) -> Pos2 {
    Pos2::new(
        sel.x + pt.x / scale - origin_x,
        sel.y + pt.y / scale - origin_y,
    )
}

fn rect_from(a: Pt, b: Pt, map: impl Fn(Pt) -> Pos2) -> Rect {
    let a = map(a);
    let b = map(b);
    Rect::from_two_pos(a, b)
}

fn to_color(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}
