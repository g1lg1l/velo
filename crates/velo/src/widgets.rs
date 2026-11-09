//! Velo's custom widgets, painted only from theme tokens.
//! From Ravune (bd10814), trimmed for Velo.

use std::f32::consts::{PI, TAU};

use std::collections::VecDeque;

use eframe::egui::{self, Align2, CornerRadius, EventFilter, Key, Mesh, Painter, Pos2, Rect, Response, RichText, Role, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, pos2, vec2};
use velo_core::{Curve, Mapping, ParamInfo};

use crate::i18n;
use crate::kit;
use crate::theme::Theme;

/// Keyboard for a focused value control: Up / Down nudge it (Shift = fine), Delete resets.
/// Returns the new value in 0..1 travel, or `None`.
fn keyboard_travel(ui: &Ui, resp: &Response, travel: f32, steps: Option<f32>) -> Option<f32> {
    if !resp.has_focus() {
        return None;
    }
    ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, EventFilter { vertical_arrows: true, ..Default::default() }));
    let (up, down, fine) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.modifiers.shift));
    let step = steps.map_or(if fine { 0.002 } else { 0.02 }, |n| 1.0 / n);
    match (up, down) {
        (true, false) => Some((travel + step).min(1.0)),
        (false, true) => Some((travel - step).max(0.0)),
        _ => None,
    }
}

fn reset_pressed(ui: &Ui, resp: &Response) -> bool {
    resp.has_focus() && ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
}

const CELL: Vec2 = vec2(58.0, 76.0);

/// An on/off parameter, in a knob's cell so it lines up with them (a mapping's Invert): its
/// name in mono capitals, a key with an arrow up and one down on it that latches down, lit in
/// the signal's green, while it's on, On or Off. Click (or Space) flips it.
pub fn switch(ui: &mut Ui, t: &Theme, info: &ParamInfo, value: f32) -> (Response, Option<f32>) {
    let (rect, resp) = ui.allocate_exact_size(CELL, Sense::click());
    let on = value >= 0.5;
    let new = (resp.clicked() || (resp.has_focus() && ui.input(|i| i.key_pressed(Key::Space)))).then_some(if on { info.min } else { info.max });
    let shown = new.map_or(on, |v| v >= 0.5);
    let name = i18n::t(info.name);
    resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, shown, name));
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    let painter = ui.painter_at(rect.expand(6.0));
    kit::fit_text(&painter, pos2(rect.center().x, rect.top()), Align2::CENTER_TOP, name.to_uppercase(), Theme::mono(t.text_xs), t.fg_muted, rect.width());
    let k = ui.ctx().animate_bool_with_time(resp.id.with("switch"), shown, t.motion);
    let h = kit::hover(ui, t, &resp);
    let down = ui.ctx().animate_bool_with_time(resp.id.with("down"), resp.is_pointer_button_down_on(), t.motion * 0.5);
    let key = Rect::from_center_size(pos2(rect.center().x, rect.top() + 36.0), vec2(38.0, 28.0));
    if k > 0.0 {
        painter.rect_filled(key.expand(4.0), CornerRadius::same(7), t.cat_signal.gamma_multiply(0.15 * k));
    }
    let fill = Theme::mix(Theme::mix(t.bg_control, t.fg_text, 0.08 * h), t.cat_signal, k);
    let face = keycap(&painter, key, fill, down.max(0.6 * k));
    kit::icon_at(&painter, face.center(), crate::icons::ARROWS_DOWN_UP, 15.0, Theme::mix(Theme::mix(t.fg_muted, t.fg_text, h), TE_INK, k));
    let text = if shown { "On" } else { "Off" };
    painter.text(pos2(rect.center().x, rect.bottom()), Align2::CENTER_BOTTOM, i18n::t(text), Theme::mono(t.text_sm), Theme::mix(t.fg_text, t.cat_signal, k));
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand), new)
}

/// An on/off parameter as a row as tall as a slider's field: its name, and a switch on the
/// right. A click anywhere on it (or Space) flips it.
pub fn toggle(ui: &mut Ui, t: &Theme, info: &ParamInfo, value: f32) -> (Response, Option<f32>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_h + 4.0), Sense::click());
    ui.add_space(t.gap);
    let on = value >= 0.5;
    let new = (resp.clicked() || (resp.has_focus() && ui.input(|i| i.key_pressed(Key::Space)))).then_some(if on { info.min } else { info.max });
    let shown = new.map_or(on, |v| v >= 0.5);
    let name = i18n::t(info.name);
    resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, shown, name));
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    let h = kit::hover(ui, t, &resp);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_field, t.fg_text, 0.03 + 0.03 * h));
    painter.text(pos2(rect.left() + t.pad - 2.0, rect.center().y), Align2::LEFT_CENTER, name, t.body(), Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * h));
    let pill = Rect::from_center_size(pos2(rect.right() - t.pad - 16.0, rect.center().y), vec2(32.0, 18.0));
    let k = ui.ctx().animate_bool_with_time(resp.id.with("toggle"), shown, t.motion);
    painter.rect_filled(pill, CornerRadius::same(9), Theme::mix(t.bg_control, t.accent, k));
    let x = pill.left() + 9.0 + k * (pill.width() - 18.0);
    painter.circle_filled(pos2(x, pill.center().y), 6.5, Theme::mix(t.fg_muted, t.fg_text, 0.5 + 0.5 * k));
    (resp, new)
}

/// Rotary knob, a mapping's (attack, release): a cap in the signal's green, its name in mono
/// capitals over it. Drag up/down (Shift = fine), scroll, Cmd-click to reset, as a slider;
/// focused, Up / Down nudge it and Delete resets it. It glows while it turns or its value
/// moves. Returns the new value when the user changed it.
pub fn knob(ui: &mut Ui, t: &Theme, info: &ParamInfo, value: f32) -> (Response, Option<f32>) {
    let name = i18n::t(info.name);
    let (rect, resp) = ui.allocate_exact_size(CELL, Sense::click_and_drag());
    let new = knob_input(ui, &resp, info, value);
    let shown = new.unwrap_or(value);
    resp.widget_info(|| WidgetInfo::slider(true, shown as f64, format!("{name} {}", info.format(shown))));
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    let painter = ui.painter_at(rect.expand(6.0));
    let c = pos2(rect.center().x, rect.top() + 36.0);
    let turning = ui.ctx().animate_bool_with_time(resp.id.with("glow"), resp.dragged(), t.motion);
    let glow = turning.max(moved(ui, t, resp.id.with("moved"), shown));
    paint_knob(&painter, t, c, eased(ui, t, resp.id.with("eased"), info.to_norm(shown), resp.dragged()), t.cat_signal, kit::hover(ui, t, &resp), glow);
    // a long name ends in "…" (the whole of it on hover)
    let resp = if kit::fit_text(&painter, pos2(c.x, rect.top()), Align2::CENTER_TOP, name.to_uppercase(), Theme::mono(t.text_xs), t.fg_muted, rect.width()).1 { resp.on_hover_text(name) } else { resp };
    let resp = knob_value(&painter, t, rect, info.format(shown), Theme::mix(t.fg_text, t.cat_signal, glow), resp);
    (resp, new)
}

/// A knob's value under it: a long one a little smaller to fit, then ending in "…" (the whole
/// of it on hover).
fn knob_value(painter: &Painter, t: &Theme, rect: Rect, text: String, color: egui::Color32, resp: Response) -> Response {
    let mut font = Theme::mono(t.text_sm);
    let wide = painter.layout_no_wrap(text.clone(), font.clone(), color).size().x;
    if wide > rect.width() {
        font.size = (font.size * rect.width() / wide).max(9.0);
    }
    if kit::fit_text(painter, pos2(rect.center().x, rect.bottom()), Align2::CENTER_BOTTOM, text.clone(), font, color, rect.width()).1 { resp.on_hover_text(text) } else { resp }
}

/// What a knob's drag, wheel and keys make of its value: drag up/down (Shift = fine), scroll,
/// Cmd-click to reset; focused, Up / Down nudge it and Delete resets it.
fn knob_input(ui: &Ui, resp: &Response, info: &ParamInfo, value: f32) -> Option<f32> {
    let id = resp.id;
    let norm = info.to_norm(value);
    let mut new = None;
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(id, norm));
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let delta = -resp.drag_delta().y * if fine { 0.0015 } else { 0.006 };
        // accumulate unrounded travel so stepped knobs still move under small drags
        let travel = (ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(norm) + delta).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(id, travel));
        new = Some(info.from_norm(travel));
    } else if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            new = Some(info.from_norm(norm + scroll * 0.0015));
            // the knob took the wheel: the panel around it doesn't scroll too
            ui.input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }
    let steps = (info.curve == Curve::Steps).then_some(info.max - info.min);
    if let Some(travel) = keyboard_travel(ui, resp, norm, steps) {
        new = Some(info.from_norm(travel));
    }
    if (resp.clicked() && ui.input(|i| i.modifiers.command)) || reset_pressed(ui, resp) {
        new = Some(info.default);
    }
    new
}

/// A knob as Teenage Engineering draws one: a ring of ticks lit up to `norm` in `cap`'s color,
/// a chunky flat cap in it on a lip, one line pointing at the value. `hot` (hovered) and
/// `glow` (turning, moving) brighten it.
fn paint_knob(painter: &Painter, t: &Theme, c: Pos2, norm: f32, cap: egui::Color32, hot: f32, glow: f32) {
    const R: f32 = 13.0;
    const TICKS: usize = 21;
    let (a0, sweep) = (0.75 * PI, 1.5 * PI);
    let norm = norm.clamp(0.0, 1.0);
    let bright = Theme::mix(cap, egui::Color32::WHITE, 0.3 * glow);
    if glow > 0.0 {
        painter.circle_filled(c, R + 9.0, cap.gamma_multiply(0.12 * glow));
    }
    // the scale: a tick every twentieth, longer each quarter, lit as far as the value (the last
    // one lighting as the value reaches it)
    let off = Theme::mix(t.line, t.fg_muted, 0.35 * hot);
    for k in 0..TICKS {
        let f = k as f32 / (TICKS - 1) as f32;
        let lit = ((norm - f) * (TICKS - 1) as f32 + 1.0).clamp(0.0, 1.0);
        let d = Vec2::angled(a0 + sweep * f);
        let long = if k % 5 == 0 { 8.0 } else { 6.0 };
        painter.line_segment([c + d * (R + 3.5), c + d * (R + long)], Stroke::new(2.0, Theme::mix(off, bright, lit)));
    }
    // the cap on its lip, and its line in whichever ink reads on it
    painter.circle_filled(c + vec2(0.0, 2.0), R, Theme::mix(cap, egui::Color32::BLACK, 0.5));
    painter.circle_filled(c, R, Theme::mix(cap, egui::Color32::WHITE, 0.12 * hot + 0.2 * glow));
    let d = Vec2::angled(a0 + sweep * norm);
    painter.line_segment([c + d * 3.0, c + d * (R - 3.0)], Stroke::new(3.0, ink(cap)));
}

/// Dark ink on a light color, white on a dark one.
fn ink(on: egui::Color32) -> egui::Color32 {
    let luma = 0.299 * f32::from(on.r()) + 0.587 * f32::from(on.g()) + 0.114 * f32::from(on.b());
    if luma > 110.0 { TE_INK } else { egui::Color32::WHITE }
}

// ---------------------------------------------------------------------------
// The deck (docs/scenes.md §2), Teenage Engineering's way
// ---------------------------------------------------------------------------

pub use crate::theme::{TE_BLUE, TE_GREEN, TE_ORANGE};
use crate::theme::TE_INK;

/// Knob `i`'s cap: blue, green, white, orange, as on the OP-1, then again.
pub fn cap_color(t: &Theme, i: usize) -> egui::Color32 {
    [TE_BLUE, TE_GREEN, t.fg_text, TE_ORANGE][i % 4]
}

/// How tall a deck knob is, its value under it.
pub const DECK_KNOB_H: f32 = 56.0;

/// A deck control's number ("01") in `color`, and the name it was given after it, over it.
pub fn numbered(ui: &mut Ui, t: &Theme, number: usize, name: &str, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.text_xs + 4.0), Sense::hover());
    let font = Theme::mono(t.text_xs);
    let mut job = egui::text::LayoutJob::default();
    job.append(&format!("{number:02}"), 0.0, egui::TextFormat::simple(font.clone(), color));
    if !name.is_empty() {
        job.append(&name.to_uppercase(), 6.0, egui::TextFormat::simple(font, t.fg_text));
    }
    job.wrap = egui::text::TextWrapping { max_width: rect.width(), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(Align2::CENTER_CENTER.anchor_size(rect.center(), galley.size()).min, galley, t.fg_text);
}

/// A macro's knob in the deck: a cap in `cap`'s color on a ring of ticks, its value under it.
/// It glows while it turns, while `lit` (learning) and as it moves on its own (a controller,
/// a morph), easing to a value it jumps to. Input as [`knob`].
pub fn deck_knob(ui: &mut Ui, t: &Theme, info: &ParamInfo, name: &str, value: f32, cap: egui::Color32, lit: bool) -> (Response, Option<f32>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(CELL.x, DECK_KNOB_H), Sense::click_and_drag());
    let new = knob_input(ui, &resp, info, value);
    let shown = new.unwrap_or(value);
    resp.widget_info(|| WidgetInfo::slider(true, shown as f64, format!("{name} {}", info.format(shown))));
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    let norm = eased(ui, t, resp.id.with("eased"), info.to_norm(shown), resp.dragged());
    let held = ui.ctx().animate_bool_with_time(resp.id.with("glow"), resp.dragged() || lit, t.motion);
    let glow = held.max(moved(ui, t, resp.id.with("moved"), shown));
    let painter = ui.painter_at(rect.expand(6.0));
    paint_knob(&painter, t, pos2(rect.center().x, rect.top() + 21.0), norm, cap, kit::hover(ui, t, &resp), glow);
    let resp = knob_value(&painter, t, rect, info.format(shown), Theme::mix(t.fg_text, cap, glow), resp);
    (resp, new)
}

/// An empty macro: where its knob would be, its ring unlit round a dashed socket with a +;
/// `waiting` while it's the one a value goes to next (the socket lit in `cap` and turning,
/// "touch…" under it).
pub fn macro_slot(ui: &mut Ui, t: &Theme, waiting: bool, cap: egui::Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(CELL.x, DECK_KNOB_H), Sense::click());
    let painter = ui.painter_at(rect.expand(6.0));
    let c = pos2(rect.center().x, rect.top() + 21.0);
    let h = kit::hover(ui, t, &resp);
    paint_knob_ring(&painter, t, c, h);
    let color = if waiting { cap } else { Theme::mix(t.fg_muted.gamma_multiply(0.7), t.fg_text, h) };
    // the socket turns while it waits
    let turn = if waiting { ui.input(|i| i.time) as f32 * 1.5 } else { 0.0 };
    let ring: Vec<Pos2> = (0..=40).map(|k| c + Vec2::angled(turn + k as f32 / 40.0 * TAU) * 12.0).collect();
    painter.extend(Shape::dashed_line(&ring, Stroke::new(1.5, color), 2.5, 2.5));
    if waiting {
        ui.ctx().request_repaint();
        kit::fit_text(&painter, pos2(c.x, rect.bottom()), Align2::CENTER_BOTTOM, tr!("touch…").to_uppercase(), Theme::mono(t.text_xs), cap, rect.width());
    }
    kit::icon_at(&painter, c, crate::icons::PLUS, 12.0, color);
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    resp
}

/// A knob's ring of ticks with nothing lit.
fn paint_knob_ring(painter: &Painter, t: &Theme, c: Pos2, hot: f32) {
    let (a0, sweep) = (0.75 * PI, 1.5 * PI);
    let off = Theme::mix(t.line, t.fg_muted, 0.35 * hot);
    for k in 0..21 {
        let d = Vec2::angled(a0 + sweep * k as f32 / 20.0);
        let long = if k % 5 == 0 { 8.0 } else { 6.0 };
        painter.line_segment([c + d * 16.5, c + d * (13.0 + long)], Stroke::new(2.0, off));
    }
}

/// `target` as shown: last frame's, most of the way there (at once while `snap`, or with no
/// motion in the theme).
fn eased(ui: &Ui, t: &Theme, id: egui::Id, target: f32, snap: bool) -> f32 {
    let was = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(target);
    let k = 1.0 - (-ui.input(|i| i.stable_dt).min(0.1) * 18.0).exp();
    let now = if snap || t.motion <= 0.0 || (target - was).abs() < 1e-3 { target } else { was + (target - was) * k };
    ui.data_mut(|d| d.insert_temp(id, now));
    if now != target {
        ui.ctx().request_repaint();
    }
    now
}

/// How lit a value is for having just moved: 1 as it moves, fading over half a second.
fn moved(ui: &Ui, t: &Theme, id: egui::Id, value: f32) -> f32 {
    let now = ui.input(|i| i.time);
    let (was, at) = ui.data(|d| d.get_temp::<(f32, f64)>(id)).unwrap_or((value, f64::NEG_INFINITY));
    let at = if (value - was).abs() > 1e-4 { now } else { at };
    ui.data_mut(|d| d.insert_temp(id, (value, at)));
    let k = (1.0 - (now - at) as f32 / 0.5).clamp(0.0, 1.0);
    if k > 0.0 && t.motion > 0.0 {
        ui.ctx().request_repaint();
    }
    if t.motion > 0.0 { kit::ease(k) } else { 0.0 }
}

/// A flat key on a lip, as Teenage Engineering's pads: its face in `fill`, sunk (0..1) onto
/// the lip as it's pressed. Where its face is.
fn keycap(painter: &Painter, rect: Rect, fill: egui::Color32, sunk: f32) -> Rect {
    const LIP: f32 = 2.0;
    let round = CornerRadius::same(4);
    painter.rect_filled(rect, round, Theme::mix(fill, egui::Color32::BLACK, 0.45));
    let face = Rect::from_min_max(rect.min + vec2(0.0, LIP * sunk), pos2(rect.right(), rect.bottom() - LIP * (1.0 - sunk)));
    painter.rect_filled(face, round, fill);
    face
}

/// A key in the deck: `text` printed on a keycap that sinks as it's pressed, lit orange while
/// `on`. `label` names it for screen readers.
pub fn deck_key(ui: &mut Ui, t: &Theme, size: Vec2, text: &str, label: &str, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let h = kit::hover(ui, t, &resp);
    let k = ui.ctx().animate_bool_with_time(resp.id.with("on"), on, t.motion);
    let sunk = ui.ctx().animate_bool_with_time(resp.id.with("down"), resp.is_pointer_button_down_on(), t.motion * 0.5);
    let fill = Theme::mix(Theme::mix(t.bg_control, t.fg_text, 0.08 * h), TE_ORANGE, k);
    let face = keycap(ui.painter(), rect, fill, sunk);
    kit::icon_at(ui.painter(), face.center(), text, t.text_sm + 1.0, Theme::mix(Theme::mix(t.fg_muted, t.fg_text, 0.6 + 0.4 * h), TE_INK, k));
    kit::focus_ring(ui, t, &resp, 4);
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, on, label));
    resp
}

/// Where a key will be: its outline, `text` printed faintly in it.
pub fn key_socket(ui: &mut Ui, t: &Theme, size: Vec2, text: &str) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, Theme::mix(t.line, t.bg_panel, 0.3)), StrokeKind::Inside);
    kit::icon_at(ui.painter(), rect.center(), text, t.text_sm + 1.0, t.fg_muted.gamma_multiply(0.3));
}

fn arc(painter: &Painter, c: Pos2, r: f32, a0: f32, a1: f32, stroke: Stroke) {
    if (a1 - a0).abs() < 0.01 {
        return;
    }
    let steps = ((a1 - a0).abs() * r / 2.0).ceil() as usize;
    let pts = (0..=steps)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    painter.line(pts, stroke);
}

/// The brand pack's icon (assets/icons): the V of light in smoke. Not redrawn, not tinted.
pub const ICON_256: &[u8] = include_bytes!("../../../assets/icons/png/velo-256.png");

/// One of the icons above as a texture, decoded once and kept in egui's memory.
pub fn brand(ctx: &egui::Context, png: &'static [u8]) -> egui::TextureId {
    let key = egui::Id::unique(("velo-brand", png.as_ptr() as usize));
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return texture.id();
    }
    let icon = eframe::icon_data::from_png_bytes(png).expect("the brand icons are PNGs");
    let image = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    let texture = ctx.load_texture("velo-brand", image, egui::TextureOptions::LINEAR);
    let id = texture.id();
    ctx.data_mut(|d| d.insert_temp(key, texture));
    id
}

// ---------------------------------------------------------------------------
// Modulation (product.md §7.3)
// ---------------------------------------------------------------------------

/// What a source chip carries while it's dragged onto a parameter: its id and its name.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceDrag(pub String, pub String);

/// What a scene card carries while it's dragged to another place in the list: its scene's id.
pub struct SceneDrag(pub String);

/// One mapping on a slider, as drawn: which source, how far it moves the parameter (a
/// fraction of the travel, -1..1) and where that source is right now (0..1).
pub struct ModView<'a> {
    pub source: &'a str,
    pub amount: f32,
    pub level: f32,
}

/// What a slider did this frame.
pub struct Slider {
    /// A new base value.
    pub value: Option<f32>,
    /// Alt-drag changed the first mapping's amount.
    pub amount: Option<f32>,
    /// A source was dropped on it: map it.
    pub dropped: Option<String>,
    /// One response per mapping badge, then the macro's, then the controller's: anchors for
    /// their popovers.
    pub badges: Vec<Response>,
    /// The slider itself (drag stopped, focus lost: where an undo step ends).
    pub response: Response,
}

/// A macro driving a parameter, as its slider shows it: "M3" and the values at the macro's ends.
pub struct Driver<'a> {
    pub label: &'a str,
    pub range: [f32; 2],
}

/// What a slider shows besides its value: its mappings, the value the engine computed (`live`),
/// the macro driving it, the controller bound to it.
#[derive(Default)]
pub struct Marks<'a> {
    pub mods: &'a [ModView<'a>],
    pub live: Option<f32>,
    pub driver: Option<Driver<'a>>,
    pub binding: Option<&'a str>,
    /// An icon before its name, in the accent (Automagic's).
    pub icon: Option<&'a str>,
    /// Its fill in this instead of the accent (a mapping's amount, in the signal's green).
    pub tint: Option<egui::Color32>,
}

/// A parameter as a filled field (Bitwig's, Figma's): its name on the left and its value on the
/// right, inside a bar that fills up to the value (from zero, for a range across it). Over the
/// fill: the range a macro takes it through (a strip along the bottom), the modulation range (a
/// band) and where the modulation has it right now (a bright line: `live`, the value the
/// engine computed, or else base + Σ amount × level). Under it, a badge per mapping, one for
/// the macro and one for a learned controller.
/// Drag anywhere to scrub (Shift = fine), Alt-drag to change the first mapping's amount,
/// double-click to type a value (Enter sets it, Esc doesn't), Cmd-click or Delete to reset;
/// drop a source chip on it to map it.
pub fn slider(ui: &mut Ui, t: &Theme, info: &ParamInfo, value: f32, marks: &Marks) -> Slider {
    let Marks { mods, live, driver, binding, icon, tint } = marks;
    let (live, binding, tint) = (*live, *binding, tint.unwrap_or(t.accent));
    let field_h = t.control_h + 4.0;
    let has_badges = !mods.is_empty() || driver.is_some() || binding.is_some();
    let badge_h = if has_badges { t.row_h - 6.0 + t.gap } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), field_h + badge_h), Sense::click_and_drag());
    // room before whatever comes next
    ui.add_space(t.gap);
    // an angle: a dial beside the slider, turned to it and turning it
    let angle = info.unit == "°";
    let field = Rect::from_min_size(rect.min, vec2(rect.width() - if angle { field_h + t.gap } else { 0.0 }, field_h));
    let x_of = |n: f32| field.left() + n.clamp(0.0, 1.0) * field.width();
    let norm = info.to_norm(value);
    let mut out = Slider { value: None, amount: None, dropped: None, badges: Vec::new(), response: resp.clone() };
    if angle {
        let dial = Rect::from_min_size(pos2(field.right() + t.gap, field.top()), Vec2::splat(field_h));
        let (v, r) = angle_dial(ui, t, dial, resp.id.with("dial"), info, value);
        out.value = v;
        out.response |= r;
    }

    // typing a value: double-click opens the field for it (put in once the slider's painted,
    // over it)
    let edit_id = resp.id.with("typing");
    let mut typing: Option<String> = ui.data(|d| d.get_temp(edit_id));
    if resp.double_clicked() {
        let shown = info.format(value);
        typing = Some(shown.split_whitespace().next().unwrap_or(&shown).to_string());
    }
    let editing = typing.is_some();

    // dragging
    let (alt, fine, command) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift, i.modifiers.command));
    if !editing && resp.dragged() {
        let delta = resp.drag_delta().x / field.width() * if fine { 0.2 } else { 1.0 };
        if alt && !mods.is_empty() {
            out.amount = Some((mods[0].amount + delta).clamp(-1.0, 1.0));
        } else {
            // where the drag has it, unrounded: a whole number's steps come as the pointer
            // reaches them, not lost to rounding a few pixels a frame
            let drag_id = resp.id.with("drag");
            let at = ui.data(|d| d.get_temp::<f32>(drag_id)).filter(|_| !resp.drag_started()).unwrap_or(norm);
            let at = (at + delta).clamp(0.0, 1.0);
            ui.data_mut(|d| d.insert_temp(drag_id, at));
            out.value = Some(info.from_norm(at));
        }
    }
    if let Some(travel) = keyboard_travel(ui, &resp, norm, (info.curve == Curve::Steps).then_some(info.max - info.min)) {
        out.value = Some(info.from_norm(travel));
    }
    if (resp.clicked() && command) || reset_pressed(ui, &resp) {
        out.value = Some(info.default);
    }
    let dropping = resp.dnd_hover_payload::<SourceDrag>();
    if let Some(drop) = resp.dnd_release_payload::<SourceDrag>() {
        out.dropped = Some(drop.0.clone());
    }

    // paint: the field, the fill from its origin (zero, for a range across it) to the value
    let shown = info.to_norm(out.value.unwrap_or(value));
    let name = i18n::t(info.name);
    resp.widget_info(|| WidgetInfo::slider(true, f64::from(out.value.unwrap_or(value)), name));
    let painter = ui.painter_at(rect.expand(4.0));
    let h = kit::hover(ui, t, &resp);
    let round = CornerRadius::same(t.radius_sm);
    painter.rect_filled(field, round, Theme::mix(t.bg_field, t.fg_text, 0.03 + 0.03 * h));
    let origin = if info.min < 0.0 && info.max > 0.0 { info.to_norm(0.0) } else { 0.0 };
    let (a, b) = (x_of(origin.min(shown)), x_of(origin.max(shown)));
    let fill = Rect::from_min_max(pos2(a, field.top()), pos2(b, field.bottom()));
    let clipped = painter.with_clip_rect(field.intersect(painter.clip_rect()));
    {
        // the fill, clipped to the field's rounded corners by drawing it as the field's shape
        let clip = clipped.with_clip_rect(fill);
        clip.rect_filled(field, round, Theme::mix(t.bg_field, tint, 0.28 + 0.06 * h));
    }
    // zero, for a range across it: a short tick
    if origin > 0.0 {
        clipped.vline(x_of(origin), field.y_range().shrink(9.0), Stroke::new(1.0, t.fg_muted.gamma_multiply(0.5)));
    }
    // the value's edge: a bright line, flaring for a moment as the value moves
    let flare = moved(ui, t, resp.id.with("moved"), shown);
    clipped.vline(x_of(shown), field.y_range().shrink(5.0 - 3.0 * flare), Stroke::new(2.0 + flare, Theme::mix(tint, egui::Color32::WHITE, 0.4 * flare)));
    if let Some(d) = driver {
        let (a, b) = (x_of(info.to_norm(d.range[0])), x_of(info.to_norm(d.range[1])));
        clipped.rect_filled(Rect::from_min_max(pos2(a.min(b), field.bottom() - 3.0), pos2(a.max(b), field.bottom())), CornerRadius::ZERO, t.fg_muted.gamma_multiply(0.6));
    }
    // the modulation range with a line at its far end, lit from the value up to where the
    // sources have it now (eased), and a bright line there glowing with them
    let reach: f32 = mods.iter().map(|m| m.amount).sum();
    if !mods.is_empty() {
        let (a, b) = (x_of(shown), x_of(shown + reach));
        let band = |x: f32| Rect::from_min_max(pos2(a.min(x), field.top()), pos2(a.max(x), field.bottom()));
        clipped.rect_filled(band(b), CornerRadius::ZERO, t.cat_signal.gamma_multiply(0.1));
        clipped.vline(b, field.y_range().shrink(3.0), Stroke::new(1.0, t.cat_signal.gamma_multiply(0.55)));
        let target = live.map_or_else(|| shown + mods.iter().map(|m| m.amount * m.level).sum::<f32>(), |v| info.to_norm(v));
        let now = x_of(eased(ui, t, resp.id.with("now"), target, false));
        clipped.rect_filled(band(now), CornerRadius::ZERO, t.cat_signal.gamma_multiply(0.22));
        let glow = mods.iter().map(|m| m.level).fold(0.0, f32::max);
        clipped.rect_filled(Rect::from_center_size(pos2(now, field.center().y), vec2(8.0, field.height())), CornerRadius::ZERO, t.cat_signal.gamma_multiply(0.2 * glow));
        clipped.vline(now, field.y_range(), Stroke::new(2.0, Theme::mix(t.cat_signal, egui::Color32::WHITE, 0.3 * glow)));
    }
    // the name and the value, over all that (while typing, the field takes the value's place)
    let pad = t.pad - 2.0;
    let value_left = if editing {
        field.center().x
    } else {
        let reading = match &dropping {
            Some(source) => tr!("Drop to map {}", source.1),
            None => info.format(out.value.unwrap_or(value)),
        };
        painter.text(pos2(field.right() - pad, field.center().y), Align2::RIGHT_CENTER, reading, t.readout(), if dropping.is_some() { t.accent } else { Theme::mix(t.fg_text, tint, flare) }).left()
    };
    let name_left = match icon {
        Some(icon) => painter.text(pos2(field.left() + pad, field.center().y), Align2::LEFT_CENTER, *icon, t.body(), t.accent).right() + t.gap * 1.5,
        None => field.left() + pad,
    };
    kit::fit_text(&painter, pos2(name_left, field.center().y), Align2::LEFT_CENTER, name, t.body(), Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * h), value_left - name_left - pad);
    if let Some(text) = &mut typing {
        let right = Rect::from_min_max(pos2(value_left, field.top()), field.max).shrink2(vec2(pad, 2.0));
        let edit = ui.put(right, egui::TextEdit::singleline(text).font(t.readout()).align(Align2::RIGHT_CENTER).frame(egui::Frame::NONE).id(edit_id.with("edit")));
        if !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if edit.lost_focus() || enter || escape {
            // a number, in the unit it's shown in (a percent is shown × 100)
            let number: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | ',')).collect();
            if !escape && let Ok(v) = number.replace(',', ".").parse::<f32>() {
                out.value = Some(info.clamp(match info.unit {
                    "%" => v / 100.0,
                    "°" => v / 360.0 * (info.max - info.min),
                    _ => v,
                }));
            }
            typing = None;
        }
    }
    ui.data_mut(|d| match &typing {
        Some(text) => {
            d.insert_temp(edit_id, text.clone());
        }
        None => d.remove::<String>(edit_id),
    });
    if dropping.is_some() {
        painter.rect_stroke(field, round, Stroke::new(1.0, t.accent.gamma_multiply(0.8)), egui::StrokeKind::Outside);
    } else if editing {
        painter.rect_stroke(field, round, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    }
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    if resp.hovered() && !editing {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let track = field;

    // badges: one per mapping, then the controller
    if has_badges {
        let mut x = rect.left();
        let top = track.bottom() + t.gap;
        for (i, m) in mods.iter().enumerate() {
            let amount = format!("{:+.0}%", m.amount * 100.0);
            let r = badge(ui, t, resp.id.with(("mod", i)), pos2(x, top), Some(m.level), m.source, &amount);
            x = r.rect.right() + t.gap;
            out.badges.push(r);
        }
        if let Some(d) = driver {
            let range = format!("{} → {}", info.format(d.range[0]), info.format(d.range[1]));
            let r = badge(ui, t, resp.id.with("macro"), pos2(x, top), None, d.label, &range);
            x = r.rect.right() + t.gap;
            out.badges.push(r);
        }
        if let Some(control) = binding {
            out.badges.push(badge(ui, t, resp.id.with("binding"), pos2(x, top), None, control, ""));
        }
    }
    out
}

/// A clip's loop on its timeline: the clip as a track (ticks along it), the part that loops
/// lit between two handles, its times under it. Drag a handle to move that end, the lit part
/// to move both, double-click for the whole clip. The new (from, to), fractions of the clip,
/// while it changes.
pub fn loop_range(ui: &mut Ui, t: &Theme, length: f64, from: f32, to: f32) -> (Response, Option<(f32, f32)>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0 + t.text_sm + t.gap), Sense::click_and_drag());
    let track = Rect::from_min_size(rect.min + vec2(6.0, 4.0), vec2(rect.width() - 12.0, 22.0));
    let x_of = |f: f32| track.left() + f.clamp(0.0, 1.0) * track.width();
    let f_of = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0);
    let (from, to) = (from.min(to), from.max(to));
    // at least a twentieth of a second between the ends
    let gap = (0.05 / length.max(0.05)) as f32;
    let grab_id = resp.id.with("grab");
    let mut out = None;
    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (near_from, near_to) = ((p.x - x_of(from)).abs(), (p.x - x_of(to)).abs());
        let grab = if near_from <= 8.0 && near_from <= near_to {
            0
        } else if near_to <= 8.0 {
            1
        } else if p.x > x_of(from) && p.x < x_of(to) {
            2
        } else if near_from < near_to {
            0
        } else {
            1
        };
        ui.data_mut(|d| d.insert_temp(grab_id, (grab, from, to, f_of(p.x))));
    }
    if resp.dragged()
        && let (Some(p), Some((grab, f0, t0, start))) = (resp.interact_pointer_pos(), ui.data(|d| d.get_temp::<(u8, f32, f32, f32)>(grab_id)))
    {
        let d = f_of(p.x) - start;
        out = Some(match grab {
            0 => ((f0 + d).clamp(0.0, (t0 - gap).max(0.0)), t0),
            1 => (f0, (t0 + d).clamp((f0 + gap).min(1.0), 1.0)),
            _ => {
                let w = t0 - f0;
                let a = (f0 + d).clamp(0.0, 1.0 - w);
                (a, a + w)
            }
        });
    }
    if resp.double_clicked() {
        out = Some((0.0, 1.0));
    }
    let (a, b) = out.unwrap_or((from, to));
    let painter = ui.painter();
    let h = kit::hover(ui, t, &resp);
    painter.rect_filled(track, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_field, t.fg_text, 0.03 + 0.03 * h));
    // a tick a second, or every five or thirty on a long clip
    let every = if length <= 20.0 {
        1.0
    } else if length <= 120.0 {
        5.0
    } else {
        30.0
    };
    let mut s = every;
    while s < length {
        let x = x_of((s / length) as f32);
        painter.vline(x, (track.bottom() - 6.0)..=track.bottom(), Stroke::new(1.0, t.line));
        s += every;
    }
    let lit = Rect::from_x_y_ranges(x_of(a)..=x_of(b), track.y_range());
    painter.rect_filled(lit, CornerRadius::ZERO, t.accent.gamma_multiply(0.22 + 0.08 * h));
    for x in [x_of(a), x_of(b)] {
        painter.rect_filled(Rect::from_center_size(pos2(x, track.center().y), vec2(4.0, track.height() + 6.0)), CornerRadius::same(2), t.accent);
    }
    let len = length as f32;
    let text = tr!("{} s → {} s · {} s of {} s", format!("{:.2}", a * len), format!("{:.2}", b * len), format!("{:.2}", (b - a) * len), format!("{len:.1}"));
    painter.text(pos2(track.left(), rect.bottom()), Align2::LEFT_BOTTOM, text, Theme::mono(t.text_xs), t.fg_muted);
    if let Some(p) = resp.hover_pos() {
        let near = (p.x - x_of(a)).abs().min((p.x - x_of(b)).abs()) <= 8.0;
        ui.ctx().set_cursor_icon(if near { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::Grab });
    }
    (resp.on_hover_text(tr!("The part of the clip that loops: drag its ends or the part itself · double-click for the whole clip")), out)
}

/// A small pill, its name in mono capitals and a readout: a mapping's in the signal's green, a
/// dot on it glowing with `level` and the pill filling up with it, a ring off the dot on a hit;
/// a macro's or a controller's muted. It sinks as it's pressed and stays lit while its popover
/// is open. Click for its popover.
fn badge(ui: &mut Ui, t: &Theme, id: egui::Id, at: Pos2, level: Option<f32>, name: &str, readout: &str) -> Response {
    let (font, caps) = (Theme::mono(t.text_xs), name.to_uppercase());
    let painter = ui.painter();
    let name_w = painter.layout_no_wrap(caps.clone(), font.clone(), t.fg_text).size().x;
    let readout_w = if readout.is_empty() { 0.0 } else { painter.layout_no_wrap(readout.to_string(), Theme::mono(t.text_sm), t.fg_muted).size().x + t.gap * 1.5 };
    let dot = if level.is_some() { 15.0 } else { 0.0 };
    let rect = Rect::from_min_size(at, vec2(dot + name_w + readout_w + 2.0 * t.pad, t.row_h - 6.0));
    let resp = ui.interact(rect, id, Sense::click());
    let h = kit::hover(ui, t, &resp);
    let open = ui.ctx().animate_bool_with_time(id.with("open"), egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp)), t.motion);
    let tint = if level.is_some() { t.cat_signal } else { t.fg_muted };
    let shape = if resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    let radius = CornerRadius::same((shape.height() / 2.0) as u8);
    let painter = ui.painter();
    painter.rect_filled(shape, radius, Theme::mix(t.bg_card, tint, 0.08 + 0.06 * h + 0.1 * open));
    let mut x = shape.left() + t.pad;
    if let Some(level) = level {
        let v = level.clamp(0.0, 1.0);
        painter.with_clip_rect(Rect::from_min_max(shape.min, pos2(shape.left() + shape.width() * v, shape.bottom()))).rect_filled(shape, radius, tint.gamma_multiply(0.16));
        // a hit: the level jumping up sends a ring out from the dot, fading
        let c = pos2(x + 3.0, shape.center().y);
        let now = ui.input(|i| i.time);
        let (was, mut hit) = ui.data(|d| d.get_temp::<(f32, f64)>(id.with("hit"))).unwrap_or((v, f64::NEG_INFINITY));
        if v - was > 0.25 {
            hit = now;
        }
        ui.data_mut(|d| d.insert_temp(id.with("hit"), (v, hit)));
        let ring = ((now - hit) as f32 / 0.4).clamp(0.0, 1.0);
        if ring < 1.0 && t.motion > 0.0 {
            painter.circle_stroke(c, 3.0 + 7.0 * kit::ease(ring), Stroke::new(1.5, tint.gamma_multiply(0.8 * (1.0 - ring))));
            ui.ctx().request_repaint();
        }
        painter.circle_filled(c, 3.0 + 3.0 * v, tint.gamma_multiply(0.3 * v));
        painter.circle_filled(c, 3.0, Theme::mix(tint.gamma_multiply(0.55), Theme::mix(tint, egui::Color32::WHITE, 0.3), v));
        x += dot;
    }
    painter.rect_stroke(shape, radius, Stroke::new(1.0, tint.gamma_multiply(0.3 + 0.3 * h + 0.4 * open)), egui::StrokeKind::Inside);
    painter.text(pos2(x, shape.center().y), Align2::LEFT_CENTER, caps, font, Theme::mix(t.fg_muted, t.fg_text, 0.6 + 0.4 * h.max(open)));
    if !readout.is_empty() {
        let lit = if level.is_some() { tint } else { t.fg_muted };
        painter.text(pos2(shape.right() - t.pad, shape.center().y), Align2::RIGHT_CENTER, readout, Theme::mono(t.text_sm), Theme::mix(lit, t.fg_text, 0.5 * h));
    }
    kit::focus_ring(ui, t, &resp, radius.nw);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, format!("{name} {readout}")));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A modulation source as a live meter `height` tall (a level, a hit, a phase) over its name.
/// Drag it onto any parameter to map it: the drop carries `source`, its id.
pub fn source_meter(ui: &mut Ui, t: &Theme, source: &str, name: &str, level: f32, kind: Meter, height: f32) -> Response {
    let id = ui.scope_id().with(("source", source));
    ui.dnd_drag_source(id, SourceDrag(source.to_string(), name.to_string()), |ui| meter(ui, t, name, level, kind, height)).response
}

/// An angle's dial: a disc with a hand at the angle (0 points right, turning clockwise as the
/// picture's y runs down) and the arc from 0 to it. Click or drag round it to point it; Shift
/// snaps to 15°. The value it set, and its response.
fn angle_dial(ui: &mut Ui, t: &Theme, rect: Rect, id: egui::Id, info: &ParamInfo, value: f32) -> (Option<f32>, Response) {
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let turn = info.max - info.min;
    let c = rect.center();
    let mut set = None;
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked() || resp.drag_started()) {
        let mut a = (p - c).angle() / TAU;
        if ui.input(|i| i.modifiers.shift) {
            a = (a * 24.0).round() / 24.0;
        }
        // into the range: a whole turn across it
        let mut v = a * turn;
        while v < info.min {
            v += turn;
        }
        while v > info.max {
            v -= turn;
        }
        set = Some(info.clamp(v));
    }
    let shown = set.unwrap_or(value) / turn * TAU;
    let h = kit::hover(ui, t, &resp);
    let painter = ui.painter();
    let r = rect.width() / 2.0 - 1.0;
    painter.circle(c, r, Theme::mix(t.bg_field, t.fg_text, 0.03 + 0.03 * h), Stroke::new(1.0, Theme::mix(t.line, t.fg_muted, 0.3 * h)));
    // a tick each quarter turn, the arc it turned through, then its hand and a dot at the rim
    for k in 0..4 {
        let d = Vec2::angled(k as f32 * TAU / 4.0);
        painter.line_segment([c + d * (r - 2.5), c + d * r], Stroke::new(1.0, t.fg_muted.gamma_multiply(0.6)));
    }
    let steps = ((shown.abs() / TAU * 48.0).ceil() as usize).max(1);
    let arc: Vec<Pos2> = (0..=steps).map(|k| c + Vec2::angled(shown * k as f32 / steps as f32) * (r - 3.0)).collect();
    painter.add(egui::Shape::line(arc, Stroke::new(2.0, Theme::mix(t.bg_field, t.accent, 0.5))));
    let tip = c + Vec2::angled(shown) * (r - 3.0);
    painter.line_segment([c, tip], Stroke::new(2.0, t.accent));
    painter.circle_filled(tip, 2.5, t.accent);
    painter.circle_filled(c, 2.0, t.fg_muted);
    kit::focus_ring(ui, t, &resp, (rect.width() / 2.0) as u8);
    let resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair).on_hover_text(tr!("Drag round to point it · Shift snaps to 15°"));
    (set, resp)
}

const MAP_AMOUNT: ParamInfo = ParamInfo { name: "Amount", min: -1.0, max: 1.0, default: Mapping::AMOUNT, curve: Curve::Linear, unit: "%" };
const MAP_ATTACK: ParamInfo = ParamInfo { name: "Attack", min: 1.0, max: 1000.0, default: Mapping::ATTACK_MS, curve: Curve::Log, unit: "ms" };
const MAP_RELEASE: ParamInfo = ParamInfo { name: "Release", min: 1.0, max: Mapping::MAX_MS, default: Mapping::RELEASE_MS, curve: Curve::Log, unit: "ms" };
const MAP_INVERT: ParamInfo = ParamInfo { name: "Invert", min: 0.0, max: 1.0, default: 0.0, curve: Curve::Steps, unit: "onoff" };

/// What the mapping popover changed this frame.
#[derive(Default)]
pub struct MapEdit {
    pub amount: Option<f32>,
    pub attack_ms: Option<f32>,
    pub release_ms: Option<f32>,
    pub invert: Option<bool>,
    pub remove: bool,
    /// A drag or a focused control ended: where an undo step ends.
    pub done: bool,
}

/// A mapping's settings (product.md §7.3): how far it moves the parameter (−100..100 % of its
/// range), how it follows the source (attack and release), inverted, or gone.
pub fn mapping_popover(ui: &mut Ui, t: &Theme, title: &str, m: &Mapping) -> MapEdit {
    ui.spacing_mut().item_spacing = vec2(t.gap * 2.0, t.gap * 2.0);
    kit::heading(ui, t, crate::icons::PATH, &title.to_uppercase());
    let mut edit = mapping_fields(ui, t, m);
    edit.remove = kit::actions(ui, t, &[(kit::Variant::Outline, Some(crate::icons::TRASH), tr!("Remove mapping"))])[0].clicked();
    edit
}

/// A mapping chip's popover, opened and closed by a click on the chip: its settings, and what
/// they changed while it's open. Every chip of a mapping opens it the same way.
pub fn mapping_popup(t: &Theme, chip: &Response, title: &str, m: &Mapping) -> Option<MapEdit> {
    let popup = egui::Popup::from_toggle_button_response(chip).frame(kit::popover_frame(&chip.ctx, t)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(260.0);
    popup
        .show(|ui| {
            let edit = mapping_popover(ui, t, title, m);
            if edit.remove {
                ui.close();
            }
            edit
        })
        .map(|r| r.inner)
}

/// A mapping's settings without its title and its removal: how far (in the signal's green), how
/// it follows (its envelope drawn over the attack and release knobs), inverted.
pub fn mapping_fields(ui: &mut Ui, t: &Theme, m: &Mapping) -> MapEdit {
    let mut edit = MapEdit::default();
    let out = slider(ui, t, &MAP_AMOUNT, m.amount, &Marks { tint: Some(t.cat_signal), ..Default::default() });
    edit.amount = out.value;
    edit.done |= out.response.drag_stopped() || out.response.lost_focus();
    let (graph, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    // a knob pointed at or turned lights its part of the envelope
    let mut hot = [0.0; 2];
    let mut invert = m.invert;
    ui.horizontal(|ui| {
        // spread under the envelope: attack, release, invert
        ui.spacing_mut().item_spacing.x = ((ui.available_width() - 3.0 * CELL.x) / 2.0).max(t.gap);
        for (k, (info, value, field)) in [(&MAP_ATTACK, m.attack_ms, &mut edit.attack_ms), (&MAP_RELEASE, m.release_ms, &mut edit.release_ms)].into_iter().enumerate() {
            let (r, new) = knob(ui, t, info, value);
            *field = new;
            hot[k] = if r.dragged() { 1.0 } else { kit::hover(ui, t, &r) };
            edit.done |= r.drag_stopped() || r.lost_focus();
        }
        if let (_, Some(v)) = switch(ui, t, &MAP_INVERT, f32::from(u8::from(m.invert))) {
            edit.invert = Some(v >= 0.5);
            invert = v >= 0.5;
        }
    });
    let id = out.response.id.with("envelope");
    let attack = eased(ui, t, id.with("attack"), MAP_ATTACK.to_norm(edit.attack_ms.unwrap_or(m.attack_ms)), false);
    let release = eased(ui, t, id.with("release"), MAP_RELEASE.to_norm(edit.release_ms.unwrap_or(m.release_ms)), false);
    let flip = ui.ctx().animate_bool_with_time(id.with("invert"), invert, t.motion * 2.0);
    envelope(ui.painter(), t, graph, attack, release, flip, hot);
    edit
}

/// How a mapping follows its source, on a little screen: a hit rising over the attack, held a
/// moment, falling away over the release (times as fractions of their knobs' travel), upside
/// down as `flip` goes to 1. `hot` lights the attack's part and the release's.
fn envelope(painter: &Painter, t: &Theme, rect: Rect, attack: f32, release: f32, flip: f32, hot: [f32; 2]) {
    painter.rect_filled(rect, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_field, t.fg_text, 0.03));
    let (left, w) = (rect.left() + 6.0, rect.width() - 12.0);
    let (top, bottom) = (rect.top() + 7.0, rect.bottom() - 6.0);
    let y = |v: f32| bottom + (top - bottom) * (v + (1.0 - 2.0 * v) * flip);
    painter.hline(left..=left + w, y(0.0), Stroke::new(1.0, t.line));
    let (peak, held) = (left + w * (0.04 + 0.28 * attack), w * 0.1);
    let end = peak + held + w * (0.06 + 0.5 * release);
    // the curve: rising quickly then easing in, falling away exponentially, then still
    const N: usize = 20;
    let rise: Vec<Pos2> = (0..=N).map(|k| k as f32 / N as f32).map(|s| pos2(left + (peak - left) * s, y(1.0 - (1.0 - s).powi(3)))).collect();
    let fall: Vec<Pos2> = (0..=N).map(|k| k as f32 / N as f32).map(|s| pos2(peak + held + (end - peak - held) * s, y(((-4.0 * s).exp() - (-4.0f32).exp()) / (1.0 - (-4.0f32).exp())))).collect();
    let mut all = rise.clone();
    all.extend(fall.iter().copied());
    all.push(pos2(left + w, y(0.0)));
    // filled down to its baseline
    let mut mesh = Mesh::default();
    let (fill, base) = (t.cat_signal.gamma_multiply(0.14), y(0.0));
    for pair in all.windows(2) {
        let i = mesh.vertices.len() as u32;
        for p in [pair[0], pair[1], pos2(pair[1].x, base), pos2(pair[0].x, base)] {
            mesh.colored_vertex(p, fill);
        }
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(Shape::mesh(mesh));
    let line = |h: f32| Stroke::new(1.5 + h, Theme::mix(t.cat_signal, egui::Color32::WHITE, 0.45 * h));
    painter.line(all, line(0.0));
    painter.line(rise, line(hot[0]));
    painter.line(fall, line(hot[1]));
    // where it peaks and where it's gone: dots
    painter.circle_filled(pos2(peak, y(1.0)), 2.0 + hot[0], Theme::mix(t.cat_signal, egui::Color32::WHITE, 0.45 * hot[0]));
    painter.circle_filled(pos2(end, y(0.0)), 2.0 + hot[1], Theme::mix(t.cat_signal, egui::Color32::WHITE, 0.45 * hot[1]));
}

/// A bin that reddens as it's pointed at: takes something away. `label` names it for screen
/// readers and the tooltip.
pub fn remove_button(ui: &mut Ui, t: &Theme, label: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(t.control_h), Sense::click());
    let h = kit::hover(ui, t, &resp);
    let shape = if resp.is_pointer_button_down_on() { rect.shrink(1.5) } else { rect };
    if h > 0.0 {
        ui.painter().rect_filled(shape, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_control, t.error, 0.2).gamma_multiply(h));
    }
    kit::icon_at(ui.painter(), shape.center(), crate::icons::TRASH, 15.0, Theme::mix(t.fg_muted, t.error, h));
    kit::focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    resp.on_hover_text(label)
}

/// How a source's meter shows it.
#[derive(Clone, Copy, PartialEq)]
pub enum Meter {
    /// A level: a column of segments, its peak held a moment and falling slowly.
    Level,
    /// A hit: a pad flashing in its color.
    Hit(egui::Color32),
    /// The beat: a lamp.
    Beat,
    /// A phase through a bar: its four beats as steps.
    Phase,
}

/// A source's column in the deck: its meter over its name, Teenage Engineering's way, lit as it
/// moves; grabbed and dropped on a slider, it maps it.
fn meter(ui: &mut Ui, t: &Theme, name: &str, level: f32, meter: Meter, height: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(METER_W, height), Sense::hover());
    let h = kit::hover(ui, t, &resp);
    let painter = ui.painter();
    if h > 0.0 {
        painter.rect_filled(rect, CornerRadius::same(t.radius_sm), t.bg_control.gamma_multiply(0.5 * h));
    }
    let label_h = t.text_xs + 8.0;
    let well = Rect::from_min_max(rect.min + vec2(0.0, t.gap), pos2(rect.right(), rect.bottom() - label_h));
    let (c, v) = (well.center(), level.clamp(0.0, 1.0));
    let (now, dt) = ui.input(|i| (i.time, i.stable_dt.min(0.1)));
    match meter {
        Meter::Level => {
            // the peak: held half a second, then falling half the meter a second
            let key = resp.id.with("peak");
            let (mut peak, mut held) = ui.data(|d| d.get_temp::<(f32, f64)>(key)).unwrap_or((v, now));
            if v >= peak {
                (peak, held) = (v, now);
            } else if now - held > 0.5 {
                peak = (peak - dt * 0.5).max(v);
            }
            ui.data_mut(|d| d.insert_temp(key, (peak, held)));
            segments(painter, t, Rect::from_center_size(c, vec2(16.0, well.height())), v, peak, h);
        }
        Meter::Hit(color) => {
            // a hit flashes the pad, sinks it and sends a ring out from it, fading
            let key = resp.id.with("hit");
            let (was, mut flash, mut at) = ui.data(|d| d.get_temp::<(f32, f32, f64)>(key)).unwrap_or((v, 0.0, f64::NEG_INFINITY));
            if v - was > 0.25 {
                at = now;
            }
            flash = v.max(flash * (-dt * 6.0).exp());
            ui.data_mut(|d| d.insert_temp(key, (v, flash, at)));
            let pad = Rect::from_center_size(c, Vec2::splat(30.0));
            let ring = ((now - at) as f32 / 0.4).clamp(0.0, 1.0);
            if ring < 1.0 && t.motion > 0.0 {
                let r = pad.expand(2.0 + 8.0 * kit::ease(ring));
                painter.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.5, color.gamma_multiply(0.8 * (1.0 - ring))), StrokeKind::Outside);
                ui.ctx().request_repaint();
            }
            painter.rect_filled(pad.expand(3.0), CornerRadius::same(7), color.gamma_multiply(0.2 * flash));
            // grey at rest with a lamp in its color on its corner; lit all over by a hit, which
            // fades quickly through the shades between
            let lit = kit::ease((flash * 1.4 - 0.2).clamp(0.0, 1.0));
            let face = keycap(painter, pad, Theme::mix(Theme::mix(t.bg_control, t.fg_text, 0.06 * h), color, lit), (flash * 1.6).min(1.0));
            painter.circle_filled(face.left_top() + vec2(6.0, 6.0), 2.0, Theme::mix(Theme::mix(t.bg_control, color, 0.7), Theme::mix(color, egui::Color32::WHITE, 0.5), lit));
        }
        Meter::Beat => {
            painter.circle_filled(c, 8.0 + 6.0 * v, t.fg_text.gamma_multiply(0.15 * v));
            painter.circle_filled(c, 8.0, Theme::mix(t.bg_control, t.fg_text, v));
        }
        Meter::Phase => {
            // the bar's four beats, two by two: the one it's on lit (the first orange), dimming
            // through it, the ones passed a little lit
            let at = v * 4.0;
            let on = (at as usize).min(3);
            let (side, gap) = (13.0, 4.0);
            let block = Rect::from_center_size(c, Vec2::splat(2.0 * side + gap));
            for k in 0..4 {
                let step = Rect::from_min_size(block.min + vec2((k % 2) as f32, (k / 2) as f32) * (side + gap), Vec2::splat(side));
                let fill = match k.cmp(&on) {
                    std::cmp::Ordering::Less => Theme::mix(t.bg_control, t.fg_text, 0.2),
                    std::cmp::Ordering::Equal => Theme::mix(t.bg_control, if k == 0 { TE_ORANGE } else { t.fg_text }, 1.0 - 0.3 * at.fract()),
                    std::cmp::Ordering::Greater => t.bg_control,
                };
                painter.rect_filled(step, CornerRadius::same(3), fill);
            }
        }
    }
    // its name in mono capitals, a little smaller to fit, then ending in "…"
    let (text, mut font) = (name.to_uppercase(), Theme::mono(t.text_xs - 1.0));
    let wide = painter.layout_no_wrap(text.clone(), font.clone(), t.fg_muted).size().x;
    if wide > rect.width() - 2.0 {
        font.size = (font.size * (rect.width() - 2.0) / wide).max(8.0);
    }
    kit::fit_text(painter, pos2(rect.center().x, rect.bottom() - label_h / 2.0), Align2::CENTER_CENTER, text, font, Theme::mix(t.fg_muted, t.fg_text, 0.3 + 0.7 * h), rect.width() - 2.0);
    resp.on_hover_cursor(egui::CursorIcon::Grab)
}

/// A level as Teenage Engineering's meters show one: segments lit from the bottom (the top one
/// as far as the level reaches into it), the loudest two orange, and the peak's segment lit.
fn segments(painter: &Painter, t: &Theme, bar: Rect, v: f32, peak: f32, hot: f32) {
    const N: usize = 10;
    let gap = 2.0;
    let h = (bar.height() - gap * (N - 1) as f32) / N as f32;
    let peak_at = (peak * N as f32 - 0.01).max(0.0) as usize;
    let off = Theme::mix(t.line, t.fg_muted, 0.25 * hot);
    for s in 0..N {
        let seg = Rect::from_min_size(pos2(bar.left(), bar.bottom() - (s + 1) as f32 * h - s as f32 * gap), vec2(bar.width(), h));
        let color = if s >= N - 2 { TE_ORANGE } else { t.fg_text };
        let lit = if s == peak_at && peak > 0.02 { 1.0 } else { (v * N as f32 - s as f32).clamp(0.0, 1.0) };
        painter.rect_filled(seg, CornerRadius::same(1), Theme::mix(off, color, lit));
    }
}

/// How wide a source's column is.
pub const METER_W: f32 = 44.0;

// ---------------------------------------------------------------------------
// MIDI (docs/control.md §3)
// ---------------------------------------------------------------------------

/// The controller bound to a control, as a small pill on its top-right corner ("CC 21",
/// "Note C1"), kept inside what the control's region shows.
pub fn control_badge(ui: &Ui, t: &Theme, corner: Pos2, text: &str) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text.to_string(), Theme::mono(t.text_xs), t.accent);
    let size = galley.size() + vec2(6.0, 2.0);
    let rect = Rect::from_min_size(corner - vec2(size.x, size.y / 2.0), size);
    let clip = ui.clip_rect();
    let rect = rect.translate(vec2((clip.left() - rect.left()).max(0.0), (clip.top() - rect.top()).max(0.0)));
    painter.rect_filled(rect, CornerRadius::same(t.radius_sm), t.bg_card);
    painter.rect_stroke(rect, CornerRadius::same(t.radius_sm), Stroke::new(1.0, t.accent.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(3.0, 1.0), galley, t.accent);
}

/// Learn mode's outline around a control MIDI can drive; a strong, breathing one around the
/// control waiting for its controller.
pub fn learn_ring(ui: &Ui, t: &Theme, rect: Rect, waiting: bool) {
    let painter = ui.painter();
    let alpha = match (waiting, t.motion > 0.0) {
        (true, true) => {
            ui.ctx().request_repaint();
            0.7 + 0.3 * (ui.input(|i| i.time) as f32 * 6.0).sin()
        }
        (true, false) => 1.0,
        (false, _) => 0.35,
    };
    painter.rect_stroke(rect.expand(2.0), CornerRadius::same(t.radius_sm), Stroke::new(if waiting { 2.0 } else { 1.0 }, t.accent.gamma_multiply(alpha)), egui::StrokeKind::Outside);
}

// ---------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------

/// How a scene card shows its scene.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct CardState {
    /// Selected in the editor.
    pub selected: bool,
    /// On the output now; `pulse` (0..1) beats with the music.
    pub live: Option<f32>,
    /// Launched, waiting for the next beat or bar: how far along the wait is (0..1).
    pub armed: Option<f32>,
    /// Its picture as it last went out (none yet: a wash in its hue).
    pub shot: Option<egui::TextureId>,
    /// A layer: over (else under) every scene, and whether it's switched on.
    pub layer: Option<(bool, bool)>,
}

/// A stable hue (0..1) for an id: what thumbnails and node viewers are tinted with until the
/// engine renders them. Golden-ratio steps spread similar ids far apart on the color wheel.
pub fn hue_of(id: &str) -> f32 {
    let hash = id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    (hash % 1000) as f32 * 0.618_034 % 1.0
}

/// The two-tone wash a placeholder thumbnail is painted with, for a hue (0..1). Hues are
/// kept to the cool-to-magenta half of the wheel, a stage's colors: yellows and olives
/// read as mud on dark grey.
pub fn wash(hue: f32) -> (egui::Color32, egui::Color32) {
    let h = 0.45 + 0.55 * hue.rem_euclid(1.0);
    (egui::ecolor::Hsva::new(h, 0.6, 0.55, 1.0).into(), egui::ecolor::Hsva::new((h + 0.07) % 1.0, 0.7, 0.18, 1.0).into())
}

/// A launch's countdown: a ring filling up to its boundary.
fn ring(painter: &Painter, t: &Theme, c: Pos2, progress: f32) {
    arc(painter, c, 7.0, 0.0, TAU, Stroke::new(2.0, t.bg_control));
    arc(painter, c, 7.0, -PI / 2.0, -PI / 2.0 + TAU * progress.clamp(0.0, 1.0), Stroke::new(2.0, t.accent));
}

/// The countdown ring on its own, in a control's height (the preview card's, by Launch).
pub fn countdown(ui: &mut Ui, t: &Theme, progress: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(18.0, t.control_h), Sense::hover());
    ring(ui.painter(), t, rect.center(), progress);
    resp
}

/// A scene card: its picture as it last went out (a wash in `hue` until it has one) filling the
/// card, its name over a shade at the bottom, its launch key in a chip, and LIVE or the countdown
/// to its launch in the other corner.
pub fn scene_card(ui: &mut Ui, t: &Theme, index: usize, name: &str, icon: &str, hue: f32, state: CardState) -> Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, (width / 2.2).clamp(72.0, 112.0)), Sense::click());
    let k = ui.ctx().animate_bool_with_time(resp.id.with("selected"), state.selected, t.motion);
    let h = kit::hover(ui, t, &resp);
    let radius = CornerRadius::same(t.radius_md);
    // live: a glow beating with the music
    if let Some(pulse) = state.live {
        ui.painter().rect_stroke(rect.expand(1.0), CornerRadius::same(t.radius_md.saturating_add(1)), Stroke::new(2.0 + 6.0 * pulse, t.accent.gamma_multiply(0.15 + 0.25 * pulse)), egui::StrokeKind::Outside);
    }
    // the picture, cropped to the card from the middle (shots are the canvas' 16:9), a little
    // nearer while hovered
    let painter = ui.painter_at(rect.expand(8.0));
    if let Some(shot) = state.shot {
        let f = ((16.0 / 9.0) / (rect.width() / rect.height())).min(1.0);
        let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(1.0, f) * (1.0 - 0.05 * h));
        egui::Image::new((shot, rect.size())).uv(uv).corner_radius(radius).paint_at(ui, rect);
    } else {
        let (a, b) = wash(hue);
        paint_gradient(&painter, rect, radius, |p| Theme::mix(a, b, (p.x - rect.left()) / rect.width()));
    }
    // a shade under the name, darker to the bottom
    let shade = t.bg_base;
    paint_gradient(&painter, rect, radius, |p| shade.gamma_multiply(((p.y - rect.center().y) / (rect.height() / 2.0)).clamp(0.0, 1.0) * 0.9));
    if h > 0.0 {
        painter.rect_filled(rect, radius, egui::Color32::WHITE.gamma_multiply(0.04 * h));
    }
    // its edge: lit when live, brighter when selected
    let edge = if state.live.is_some() { t.accent } else { Theme::mix(t.line, t.fg_text, 0.25 + 0.55 * k) };
    painter.rect_stroke(rect, radius, Stroke::new(if state.live.is_some() || k > 0.5 { 1.5 } else { 1.0 }, edge), egui::StrokeKind::Inside);
    kit::spotlight(ui, t, rect, t.radius_md);
    let inner = rect.shrink(t.gap + 2.0);
    // the launch key, in a chip
    let chip = |at: Pos2, text: String, color: egui::Color32| {
        let galley = painter.layout_no_wrap(text, Theme::mono(t.text_xs), color);
        let r = Rect::from_min_size(at, galley.size() + vec2(10.0, 4.0));
        painter.rect_filled(r, CornerRadius::same(t.radius_sm), t.bg_base.gamma_multiply(0.75));
        painter.galley(r.min + vec2(5.0, 2.0), galley, color);
        r
    };
    chip(inner.min, (index + 1).to_string(), Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * k));
    // LIVE with a dot beating with the music, or the launch's countdown
    if let Some(pulse) = state.live {
        let text = tr!("LIVE").to_string();
        let w = painter.layout_no_wrap(text.clone(), Theme::mono(t.text_xs), t.accent).size().x + 10.0 + 10.0;
        let r = chip(pos2(inner.right() - w, inner.top()), format!("  {text}"), t.accent);
        let dot = pos2(r.left() + 7.0, r.center().y);
        painter.circle_filled(dot, 2.5 + 3.0 * pulse, t.accent.gamma_multiply(0.3 * pulse));
        painter.circle_filled(dot, 2.5, t.accent);
    } else if let Some(progress) = state.armed {
        let c = pos2(inner.right() - 9.0, inner.top() + 9.0);
        painter.circle_filled(c, 11.0, t.bg_base.gamma_multiply(0.75));
        ring(&painter, t, c, progress);
    } else if let Some((over, on)) = state.layer {
        // a layer: where it shows, lit while it's on
        let text = if over { tr!("ON TOP") } else { tr!("UNDER") }.to_string();
        let color = if on { t.accent } else { t.fg_muted };
        let w = painter.layout_no_wrap(text.clone(), Theme::mono(t.text_xs), color).size().x + 10.0;
        chip(pos2(inner.right() - w, inner.top()), text, color);
    }
    // its emoji and its name, over the shade
    let mut x = inner.left();
    let y = inner.bottom() - t.body().size * 0.6;
    if !icon.is_empty() {
        painter.text(pos2(x, y), Align2::LEFT_CENTER, icon, egui::FontId::proportional(15.0), egui::Color32::WHITE);
        x += 22.0;
    }
    kit::fit_text(&painter, pos2(x, y), Align2::LEFT_CENTER, name, egui::FontId::proportional(t.body().size + 1.0), egui::Color32::WHITE, inner.right() - x);
    kit::focus_ring(ui, t, &resp, t.radius_md);
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, state.selected, name));
    resp
}

/// The scenes rail while it has none: three ghost scene cards stacked like a hand of cards,
/// fanning out while `spread` (the pointer is over the rail).
pub fn scene_deck(ui: &mut Ui, t: &Theme, spread: bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 104.0), Sense::hover());
    let k = ui.ctx().animate_bool_with_time(resp.id.with("spread"), spread, t.motion * 2.0);
    // it overshoots a little and settles, both ways
    let s = if spread { kit::spring(k) } else { 1.0 - kit::spring(1.0 - k) };
    let scale = (rect.width() / 190.0).min(1.0);
    let c = rect.center() + vec2(0.0, 16.0 * scale);
    // the cards turn about a point under the stack
    let pivot = c + vec2(0.0, 110.0 * scale);
    // back to front: launch key, name, emoji, hue, how far up the stack, turn at rest and fanned, dim
    for (key, name, icon, hue, rise, rest, fan, dim) in [(3, "Drop", "🔥", 0.85, 14.0, 0.08, 0.3, 0.5), (2, "Build", "🌊", 0.45, 7.0, -0.06, -0.3, 0.3), (1, "Intro", "🌙", 0.1, 0.0, 0.0, 0.0, 0.0)] {
        let angle = rest + (fan - rest) * s;
        let at = pivot + egui::emath::Rot2::from_angle(angle) * (c - vec2(0.0, rise * scale) - pivot);
        ghost_card(ui.painter(), t, Rect::from_center_size(at, vec2(104.0, 48.0) * scale), angle, key, name, icon, hue, dim * (1.0 - 0.7 * s));
    }
}

/// One card of [`scene_deck`]: a scene card's look turned by `angle` about its middle, dimmed
/// toward the panel by `dim`.
#[allow(clippy::too_many_arguments)]
fn ghost_card(painter: &Painter, t: &Theme, rect: Rect, angle: f32, key: usize, name: &str, icon: &str, hue: f32, dim: f32) {
    let (c, rot) = (rect.center(), egui::emath::Rot2::from_angle(angle));
    let turn = |p: Pos2| c + rot * (p - c);
    let radius = CornerRadius::same(t.radius_md);
    let (a, b) = wash(hue);
    let mut card = gradient_mesh(rect, radius, |p| {
        let shade = ((p.y - c.y) / (rect.height() / 2.0)).clamp(0.0, 1.0) * 0.9;
        Theme::mix(Theme::mix(Theme::mix(a, b, (p.x - rect.left()) / rect.width()), t.bg_base, shade), t.bg_panel, dim)
    });
    card.rotate(rot, c);
    // a shadow under it, so the cards read as a stack
    let mut shadow = card.clone();
    shadow.translate(vec2(0.0, 3.0));
    shadow.vertices.iter_mut().for_each(|v| v.color = t.bg_base.gamma_multiply(0.7));
    painter.add(Shape::mesh(shadow));
    painter.add(Shape::mesh(card));
    let mut edge = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(&mut edge, egui::epaint::RoundedRect::new(rect, radius));
    painter.add(Shape::closed_line(edge.into_iter().map(turn).collect(), Stroke::new(1.0, Theme::mix(Theme::mix(t.line, t.fg_text, 0.25), t.bg_panel, dim))));
    // the launch key at the top, the emoji and the name over the shade
    let ink = egui::Color32::WHITE.gamma_multiply(1.0 - 0.6 * dim);
    let text = |at: Pos2, text: &str, font: egui::FontId, color: egui::Color32| {
        let galley = painter.layout_no_wrap(text.to_string(), font, color);
        let width = galley.size().x;
        painter.add(egui::epaint::TextShape::new(turn(at - vec2(0.0, galley.size().y / 2.0)), galley, color).with_angle(angle));
        width
    };
    let inner = rect.shrink(t.gap + 2.0);
    text(inner.left_top() + vec2(0.0, 5.0), &key.to_string(), Theme::mono(t.text_xs), ink.gamma_multiply(0.6));
    let y = inner.bottom() - t.body().size * 0.6;
    let x = inner.left() + text(pos2(inner.left(), y), icon, egui::FontId::proportional(13.0), ink) + 6.0;
    text(pos2(x, y), name, t.body(), ink);
}

/// Fills a rounded `rect` with a color that varies over it (`at` each point), as a fan from its
/// middle: exact for colors that change linearly across each half.
fn paint_gradient(painter: &Painter, rect: Rect, radius: CornerRadius, at: impl Fn(Pos2) -> egui::Color32) {
    painter.add(Shape::mesh(gradient_mesh(rect, radius, at)));
}

/// [`paint_gradient`]'s mesh, to move or turn before painting.
fn gradient_mesh(rect: Rect, radius: CornerRadius, at: impl Fn(Pos2) -> egui::Color32) -> Mesh {
    let mut outline = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(&mut outline, egui::epaint::RoundedRect::new(rect, radius));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), at(rect.center()));
    for &p in &outline {
        mesh.colored_vertex(p, at(p));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    mesh
}

/// Emoji to mark a node or a scene with (ones the bundled emoji font draws).
const EMOJI: [&str; 40] =
    ["✨", "🔥", "🌊", "⚡", "🌀", "💫", "⭐", "🌙", "☀", "❄", "🌈", "🌸", "🍄", "🌋", "🌐", "💎", "🔮", "🌌", "🎯", "🚀", "🎨", "🎬", "📺", "💡", "🎹", "📻", "🎵", "🎶", "🎸", "🔊", "🎧", "🎤", "🔺", "🔷", "⬛", "⚪", "💀", "👾", "👽", "❤"];

/// A node's or a scene's mark: its emoji, or a dot in `color`; a click opens the emoji to pick
/// from. Returns the one picked ("" for none).
pub fn icon_picker(ui: &mut Ui, t: &Theme, icon: &str, color: egui::Color32) -> Option<String> {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.control_h, t.control_h), Sense::click());
    let h = crate::kit::hover(ui, t, &resp);
    ui.painter().rect_filled(rect, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_field, t.fg_text, 0.06 * h));
    if icon.is_empty() {
        ui.painter().circle_filled(rect.center(), 4.0, color);
    } else {
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, egui::FontId::proportional(15.0), t.fg_text);
    }
    let resp = resp.on_hover_text(tr!("Pick an icon"));
    egui::Popup::menu(&resp).show(|ui| emoji_grid(ui, icon)).and_then(|r| r.inner)
}

/// The emoji to pick from, eight a row, and "No icon": the one picked ("" for none).
pub fn emoji_grid(ui: &mut Ui, icon: &str) -> Option<String> {
    let mut picked = None;
    egui::Grid::new("emoji").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
        for (i, e) in EMOJI.iter().enumerate() {
            if ui.add(egui::Button::new(RichText::new(*e).size(16.0)).selected(*e == icon).min_size(vec2(28.0, 28.0))).clicked() {
                picked = Some(e.to_string());
            }
            if i % 8 == 7 {
                ui.end_row();
            }
        }
    });
    ui.separator();
    if ui.add_enabled(!icon.is_empty(), egui::Button::new(tr!("No icon"))).clicked() {
        picked = Some(String::new());
    }
    picked
}

/// The badge on what the drum model decides (an Audio In's hits in Model mode, the tempo it hears).
pub const MODEL_BADGE: &str = "AI";

pub fn model_tip() -> &'static str {
    tr!("From the drum model: the Audio In is in Model mode")
}

/// How much room a [`model_badge`] takes.
pub fn model_badge_size(ui: &Ui, t: &Theme) -> Vec2 {
    ui.painter().layout_no_wrap(MODEL_BADGE.into(), Theme::mono(t.text_xs), t.fg_muted).size() + vec2(8.0, 2.0)
}

/// A small outlined [`MODEL_BADGE`]; its hover says what it means.
pub fn model_badge(ui: &mut Ui, t: &Theme) -> Response {
    let galley = ui.painter().layout_no_wrap(MODEL_BADGE.into(), Theme::mono(t.text_xs), t.fg_muted);
    let (rect, response) = ui.allocate_exact_size(model_badge_size(ui, t), Sense::hover());
    ui.painter().rect_stroke(rect, CornerRadius::same(t.radius_sm), Stroke::new(1.0, t.line), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.fg_muted);
    response.on_hover_text(model_tip())
}

/// One of Velo's loads, as the status bar shows it.
pub struct Load {
    pub name: &'static str,
    /// The value now, as `format` writes it; `room` is as wide as it gets, so the chips beside
    /// it don't move as it counts.
    pub value: f32,
    pub format: fn(f32) -> String,
    pub room: &'static str,
    /// How near its limit it runs (0..1): amber past `warn`, red past `high`.
    pub strain: f32,
    pub warn: f32,
    pub high: f32,
    /// Its last half minute (0..1, oldest first) for the sparkline.
    pub trail: Vec<f32>,
}

/// A load as a chip, its right edge at `right`: a dot in its level's color (breathing once it's
/// red), its name, the last half minute as a sparkline and the value now, which counts to each
/// new reading instead of jumping. The chip warms to amber or red with it.
pub fn load_chip(ui: &mut Ui, t: &Theme, right: f32, load: &Load) -> Response {
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(("load", load.name));
    let value = ctx.animate_value_with_time(id.with("value"), load.value, 0.6);
    let last = ctx.animate_value_with_time(id.with("last"), load.trail.last().copied().unwrap_or(0.0), 0.6);
    let level = if load.strain >= load.high {
        2.0
    } else if load.strain >= load.warn {
        1.0
    } else {
        0.0
    };
    let k = ctx.animate_value_with_time(id.with("level"), level, 0.3);
    let color = if k <= 1.0 { Theme::mix(Theme::TONE_DONE, t.warn, k) } else { Theme::mix(t.warn, t.error, k - 1.0) };
    let painter = ui.painter();
    let name = painter.layout_no_wrap(load.name.into(), Theme::mono(t.text_xs), t.fg_muted);
    let font = Theme::mono(t.text_sm);
    let text = painter.layout_no_wrap((load.format)(value), font.clone(), Theme::mix(t.fg_text, color, k.min(1.0)));
    let room = painter.layout_no_wrap(load.room.into(), font, t.fg_text).size().x.max(text.size().x);
    let (pad, gap, spark) = (7.0, 6.0, vec2(34.0, 12.0));
    let width = pad + 6.0 + gap + name.size().x + gap + spark.x + gap + room + pad;
    let y = ui.max_rect().center().y;
    let rect = Rect::from_min_max(pos2(right - width, y - 9.0), pos2(right, y + 9.0));
    let resp = ui.interact(rect, id, Sense::hover());
    let h = kit::hover(ui, t, &resp);
    let painter = ui.painter();
    let fill = Theme::mix(Theme::mix(t.bg_field, t.fg_text, 0.06 * h), color, 0.08 * k);
    painter.rect(rect, CornerRadius::same(t.radius_sm), fill, Stroke::new(1.0, Theme::mix(t.line, color, 0.45 * k.min(1.0))), StrokeKind::Inside);
    // the dot, a halo breathing round it while it's red
    let dot = pos2(rect.left() + pad + 3.0, y);
    if level >= 2.0 && t.motion > 0.0 {
        let breath = 0.5 + 0.5 * (ui.input(|i| i.time) * 5.0).sin() as f32;
        painter.circle_filled(dot, 3.0 + 3.5 * breath, color.gamma_multiply(0.35 * (1.0 - breath)));
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
    painter.circle_filled(dot, 3.0, color);
    let x = dot.x + 3.0 + gap + name.size().x + gap;
    painter.galley(pos2(dot.x + 3.0 + gap, y - name.size().y / 2.0), name, t.fg_muted);
    sparkline(painter, t, Rect::from_min_size(pos2(x, y - spark.y / 2.0), spark), &load.trail, last, Theme::mix(t.fg_muted, color, 0.35 + 0.65 * k.min(1.0)));
    // the value, right-aligned in its room
    painter.galley(pos2(rect.right() - pad - text.size().x, y - text.size().y / 2.0), text, t.fg_text);
    resp
}

/// A trail of readings (0..1, oldest first, the newest at `last` as it eases there) as a line
/// with a fading area under it, from the right edge.
fn sparkline(painter: &Painter, t: &Theme, rect: Rect, trail: &[f32], last: f32, color: egui::Color32) {
    painter.rect_filled(rect.expand2(vec2(2.0, 1.0)), CornerRadius::same(2), t.bg_base.gamma_multiply(0.6));
    let step = rect.width() / (crate::usage::TRAIL - 1) as f32;
    let n = trail.len();
    let points: Vec<Pos2> = trail
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let v = if i + 1 == n { last } else { *v };
            pos2(rect.right() - (n - 1 - i) as f32 * step, rect.bottom() - v.clamp(0.0, 1.0) * rect.height())
        })
        .collect();
    let Some(&end) = points.last() else { return };
    let mut mesh = Mesh::default();
    for (i, p) in points.iter().enumerate() {
        mesh.colored_vertex(*p, color.gamma_multiply(0.3));
        mesh.colored_vertex(pos2(p.x, rect.bottom()), color.gamma_multiply(0.02));
        if i > 0 {
            let k = 2 * i as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::line(points, Stroke::new(1.25, color)));
    painter.circle_filled(end, 1.75, color);
}

/// A detector's signal over the last `span` seconds (`now` at the right edge, 0 to `top` up),
/// its threshold as a line, its hits as dots along the top: what to tune it by while the music
/// plays. Repaints while shown.
pub fn detector_graph(ui: &mut Ui, t: &Theme, points: &VecDeque<(f64, f32, bool)>, threshold: f32, top: f32, now: f64, span: f64) -> (Response, Option<f32>) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 80.0), Sense::drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(t.radius_sm), t.bg_field);
    let area = rect.shrink2(vec2(2.0, 6.0));
    let x = |at: f64| area.right() - ((now - at) / span) as f32 * area.width();
    let y = |v: f32| area.bottom() - (v / top).clamp(0.0, 1.0) * area.height();
    // the threshold's line moves when a drag starts on it, to where the pointer is
    let near = |p: Pos2| (p.y - y(threshold)).abs() <= 8.0;
    let grab = response.id.with("grab");
    if response.drag_started() {
        let on_line = ui.input(|i| i.pointer.press_origin()).is_some_and(near);
        ui.data_mut(|d| d.insert_temp(grab, on_line));
    }
    let dragging = response.dragged() && ui.data(|d| d.get_temp::<bool>(grab)).unwrap_or(false);
    let moved = response.interact_pointer_pos().filter(|_| dragging).map(|p| (area.bottom() - p.y) / area.height() * top);
    let lit = dragging || response.hover_pos().is_some_and(near);
    if lit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let line: Vec<Pos2> = points.iter().map(|&(at, v, _)| pos2(x(at), y(v))).collect();
    if line.len() >= 2 {
        let mut fill = Mesh::default();
        for (i, p) in line.iter().enumerate() {
            fill.colored_vertex(*p, t.cat_signal.gamma_multiply(0.3));
            fill.colored_vertex(pos2(p.x, area.bottom()), t.cat_signal.gamma_multiply(0.04));
            if i > 0 {
                let b = 2 * i as u32;
                fill.add_triangle(b - 2, b - 1, b);
                fill.add_triangle(b - 1, b + 1, b);
            }
        }
        painter.add(fill);
        painter.add(Shape::line(line, Stroke::new(1.2, t.cat_signal)));
    }
    painter.hline(area.x_range(), y(moved.unwrap_or(threshold)), Stroke::new(if lit { 2.5 } else { 1.5 }, t.accent));
    for &(at, _, hit) in points {
        if hit {
            painter.circle_filled(pos2(x(at), rect.top() + 3.0), 2.0, t.accent);
        }
    }
    ui.ctx().request_repaint();
    (response, moved)
}

/// A color as a field as wide as its row: its swatch (its alpha over a checkerboard) and its
/// hex. A click opens the picker: its name and swatch on top; saturation and brightness, hue,
/// alpha; its hex to type, paste or copy at the foot.
pub fn color(ui: &mut Ui, t: &Theme, name: &str, value: [f32; 4]) -> (Response, Option<[f32; 4]>) {
    let bytes = value.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let shown = egui::Color32::from_rgba_unmultiplied(bytes[0], bytes[1], bytes[2], bytes[3]);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_h + 4.0), Sense::click());
    let h = kit::hover(ui, t, &r);
    ui.painter().rect_filled(rect, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_field, t.fg_text, 0.03 + 0.03 * h));
    // the hex on the right, its swatch before it, the name on the left where there's room
    let hex = ui.painter().text(pos2(rect.right() - t.pad, rect.center().y), Align2::RIGHT_CENTER, hex_of(bytes), Theme::mono(t.text_sm), t.fg_text);
    let swatch = Rect::from_center_size(pos2(hex.left() - t.pad - 9.0, rect.center().y), vec2(18.0, 18.0));
    egui::widgets::color_picker::show_color_at(ui.painter(), shown, swatch);
    ui.painter().rect_stroke(swatch, CornerRadius::same(3), Stroke::new(1.0, t.line), StrokeKind::Inside);
    let room = swatch.left() - t.pad - rect.left() - t.pad;
    if room > 24.0 {
        kit::fit_text(ui.painter(), pos2(rect.left() + t.pad - 2.0, rect.center().y), Align2::LEFT_CENTER, name, t.body(), Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * h), room);
    }
    kit::focus_ring(ui, t, &r, t.radius_sm);
    let id = r.id.with("picker");
    let mut out = None;
    const W: f32 = 232.0;
    let popup = egui::Popup::menu(&r).id(id).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(kit::popover_frame(ui.ctx(), t).inner_margin(0)).width(W);
    popup.show(|ui| {
        ui.spacing_mut().item_spacing = vec2(t.gap, 0.0);
        // the hue a grey would lose, and the hex as it's being typed, while it's open
        let (mut hsva, mut hex) = ui.data(|d| d.get_temp::<([f32; 4], String)>(id)).filter(|(c, _)| to_bytes(*c) == bytes).unwrap_or_else(|| (to_hsva(bytes), hex_of(bytes)));
        let pad = egui::Margin::same(t.pad as i8);
        // header: what it colors, and how it looks now
        egui::Frame::new().inner_margin(pad).show(ui, |ui| {
            ui.set_width(W - 2.0 * t.pad);
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).strong().color(t.fg_text));
                kit::right(ui, |ui| {
                    let (sw, _) = ui.allocate_exact_size(vec2(36.0, 18.0), Sense::hover());
                    egui::widgets::color_picker::show_color_at(ui.painter(), shown, sw);
                    ui.painter().rect_stroke(sw, CornerRadius::ZERO, Stroke::new(1.0, t.line), StrokeKind::Inside);
                });
            });
        });
        kit::menu_line(ui, t);
        // the color: saturation across and brightness up, then hue, then alpha
        egui::Frame::new().inner_margin(pad).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, t.pad);
            let w = W - 2.0 * t.pad;
            let changed = sv_square(ui, t, &mut hsva, w) | strip(ui, t, &mut hsva, w, 0) | strip(ui, t, &mut hsva, w, 3);
            if changed {
                hex = hex_of(to_bytes(hsva));
                out = Some(to_bytes(hsva));
            }
        });
        kit::menu_line(ui, t);
        // footer: its hex, to type, paste or copy
        egui::Frame::new().inner_margin(pad).show(ui, |ui| {
            ui.horizontal(|ui| {
                let copy_w = 64.0;
                let typed = ui.add_sized(vec2(W - 2.0 * t.pad - copy_w - t.gap, t.control_h), kit::field(&mut hex).font(Theme::mono(t.text_sm)));
                if typed.changed()
                    && let Some(b) = parse_hex(&hex)
                {
                    hsva = to_hsva(b);
                    out = Some(b);
                }
                if kit::action_sized(ui, t, kit::Variant::Outline, None, tr!("Copy"), Some(copy_w)).clicked() {
                    ui.ctx().copy_text(hex.clone());
                }
            });
        });
        ui.data_mut(|d| d.insert_temp(id, (hsva, hex)));
    });
    (r, out.map(|b| b.map(|c| f32::from(c) / 255.0)))
}

/// sRGB bytes as hue, saturation, brightness and alpha, 0..1.
fn to_hsva(c: [u8; 4]) -> [f32; 4] {
    let [r, g, b, a] = c.map(|x| f32::from(x) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max, a]
}

/// Hue, saturation, brightness and alpha as sRGB, 0..1.
fn rgb_of([h, s, v, a]: [f32; 4]) -> [f32; 4] {
    let k = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [k(5.0), k(3.0), k(1.0), a]
}

fn to_bytes(hsva: [f32; 4]) -> [u8; 4] {
    rgb_of(hsva).map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn color32(c: [f32; 4]) -> egui::Color32 {
    let [r, g, b, a] = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Saturation across, brightness up, at the color's hue; a ring where it is.
fn sv_square(ui: &mut Ui, t: &Theme, hsva: &mut [f32; 4], w: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, w * 0.62), Sense::click_and_drag());
    let n = 24;
    let mut mesh = Mesh::default();
    for row in 0..=n {
        for col in 0..=n {
            let (s, v) = (col as f32 / n as f32, 1.0 - row as f32 / n as f32);
            mesh.colored_vertex(pos2(rect.left() + s * rect.width(), rect.top() + (1.0 - v) * rect.height()), color32(rgb_of([hsva[0], s, v, 1.0])));
        }
    }
    for row in 0..n {
        for col in 0..n {
            let i = (row * (n + 1) + col) as u32;
            let j = i + (n + 1) as u32;
            mesh.add_triangle(i, i + 1, j);
            mesh.add_triangle(i + 1, j + 1, j);
        }
    }
    ui.painter().add(Shape::mesh(mesh));
    ui.painter().rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, t.line), StrokeKind::Inside);
    let mut changed = false;
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
        hsva[1] = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        hsva[2] = 1.0 - ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let at = pos2(rect.left() + hsva[1] * rect.width(), rect.top() + (1.0 - hsva[2]) * rect.height());
    ui.painter().circle_stroke(at, 6.0, Stroke::new(2.0, egui::Color32::WHITE));
    ui.painter().circle_stroke(at, 7.5, Stroke::new(1.0, egui::Color32::BLACK.gamma_multiply(0.5)));
    changed
}

/// A strip for one of the color's numbers: `k` 0 the hue (every hue across), 3 the alpha (the
/// color from clear to solid, over a checkerboard). A bar where it is.
fn strip(ui: &mut Ui, t: &Theme, hsva: &mut [f32; 4], w: f32, k: usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 14.0), Sense::click_and_drag());
    let n = 36;
    let mut mesh = Mesh::default();
    if k == 3 {
        // the checkerboard first: a clear color shows it
        egui::widgets::color_picker::show_color_at(ui.painter(), egui::Color32::TRANSPARENT, rect);
    }
    for i in 0..=n {
        let x = i as f32 / n as f32;
        let mut c = *hsva;
        if k == 0 {
            c = [x, 1.0, 1.0, 1.0];
        }
        c[3] = if k == 3 { x } else { 1.0 };
        let px = rect.left() + x * rect.width();
        mesh.colored_vertex(pos2(px, rect.top()), color32(rgb_of(c)));
        mesh.colored_vertex(pos2(px, rect.bottom()), color32(rgb_of(c)));
    }
    for i in 0..n as u32 {
        mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
        mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
    }
    ui.painter().add(Shape::mesh(mesh));
    ui.painter().rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, t.line), StrokeKind::Inside);
    let mut changed = false;
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.is_pointer_button_down_on()) {
        hsva[k] = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        changed = true;
    }
    let x = rect.left() + hsva[k] * rect.width();
    let bar = Rect::from_center_size(pos2(x, rect.center().y), vec2(4.0, rect.height() + 4.0));
    ui.painter().rect(bar, CornerRadius::same(1), egui::Color32::WHITE, Stroke::new(1.0, egui::Color32::BLACK.gamma_multiply(0.5)), StrokeKind::Outside);
    changed
}

/// A color as hex: #rrggbb, with its alpha (#rrggbbaa) when it isn't opaque.
fn hex_of(c: [u8; 4]) -> String {
    let rgb = format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    if c[3] == 255 { rgb } else { format!("{rgb}{:02x}", c[3]) }
}

/// A color typed as hex: #rgb, #rrggbb or #rrggbbaa, the # optional.
fn parse_hex(text: &str) -> Option<[u8; 4]> {
    let h = text.trim().trim_start_matches('#');
    let digits = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        3 => Some([digits(0, 1)? * 17, digits(1, 1)? * 17, digits(2, 1)? * 17, 255]),
        6 => Some([digits(0, 2)?, digits(2, 2)?, digits(4, 2)?, 255]),
        8 => Some([digits(0, 2)?, digits(2, 2)?, digits(4, 2)?, digits(6, 2)?]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_to_pick_is_in_the_fonts() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let font = egui::FontId::proportional(16.0);
        let missing: Vec<&str> = EMOJI.iter().copied().filter(|e| !ctx.fonts_mut(|f| f.has_glyphs(&font, e))).collect();
        assert!(missing.is_empty(), "Noto Emoji can't draw {missing:?}");
    }

    #[test]
    fn the_picker_reads_back_the_color_it_shows() {
        for c in [[255, 0, 128, 255], [18, 52, 86, 120], [0, 0, 0, 0], [200, 200, 200, 255], [10, 250, 30, 9]] {
            assert_eq!(to_bytes(to_hsva(c)), c, "{c:?}");
        }
        assert_eq!(to_hsva([255, 0, 0, 255])[..3], [0.0, 1.0, 1.0]);
    }

    #[test]
    fn hex_reads_what_it_writes_and_the_usual_shorthands() {
        for c in [[255, 0, 128, 255], [18, 52, 86, 120], [0, 0, 0, 0]] {
            assert_eq!(parse_hex(&hex_of(c)), Some(c));
        }
        assert_eq!(hex_of([255, 0, 128, 255]), "#ff0080");
        assert_eq!(parse_hex(" FFF "), Some([255, 255, 255, 255]));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(parse_hex("#ééé"), None);
    }
}
