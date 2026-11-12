//! Layout kit.
//! From Ravune (bd10814), trimmed for Velo.
//! Every region is a stack of fixed-height bars and one edge-to-edge scroll area, split by
//! 1-px dividers. Space is padding inside elements, never outer margins, so
//! scroll bars land on the dividers. Motion ported from React Bits: a pointer spotlight on
//! cards, a sliding tab pill, a glowing focus ring, fading list edges.

use std::hash::Hash;

use eframe::egui::{self, Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Layout, Mesh, Painter, Pos2, Rect, Response, RichText, Role, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, pos2, vec2};

use crate::theme::Theme;

/// Cubic ease-out, 0..1.
pub fn ease(k: f32) -> f32 {
    1.0 - (1.0 - k.clamp(0.0, 1.0)).powi(3)
}

/// A spring with Sileo's (Framer Motion's) 0.25 bounce that settles at k = 1: 0 → 1 by k = 0.34,
/// then about 3 % over and back.
pub fn spring(k: f32) -> f32 {
    if k >= 1.0 {
        return 1.0;
    }
    let k = k.max(0.0);
    1.0 - (-8.06 * k).exp() * ((7.11 * k).cos() + 1.134 * (7.11 * k).sin())
}

/// Eased 0..1 hover amount: hovers fade in and out instead of snapping.
pub fn hover(ui: &Ui, t: &Theme, resp: &Response) -> f32 {
    ui.ctx().animate_bool_with_time(resp.id.with("hover"), resp.hovered() || resp.has_focus(), t.motion)
}

/// Starts a region (panel, window): children stack with no gaps; bars and content bring
/// their own padding.
pub fn region(ui: &mut Ui) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
}

/// A full-width strip `t.bar_h` tall with horizontal padding and a divider below.
/// Contents are laid out left to right, vertically centred.
pub fn bar<R>(ui: &mut Ui, t: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    bar_with(ui, t, t.bg_panel, add)
}

pub fn bar_with<R>(ui: &mut Ui, t: &Theme, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = strip_rect(ui, t, t.bar_h, fill);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.line));
    strip_child(ui, t, rect, add)
}

/// Like [`bar`] without the divider, at any height (status bar).
pub fn strip<R>(ui: &mut Ui, t: &Theme, height: f32, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = strip_rect(ui, t, height, fill);
    strip_child(ui, t, rect, add)
}

/// A strip's room, filled. The first thing in its region rounds its top corners as a card's
/// (inside its 1-px border): on a docked panel the panel's own fill hides that.
fn strip_rect(ui: &mut Ui, t: &Theme, height: f32, fill: Color32) -> Rect {
    let first = ui.cursor().top() <= ui.max_rect().top() + 0.5;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let r = if first { t.radius_md.saturating_sub(1) } else { 0 };
    ui.painter().rect_filled(rect, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, fill);
    rect
}

fn strip_child<R>(ui: &mut Ui, t: &Theme, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = rect.shrink2(vec2(t.pad, 0.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = vec2(t.gap, t.gap);
    add(&mut child)
}

/// A row that scrolls sideways when it's wider than its room (a narrow window, a panel open
/// beside it): the wheel scrolls it too, its bar floats over it.
pub fn hscroll<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.style_mut().always_scroll_the_only_direction = true;
        ui.style_mut().spacing.scroll = egui::style::ScrollStyle::floating();
        egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| ui.horizontal(add).inner).inner
    })
    .inner
}

/// A one-line text field, as tall as a button. egui leaves the text ~2 px above the middle of
/// its box: a taller top margin sets it (and the placeholder) right in the middle.
pub fn field(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(egui::Margin { left: 8, right: 8, top: 8, bottom: 4 })
}

/// A number to scrub, as in Figma's fields: drag on it (up or right for more, down or left for
/// less) or click it to type; `label` sits inside on the left, the number in mono.
pub fn scrub<N: egui::emath::Numeric>(ui: &mut Ui, t: &Theme, label: &str, value: &mut N, range: std::ops::RangeInclusive<N>, width: f32) -> Response {
    let fill = t.bg_field;
    let inner = Frame::new().fill(fill).corner_radius(CornerRadius::same(t.radius_sm)).inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
        ui.set_width(width - 16.0);
        ui.set_height(t.control_h);
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(label).font(Theme::mono(t.text_xs)).color(t.fg_muted));
            ui.style_mut().override_font_id = Some(Theme::mono(t.text_sm));
            let v = ui.visuals_mut();
            for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
                (w.weak_bg_fill, w.bg_fill, w.bg_stroke) = (fill, fill, Stroke::NONE);
            }
            ui.add_sized(ui.available_size(), egui::DragValue::new(value).range(range).speed(1.0))
        })
        .inner
    });
    let r = inner.inner;
    focus_ring(ui, t, &r, t.radius_sm);
    r
}

/// Right-aligned group inside a bar.
pub fn right<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(Layout::right_to_left(Align::Center), add).inner
}

/// A 1-px vertical divider spanning the bar it sits in.
pub fn vsep(ui: &mut Ui, t: &Theme) {
    let h = ui.max_rect().height();
    let (rect, _) = ui.allocate_exact_size(vec2(1.0 + 2.0 * t.gap, h), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, t.line));
}

/// Small-caps section label.
pub fn caps(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    ui.label(RichText::new(sentence(text)).size(t.text_sm).color(t.fg_muted))
}

/// A label written in capitals (as the strings and their translations are) set in sentence
/// case, read faster than tracked capitals: its first letter up, the rest down, acronyms kept.
pub fn sentence(text: &str) -> String {
    const KEPT: [&str; 16] = ["AI", "MCP", "MIDI", "OSC", "NDI", "BPM", "GPU", "CPU", "UI", "ISF", "WGSL", "FPS", "LFO", "HAP", "API", "URL"];
    let mut out = String::with_capacity(text.len());
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // an acronym, bare or in brackets ("(MCP)")
        let bare = word.trim_matches(|c: char| !c.is_alphanumeric());
        if KEPT.contains(&bare) {
            out.push_str(word);
        } else if i == 0 {
            let mut chars = word.chars();
            out.extend(chars.next().into_iter().flat_map(char::to_uppercase));
            out.push_str(&chars.as_str().to_lowercase());
        } else {
            out.push_str(&word.to_lowercase());
        }
    }
    out
}

/// A titled group of controls: a divider edge to edge above it (none above a panel's
/// `first`), room around it, its icon and title, then what's in it. A click on the title folds
/// it away (its caret turns), and again brings it back; each title remembers, whatever node
/// shows it. Folded, `add` doesn't run: what it returns is its default.
pub fn section<R: Default>(ui: &mut Ui, t: &Theme, icon: &str, title: &str, first: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !first {
        ui.add_space(t.pad);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline((line.left() - t.pad)..=(line.right() + t.pad), line.center().y, Stroke::new(1.0, t.line));
        ui.add_space(t.pad);
    }
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), Id::unique(("section", title)), true);
    let open = state.openness(ui.ctx());
    if section_title(ui, t, icon, title, open).clicked() {
        state.toggle(ui);
    }
    state
        .show_body_unindented(ui, |ui| {
            ui.add_space(2.0);
            add(ui)
        })
        .map_or_else(R::default, |body| body.inner)
}

/// A section's title the width of its panel, its caret on the right turned as open as it is.
fn section_title(ui: &mut Ui, t: &Theme, icon: &str, title: &str, open: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.text_md + 4.0), Sense::click());
    let h = hover(ui, t, &resp);
    let color = Theme::mix(t.fg_muted, t.fg_text, h);
    let painter = ui.painter();
    let y = rect.center().y;
    icon_at(painter, pos2(rect.left() + 6.0, y), icon, 12.0, color);
    let words = egui::WidgetText::from(RichText::new(sentence(title)).size(t.text_sm).color(color)).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
    painter.galley(pos2(rect.left() + 18.0, y - words.size().y / 2.0), words, color);
    icon_turned(painter, pos2(rect.right() - 6.0, y), crate::icons::CARET_DOWN, 12.0, color, -std::f32::consts::FRAC_PI_2 * (1.0 - open));
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, open > 0.5, title));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A section's title: its icon, then its words in small caps.
pub fn heading(ui: &mut Ui, t: &Theme, icon: &str, title: &str) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (spot, _) = ui.allocate_exact_size(vec2(12.0, t.text_md), Sense::hover());
        icon_at(ui.painter(), spot.center(), icon, 12.0, t.fg_muted);
        caps(ui, t, title)
    })
    .inner
}

/// The name of the control under it (a choice, a timeline), muted.
pub fn label(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    ui.add_space(2.0);
    ui.label(RichText::new(text).font(t.body()).color(t.fg_muted))
}

/// Help under a control, a line or two: small, muted, its lines given room.
pub fn hint(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    note(ui, t, None, text, t.fg_muted)
}

/// Something that went wrong (a file that won't load), led by a warning sign.
pub fn warning(ui: &mut Ui, t: &Theme, text: &str) -> Response {
    note(ui, t, Some(crate::icons::WARNING), text, t.warn)
}

/// [`hint`] in `color`, led by `icon` if it has one, its lines wrapped past it.
pub fn note(ui: &mut Ui, t: &Theme, icon: Option<&str>, text: &str, color: Color32) -> Response {
    let line = t.text_sm + 5.0;
    let indent = if icon.is_some() { 18.0 } else { 0.0 };
    let width = ui.available_width();
    let format = egui::TextFormat { font_id: t.small(), color, line_height: Some(line), ..Default::default() };
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    job.wrap.max_width = (width - indent).max(1.0);
    let galley = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    if let Some(icon) = icon {
        icon_at(ui.painter(), pos2(rect.left() + 6.0, rect.top() + line / 2.0), icon, 13.0, color);
    }
    ui.painter().galley(pos2(rect.left() + indent, rect.top()), galley, color);
    resp
}

/// A Preferences page's head: its icon on a tile of the accent, its name large, and a line on
/// what the page is for.
pub fn page_title(ui: &mut Ui, t: &Theme, icon: &str, title: &str, blurb: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.pad;
        let (tile, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
        ui.painter().rect_filled(tile, CornerRadius::same(t.radius_md), t.accent.gamma_multiply(0.16));
        icon_at(ui.painter(), tile.center(), icon, 18.0, t.accent);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).size(t.text_lg + 3.0).strong().color(t.fg_text));
            ui.label(RichText::new(blurb).font(t.small()).color(t.fg_muted));
        });
    });
    ui.add_space(t.pad * 1.5);
}

/// A group of preferences, set apart by its heading and the room round it, not by a frame: the
/// name in the text's own color over its rows, straight on the page. Rows end with room of
/// their own ([`setting`] does); after anything else, add `t.pad`.
pub fn card<R>(ui: &mut Ui, t: &Theme, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !title.is_empty() {
        ui.label(RichText::new(sentence(title)).size(t.text_md).strong().color(t.fg_text));
        ui.add_space(t.gap);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, t.line));
        ui.add_space(t.pad);
    }
    let out = add(ui);
    ui.add_space(t.pad * 1.25);
    out
}

/// A hairline between two rows of a [`card`], with the room under it a row has.
pub fn divider(ui: &mut Ui, t: &Theme) {
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, t.line.gamma_multiply(0.6)));
    ui.add_space(t.pad);
}

/// Words that wrap under themselves, `indent` in from the left: where a wrapped label would run
/// back to the row's start.
fn wrapped(ui: &mut Ui, text: &str, font: FontId, color: Color32, line: f32, indent: f32) -> egui::Rect {
    let width = ui.available_width();
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat { font_id: font, color, line_height: Some(line), ..Default::default() });
    job.wrap.max_width = (width - indent).max(1.0);
    let galley = ui.painter().layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    ui.painter().galley(pos2(rect.left() + indent, rect.top()), galley, color);
    rect
}

/// Numbered steps: each number on a badge, its words wrapped beside it.
pub fn steps(ui: &mut Ui, t: &Theme, steps: &[&str]) {
    let line = t.text_md + 6.0;
    for (i, step) in steps.iter().enumerate() {
        let rect = wrapped(ui, step, t.body(), t.fg_text, line, 30.0);
        let badge = pos2(rect.left() + 10.0, rect.top() + line / 2.0);
        ui.painter().circle_filled(badge, 10.0, t.accent.gamma_multiply(0.18));
        ui.painter().text(badge, Align2::CENTER_CENTER, (i + 1).to_string(), Theme::mono(t.text_xs), t.accent);
        ui.add_space(t.gap + 2.0);
    }
}

/// A paragraph of a page: body size, wrapped, its lines given room.
pub fn para(ui: &mut Ui, t: &Theme, text: &str, color: Color32) {
    wrapped(ui, text, t.body(), color, t.text_md + 6.0, 0.0);
    ui.add_space(t.gap);
}

/// Something to know, set apart in a box tinted `color`, led by `icon`.
pub fn callout(ui: &mut Ui, t: &Theme, icon: &str, text: &str, color: Color32) {
    Frame::new().fill(color.gamma_multiply(0.10)).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(t.pad as i8, t.gap as i8 + 3)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        note(ui, t, Some(icon), text, Theme::mix(color, t.fg_text, 0.55));
    });
}

/// A command or a snippet on one line, mono in a framed box, cut short where it doesn't fit,
/// with a button at its end that copies it: true when that's clicked.
pub fn snippet(ui: &mut Ui, t: &Theme, text: &str) -> bool {
    Frame::new()
        .fill(t.bg_field)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_sm)
        .inner_margin(egui::Margin { left: t.pad as i8, right: 2, top: 2, bottom: 2 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let copied = icon_button(ui, t, crate::icons::COPY, tr!("Copy"), false).clicked();
                    // the words from the left, up to the button
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(text).font(Theme::mono(t.text_xs)).color(t.fg_text)).truncate());
                    });
                    copied
                })
                .inner
            })
            .inner
        })
        .inner
}

/// A preference as a row: what it is with a line on what it does under it, its control on the
/// right, room under it.
pub fn setting<R>(ui: &mut Ui, t: &Theme, title: &str, detail: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = ui
        .horizontal_top(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let out = add(ui);
                ui.add_space(t.pad * 2.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    // the title's line on the control's middle
                    ui.add_space((t.control_h - t.text_md - 4.0) / 2.0);
                    ui.label(RichText::new(title).font(t.body()).color(t.fg_text));
                    if !detail.is_empty() {
                        hint(ui, t, detail);
                    }
                });
                out
            })
            .inner
        })
        .inner;
    ui.add_space(t.pad);
    out
}

/// An on/off switch: a pill whose knob springs to the lit side. A click flips `on`.
pub fn switch(ui: &mut Ui, t: &Theme, on: &mut bool, label: &str) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let enabled = ui.is_enabled();
    let k = ui.ctx().animate_bool_with_time(resp.id.with("switch"), *on, t.motion * 1.5);
    let h = if enabled { hover(ui, t, &resp) } else { 0.0 };
    // the knob overshoots a little and settles, both ways
    let x = if *on { spring(k) } else { 1.0 - spring(1.0 - k) };
    let painter = ui.painter();
    // switched off by another switch: it keeps its place, but nothing about it looks on
    let lit = if enabled { k } else { 0.2 * k };
    let track = Theme::mix(Theme::mix(t.bg_control, t.fg_text, 0.08 * h), t.accent, lit);
    painter.rect_filled(rect, CornerRadius::same(10), track);
    let knob = pos2(rect.left() + 10.0 + x * (rect.width() - 20.0), rect.center().y);
    let knob_color = if enabled { Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * k.max(h)) } else { Theme::mix(t.bg_control, t.fg_muted, 0.5) };
    painter.circle_filled(knob, 7.0, knob_color);
    focus_ring(ui, t, &resp, 10);
    resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, *on, label));
    resp
}

/// Where a file goes while there's none: a dashed card with an icon, what to pick and what it
/// takes. It lights up under the pointer; a click is the caller's file dialog.
pub fn drop_zone(ui: &mut Ui, t: &Theme, icon: &str, title: &str, detail: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::click());
    let h = hover(ui, t, &resp);
    let shape = if resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    let painter = ui.painter();
    painter.rect_filled(shape, CornerRadius::same(t.radius_md), Theme::mix(t.bg_panel, t.accent, 0.03 + 0.05 * h));
    painter.extend(Shape::dashed_line(&rounded_outline(shape.shrink(0.5), f32::from(t.radius_md)), Stroke::new(1.0, Theme::mix(t.line, t.accent, 0.2 + 0.6 * h)), 4.0, 3.0));
    // the icon lifts toward the pointer
    let x = shape.center().x;
    icon_at(painter, pos2(x, shape.top() + 24.0 - 2.0 * h), icon, 20.0, Theme::mix(t.fg_muted, t.accent, h));
    fit_text(painter, pos2(x, shape.top() + 48.0), Align2::CENTER_CENTER, title, t.body(), t.fg_text, shape.width() - 2.0 * t.pad);
    fit_text(painter, pos2(x, shape.top() + 65.0), Align2::CENTER_CENTER, detail, t.small(), t.fg_muted, shape.width() - 2.0 * t.pad);
    focus_ring(ui, t, &resp, t.radius_md);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, title));
    resp
}

/// The outline of `rect` with its corners rounded by `r`, closed, for a dashed border.
fn rounded_outline(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [(rect.right_top() + vec2(-r, r), -0.25), (rect.right_bottom() + vec2(-r, -r), 0.0), (rect.left_bottom() + vec2(r, -r), 0.25), (rect.left_top() + vec2(r, r), 0.5)];
    let mut points: Vec<Pos2> = corners.iter().flat_map(|&(c, start)| (0..=6).map(move |k| c + Vec2::angled((start + k as f32 / 24.0) * std::f32::consts::TAU) * r)).collect();
    points.push(points[0]);
    points
}

/// The rest of the region as a vertical scroll area, edge to edge; content gets `t.pad`
/// padding (or none, for full-width rows). The list edges fade when there is more to scroll.
pub fn scroll<R>(ui: &mut Ui, t: &Theme, id: impl Hash + std::fmt::Debug, padded: bool, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    // its bar floats over the padding: a bar that took room would narrow the content, and a list
    // as tall as it is wide (the scene cards) would then fit, lose the bar, widen, and flicker
    let s = ui.style().spacing.scroll;
    ui.style_mut().spacing.scroll = egui::style::ScrollStyle { floating: true, floating_width: s.bar_width / 2.0, floating_allocated_width: 0.0, ..s };
    let out = egui::ScrollArea::vertical().id_salt(id).auto_shrink(false).show(ui, |ui| {
        let pad = if padded { t.pad } else { 0.0 };
        Frame::new()
            .inner_margin(pad)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing = if padded { vec2(t.gap, t.gap) } else { Vec2::ZERO };
                add(ui)
            })
            .inner
    });
    let (view, offset, content) = (out.inner_rect, out.state.clamped_offset().y, out.content_size.y);
    let fade = 14.0;
    if offset > 1.0 {
        gradient(ui, Rect::from_min_size(view.min, vec2(view.width(), fade)), fill, true);
    }
    if offset + view.height() < content - 1.0 {
        gradient(ui, Rect::from_min_size(pos2(view.left(), view.bottom() - fade), vec2(view.width(), fade)), fill, false);
    }
    ui.style_mut().spacing.scroll = s;
    out.inner
}

/// Vertical fade from `color` (opaque edge) to transparent, `top` = opaque at the top.
fn gradient(ui: &Ui, rect: Rect, color: Color32, top: bool) {
    let clear = color.gamma_multiply(0.0);
    let (a, b) = if top { (color, clear) } else { (clear, color) };
    let mut mesh = Mesh::default();
    for (pos, c) in [(rect.left_top(), a), (rect.right_top(), a), (rect.right_bottom(), b), (rect.left_bottom(), b)] {
        mesh.colored_vertex(pos, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(Shape::mesh(mesh));
}

/// A centred icon, a title and a muted line under it: what an area shows while it has
/// nothing in it yet.
pub fn empty_state(ui: &mut Ui, t: &Theme, icon: &str, title: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = t.gap;
        ui.add_space((ui.available_height() * 0.35 - 40.0).max(t.pad));
        ui.label(RichText::new(icon).size(28.0).color(t.fg_muted));
        ui.add_space(t.gap);
        ui.label(RichText::new(title).strong().color(t.fg_text));
        ui.label(RichText::new(text).color(t.fg_muted));
    });
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// One line of text no wider than `width`, cut with "…" when it doesn't fit. Returns where
/// it went and whether it was cut (then show the whole of it on hover).
pub fn fit_text(painter: &Painter, pos: Pos2, align: Align2, text: impl Into<String>, font: FontId, color: Color32, width: f32) -> (Rect, bool) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap = egui::text::TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    let (rect, cut) = (align.anchor_size(pos, galley.size()), galley.elided);
    painter.galley(rect.min, galley, color);
    (rect, cut)
}

/// An icon centred on its ink: glyphs carry uneven side bearings, so centring their text box
/// leaves them off to one side.
pub fn icon_at(painter: &Painter, center: Pos2, icon: &str, size: f32, color: Color32) {
    icon_turned(painter, center, icon, size, color, 0.0);
}

/// [`icon_at`], turned by `angle` (radians, clockwise) about its ink's centre: one sidebar
/// glyph serves every edge.
pub fn icon_turned(painter: &Painter, center: Pos2, icon: &str, size: f32, color: Color32, angle: f32) {
    let galley = painter.layout_no_wrap(icon.to_string(), FontId::proportional(size), color);
    let ink = galley.mesh_bounds.center().to_vec2();
    let turn = egui::emath::Rot2::from_angle(angle);
    painter.add(egui::epaint::TextShape::new(center - turn * ink, galley, color).with_angle(angle));
}

/// Square icon button. `on` lights it like a hardware toggle. `label` names it for screen
/// readers and the tooltip (add the shortcut there).
pub fn icon_button(ui: &mut Ui, t: &Theme, icon: &str, label: &str, on: bool) -> Response {
    icon_button_turned(ui, t, icon, 0.0, label, on)
}

/// [`icon_button`] with its glyph turned by `angle` (radians, clockwise).
pub fn icon_button_turned(ui: &mut Ui, t: &Theme, icon: &str, angle: f32, label: &str, on: bool) -> Response {
    icon_button_with(ui, t, icon, angle, label, on, None)
}

/// A panel's show / hide toggle: quieter than a lit button (several sit together and are
/// usually on), its glyph bright while the panel shows and dim while it's hidden.
pub fn panel_toggle(ui: &mut Ui, t: &Theme, icon: &str, angle: f32, label: &str, open: bool) -> Response {
    let resp = icon_button_with(ui, t, icon, angle, label, false, Some(if open { t.fg_text } else { t.fg_muted.gamma_multiply(0.7) }));
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, open, label));
    resp
}

fn icon_button_with(ui: &mut Ui, t: &Theme, icon: &str, angle: f32, label: &str, on: bool, color: Option<Color32>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(t.control_h, t.control_h), Sense::click());
    let enabled = ui.is_enabled();
    let h = if enabled { hover(ui, t, &resp) } else { 0.0 };
    // pressed: the button sinks a little; in a group, inside the group's box
    let shape = if resp.is_pointer_button_down_on() { rect.shrink(1.5) } else { rect };
    let shape = if ui.visuals().button_frame { shape } else { shape.shrink(3.0) };
    let painter = ui.painter();
    if on {
        painter.rect_filled(shape, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_control, t.accent, 0.16 + 0.08 * h));
    } else if h > 0.0 {
        painter.rect_filled(shape, CornerRadius::same(t.radius_sm), t.bg_control.gamma_multiply(h));
    }
    let color = match (enabled, on, color) {
        (false, ..) => t.fg_muted.gamma_multiply(0.5),
        (_, true, _) => t.accent,
        (_, _, Some(c)) => Theme::mix(c, t.fg_text, h),
        _ => Theme::mix(t.fg_muted, t.fg_text, h),
    };
    icon_turned(painter, shape.center(), icon, if shape == rect { 16.0 } else { 15.0 }, color, angle);
    focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::selected(Role::Button, enabled, on, label));
    resp.on_hover_text(label)
}

/// An icon toggle that turns red while it's on: a state the whole room sees (Blackout).
pub fn alarm_button(ui: &mut Ui, t: &Theme, icon: &str, label: &str, on: bool) -> Response {
    if !on {
        return icon_button(ui, t, icon, label, false);
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(t.control_h, t.control_h), Sense::click());
    let h = hover(ui, t, &resp);
    let shape = if ui.visuals().button_frame { rect } else { rect.shrink(3.0) };
    ui.painter().rect_filled(shape, CornerRadius::same(t.radius_sm), Theme::mix(t.bg_control, t.error, 0.3 + 0.1 * h));
    icon_at(ui.painter(), shape.center(), icon, 15.0, t.error);
    focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::selected(Role::Button, true, true, label));
    resp.on_hover_text(label)
}

/// Text button with an optional leading icon, sized to its content.
pub fn button(ui: &mut Ui, t: &Theme, icon: Option<&str>, text: &str, on: bool) -> Response {
    let font = t.body();
    let text_color = if on { t.accent } else { t.fg_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), text_color);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 2.0 * t.pad, t.control_h);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let h = if enabled { hover(ui, t, &resp) } else { 0.0 };
    // in a group the group's box is its frame: only hovered or on does it fill, inside the box
    let framed = ui.visuals().button_frame;
    let base = if framed { t.bg_control } else { t.bg_field };
    let fill = if on { Theme::mix(base, t.accent, 0.16 + 0.08 * h) } else { Theme::mix(base, t.fg_text, 0.08 * h) };
    let painter = ui.painter();
    let shape = if resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    let shape = if framed { shape } else { shape.shrink(3.0) };
    if framed || on || h > 0.0 {
        painter.rect_filled(shape, CornerRadius::same(if framed { t.radius_sm } else { t.radius_sm - 2 }), fill);
    }
    let mut x = rect.left() + t.pad;
    if let Some(icon) = icon {
        icon_at(painter, pos2(x + 7.0, rect.center().y), icon, 15.0, if on { t.accent } else { t.fg_muted });
        x += icon_w;
    }
    let color = if enabled { text_color } else { t.fg_muted };
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::selected(Role::Button, enabled, on, text));
    resp
}

/// Controls that belong together on a bar (the sound, the stage) in one rounded box, divided by
/// [`group_line`]: its buttons have no box of their own.
pub fn group<R>(ui: &mut Ui, t: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    let under = ui.painter().add(Shape::Noop);
    let out = ui.scope(|ui| {
        ui.visuals_mut().button_frame = false;
        ui.spacing_mut().item_spacing.x = 0.0;
        add(ui)
    });
    let rect = Rect::from_center_size(out.response.rect.center(), vec2(out.response.rect.width(), t.control_h));
    ui.painter().set(under, egui::epaint::RectShape::new(rect, CornerRadius::same(t.radius_sm), t.bg_field, Stroke::new(1.0, t.line), StrokeKind::Inside));
    out.inner
}

/// A divider between the parts of a [`group`].
pub fn group_line(ui: &mut Ui, t: &Theme) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, t.control_h), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range().shrink(7.0), Stroke::new(1.0, t.line));
}

/// A [`button`] with no box until it's hovered: what a bar shows plainly (the project's name).
pub fn ghost(ui: &mut Ui, t: &Theme, icon: Option<&str>, text: &str) -> Response {
    ui.scope(|ui| {
        ui.visuals_mut().button_frame = false;
        button(ui, t, icon, text, false)
    })
    .inner
}

/// A choice that drops a list: a field as wide as its row (the sliders' look), what's chosen
/// on the left, the caret on the right. Every select is this one.
pub fn select(ui: &mut Ui, t: &Theme, icon: Option<&str>, text: &str) -> Response {
    select_with(ui, t, icon, None, text)
}

/// A [`select`] with its name inside on the left and what's chosen on the right, as a
/// slider shows its name and value.
pub fn select_named(ui: &mut Ui, t: &Theme, name: &str, text: &str) -> Response {
    select_with(ui, t, None, Some(name), text)
}

fn select_with(ui: &mut Ui, t: &Theme, icon: Option<&str>, name: Option<&str>, text: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_h + 4.0), Sense::click());
    let h = if ui.is_enabled() { hover(ui, t, &resp) } else { 0.0 };
    // the caret turns over while its list is open
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let turn = ui.ctx().animate_bool_with_time(resp.id.with("open"), open, t.motion);
    let painter = ui.painter();
    let round = CornerRadius::same(t.radius_sm);
    painter.rect_filled(rect, round, Theme::mix(t.bg_field, t.fg_text, 0.03 + 0.03 * h.max(turn)));
    let mut x = rect.left() + t.pad;
    if let Some(icon) = icon {
        icon_at(painter, pos2(x + 7.0, rect.center().y), icon, 15.0, t.fg_muted);
        x += 20.0;
    }
    let caret = pos2(rect.right() - t.pad - 6.0, rect.center().y);
    icon_turned(painter, caret, crate::icons::CARET_DOWN, 13.0, Theme::mix(t.fg_muted, t.fg_text, h.max(turn)), turn * std::f32::consts::PI);
    let cut = match name {
        Some(name) => {
            let (named, _) = fit_text(painter, pos2(x, rect.center().y), Align2::LEFT_CENTER, name, t.body(), Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * h), (caret.x - x) * 0.5);
            fit_text(painter, pos2(caret.x - 12.0, rect.center().y), Align2::RIGHT_CENTER, text, t.body(), t.fg_text, caret.x - 12.0 - named.right() - t.pad).1
        }
        None => fit_text(painter, pos2(x, rect.center().y), Align2::LEFT_CENTER, text, t.body(), t.fg_text, caret.x - 12.0 - x).1,
    };
    focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::labeled(Role::ComboBox, true, text));
    if cut { resp.on_hover_text(text) } else { resp }
}

/// Buttons that share their row's width evenly: the actions under a section.
pub fn actions(ui: &mut Ui, t: &Theme, items: &[(Variant, Option<&str>, &str)]) -> Vec<Response> {
    let n = items.len().max(1) as f32;
    let w = ((ui.available_width() - t.gap * (n - 1.0)) / n).max(0.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.gap;
        items.iter().map(|(variant, icon, text)| action_sized(ui, t, *variant, *icon, text, Some(w))).collect()
    })
    .inner
}

/// A button as wide as where it sits that confirms once after being held, then briefly shows a
/// completion label.
pub fn hold_button(ui: &mut Ui, t: &Theme, icon: Option<&str>, label: &str, done_label: &str, hold_secs: f32) -> bool {
    #[derive(Clone, Default)]
    struct HoldState {
        started: Option<f64>,
        released: Option<(f64, f32)>,
        progress: f32,
        done_until: Option<f64>,
        needs_release: bool,
    }

    let font = t.body();
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.fg_text);
    let done_galley = ui.painter().layout_no_wrap(done_label.to_string(), font, t.fg_text);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let size = vec2(ui.available_width().max(galley.size().x.max(done_galley.size().x) + icon_w + 2.0 * t.pad), t.control_h);
    // a drag too: egui stops treating a press as a click after max_click_duration (0.8 s), and the hold is longer
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let enabled = ui.is_enabled();
    let (now, primary, keyboard) = ui.input(|i| (i.time, i.pointer.primary_down(), i.key_down(egui::Key::Enter) || i.key_down(egui::Key::Space)));
    let held = enabled && ((resp.is_pointer_button_down_on() && primary) || (resp.has_focus() && keyboard));
    let id = resp.id;
    let mut state = ui.data(|d| d.get_temp::<HoldState>(id)).unwrap_or_default();
    let mut confirmed = false;
    if let Some(until) = state.done_until {
        state.needs_release = held;
        if now >= until {
            state.done_until = None;
            state.progress = 0.0;
        }
    } else if held && !state.needs_release {
        let started = *state.started.get_or_insert(now);
        state.released = None;
        state.progress = ((now - started) as f32 / hold_secs.max(0.001)).clamp(0.0, 1.0);
        if state.progress >= 1.0 {
            confirmed = true;
            state.started = None;
            state.done_until = Some(now + 1.2);
            state.needs_release = true;
        }
    } else {
        state.started = None;
        if !held {
            state.needs_release = false;
        }
        if state.progress > 0.0 {
            let (at, from) = *state.released.get_or_insert((now, state.progress));
            state.progress = from * (1.0 - ease(((now - at) / 0.2) as f32));
        }
    }
    if held || state.progress > 0.0 || state.done_until.is_some() {
        ui.ctx().request_repaint();
    }

    let shape = if held && t.motion != 0.0 { rect.shrink2(rect.size() * 0.01) } else { rect };
    let painter = ui.painter();
    painter.rect_filled(shape, CornerRadius::same(t.radius_sm), t.bg_control);
    let fill = Theme::mix(t.bg_control, t.warn, 0.55);
    if state.progress >= 1.0 {
        painter.rect_filled(shape, CornerRadius::same(t.radius_sm), fill);
    } else if state.progress > 0.0 {
        let radius = (t.radius_sm as f32).min(shape.width() / 2.0).min(shape.height() / 2.0);
        let amplitude = if t.motion == 0.0 { 0.0 } else { 3.0_f32.min(shape.width() * state.progress.min(1.0 - state.progress)) };
        let steps = (shape.height() * 2.0).ceil().max(1.0) as u32;
        let mut mesh = Mesh::default();
        // Each row stays inside the rounded outline, including the wavy leading edge.
        for row in 0..=steps {
            let v = row as f32 / steps as f32;
            let y = shape.top() + v * shape.height();
            let dy = (radius - (y - shape.top()).min(shape.bottom() - y)).max(0.0);
            let inset = radius - (radius * radius - dy * dy).max(0.0).sqrt();
            let left = shape.left() + inset;
            let edge = shape.left() + shape.width() * state.progress + amplitude * (std::f32::consts::TAU * (2.0 * v + state.progress)).sin();
            let right = edge.clamp(left, shape.right() - inset);
            mesh.colored_vertex(pos2(left, y), fill);
            mesh.colored_vertex(pos2(right, y), fill);
            if row > 0 {
                let i = row * 2;
                mesh.add_triangle(i - 2, i - 1, i);
                mesh.add_triangle(i - 1, i + 1, i);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    let done = state.done_until.is_some();
    let galley = if done { done_galley } else { galley };
    let color = if enabled { t.fg_text } else { t.fg_muted };
    // the icon and the words together in the middle
    let x = shape.center().x - (icon_w + galley.size().x) / 2.0;
    if let Some(icon) = icon {
        icon_at(painter, pos2(x + 7.0, shape.center().y), icon, 15.0, color);
    }
    painter.galley(pos2(x + icon_w, shape.center().y - galley.size().y / 2.0), galley, color);
    focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::selected(Role::Button, enabled, done, if done { done_label } else { label }));
    ui.data_mut(|d| d.insert_temp(id, state));
    confirmed
}

/// Pill navigation (React Bits' Pill Nav): the active pill slides to the picked tab.
/// `compact` shows only the icons of inactive tabs and lets the active one grow open; tabs
/// `off` says are empty are dimmed and can't be picked. Returns the tab the user picked this frame.
pub fn tabs<T: Copy + PartialEq>(ui: &mut Ui, t: &Theme, id: Id, current: T, items: &[(T, &str, &str)], compact: bool, off: impl Fn(T) -> bool) -> Option<T> {
    let font = t.body();
    let mut picked = None;
    let mut slots = Vec::with_capacity(items.len());
    let pad = t.pad - 2.0;
    for (i, &(value, icon, label)) in items.iter().enumerate() {
        let full = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.fg_text).size().x + 20.0 + 2.0 * pad;
        let open = if compact { ui.ctx().animate_bool_with_time(id.with(i), value == current, t.motion) } else { 1.0 };
        let icon_only = 14.0 + 2.0 * pad;
        let w = icon_only + (full - icon_only) * open;
        let dead = off(value);
        let (rect, resp) = ui.allocate_exact_size(vec2(w, t.control_h), if dead { Sense::hover() } else { Sense::click() });
        if resp.clicked() {
            picked = Some(value);
        }
        resp.widget_info(|| WidgetInfo::selected(Role::Tab, !dead, value == current, label));
        let resp = resp.on_hover_text(label);
        slots.push((value, icon, label, rect, resp, dead));
    }
    let painter = ui.painter();
    if let Some((.., rect, _, _)) = slots.iter().find(|s| s.0 == current) {
        let ctx = ui.ctx();
        let x = ctx.animate_value_with_time(id.with("x"), rect.left(), t.motion);
        let w = ctx.animate_value_with_time(id.with("w"), rect.width(), t.motion);
        painter.rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(w, rect.height())), CornerRadius::same(t.radius_sm), t.bg_control);
    }
    for (value, icon, label, rect, resp, dead) in &slots {
        let active = *value == current;
        let color = if active {
            t.fg_text
        } else if *dead {
            t.fg_muted.gamma_multiply(0.35)
        } else if resp.hovered() {
            Theme::mix(t.fg_muted, t.fg_text, 0.6)
        } else {
            t.fg_muted
        };
        let icon_color = if active { t.accent } else { color };
        icon_at(painter, pos2(rect.left() + pad + 7.0, rect.center().y), icon, 15.0, icon_color);
        if rect.width() > 30.0 {
            let label_color = color.gamma_multiply(((rect.width() - 30.0) / 30.0).min(1.0));
            painter.with_clip_rect(*rect).text(pos2(rect.left() + pad + 20.0, rect.center().y), Align2::LEFT_CENTER, *label, font.clone(), label_color);
        }
        focus_ring(ui, t, resp, t.radius_sm);
    }
    picked
}

/// A segmented switch (language, motion…). Returns the picked index. Labels too long for the
/// room (Italian's, in a narrow panel) get the small font and tighter padding.
pub fn segmented(ui: &mut Ui, t: &Theme, current: usize, labels: &[&str]) -> Option<usize> {
    segmented_with(ui, t, current, labels).0
}

/// [`segmented`], and the whole control's response (for a right-click menu on it).
pub fn segmented_with(ui: &mut Ui, t: &Theme, current: usize, labels: &[&str]) -> (Option<usize>, Response) {
    let mut picked = None;
    let measure = |font: &FontId, pad: f32| labels.iter().map(|l| ui.painter().layout_no_wrap(l.to_string(), font.clone(), t.fg_text).size().x + 2.0 * pad).collect::<Vec<f32>>();
    let (mut font, mut widths) = (t.body(), measure(&t.body(), t.pad));
    if widths.iter().sum::<f32>() + 4.0 > ui.available_width() {
        (font, widths) = (t.small(), measure(&t.small(), t.gap + 2.0));
    }
    let room = ui.available_width() - 4.0 - widths.iter().sum::<f32>();
    if !ui.layout().is_horizontal() && room > 0.0 {
        let more = room / widths.len().max(1) as f32;
        widths.iter_mut().for_each(|w| *w += more);
    }
    let (rect, whole) = ui.allocate_exact_size(vec2(widths.iter().sum::<f32>() + 4.0, t.control_h), Sense::hover());
    let id = ui.scope_id().with(("segmented", labels));
    let painter = ui.painter();
    // a quiet track, round at its ends; the choice sits on a pill a shade lighter, not a block
    // of the accent: the eye goes to the words, and to the dot beside the chosen ones
    let round = CornerRadius::same((rect.height() / 2.0) as u8);
    painter.rect_filled(rect, round, t.bg_field);
    let mut cells = Vec::with_capacity(labels.len());
    let mut x = rect.left() + 2.0;
    for (label, w) in labels.iter().zip(&widths) {
        let cell = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(*w, rect.height() - 4.0));
        cells.push((cell, ui.interact(cell, id.with(label), Sense::click())));
        x += w;
    }
    // the pill slides to the new choice like jelly: it reaches out the way it's going and
    // thins as it does, then gathers itself there
    if let Some((cell, _)) = cells.get(current) {
        let cx = ui.ctx().animate_value_with_time(id.with("x"), cell.center().x, t.motion * 1.6);
        let cw = ui.ctx().animate_value_with_time(id.with("w"), cell.width(), t.motion * 1.6);
        let ahead = cell.center().x - cx;
        let (reach, thin) = (ahead * 0.45, (ahead.abs() / rect.width().max(1.0)).min(1.0) * 0.3);
        let pill = Rect::from_min_max(pos2(cx - cw / 2.0 + reach.min(0.0), cell.top() + cell.height() * thin / 2.0), pos2(cx + cw / 2.0 + reach.max(0.0), cell.bottom() - cell.height() * thin / 2.0));
        let pill_round = CornerRadius::same((pill.height() / 2.0) as u8);
        painter.rect_filled(pill, pill_round, Theme::mix(t.bg_field, t.fg_text, 0.15));
        painter.rect_stroke(pill, pill_round, Stroke::new(1.0, t.fg_text.gamma_multiply(0.10)), StrokeKind::Inside);
    }
    for (i, ((cell, resp), label)) in cells.iter().zip(labels).enumerate() {
        if resp.clicked() {
            picked = Some(i);
        }
        let on = i == current;
        let lit = ui.ctx().animate_bool_with_time(id.with(("on", i)), on, t.motion);
        let h = if on { 0.0 } else { hover(ui, t, resp) };
        let color = Theme::mix(Theme::mix(t.fg_muted, t.fg_text, 0.5 * h), t.fg_text, lit);
        let words = painter.text(cell.center() + vec2(4.0 * lit, 0.0), Align2::CENTER_CENTER, *label, font.clone(), color);
        // the dot that says which
        if lit > 0.0 {
            painter.circle_filled(pos2(words.left() - 7.0, cell.center().y), 2.5 * lit, t.accent);
        }
        focus_ring(ui, t, resp, (cell.height() / 2.0) as u8);
        resp.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, on, *label));
    }
    (picked, cells.into_iter().fold(whole, |a, (_, r)| a.union(r)))
}

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

/// A full-width list row (React Bits' Animated List): hover and selection fills, focusable
/// (arrows move between rows, Enter clicks). The caller paints the content in `rect`.
pub fn row(ui: &mut Ui, t: &Theme, id: Id, selected: bool) -> Response {
    row_sized(ui, t, id, selected, t.row_h)
}

/// A [`row`] `height` tall (two lines, say).
pub fn row_sized(ui: &mut Ui, t: &Theme, id: Id, selected: bool, height: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    // a clicked row takes the keyboard: ↑ ↓ walk the list from there
    if resp.clicked() {
        resp.request_focus();
    }
    let k = ui.ctx().animate_bool_with_time(id.with("sel"), selected, t.motion);
    let alpha = appear(ui, t, id, rect);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(alpha);
    if resp.hovered() && !selected {
        painter.rect_filled(rect, CornerRadius::ZERO, Theme::mix(t.bg_panel, t.fg_text, 0.05));
    }
    if k > 0.0 {
        painter.rect_filled(rect, CornerRadius::ZERO, Theme::mix(t.bg_panel, t.select, 0.12 * k));
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), CornerRadius::ZERO, t.accent.gamma_multiply(k));
    }
    if resp.has_focus() {
        painter.rect_stroke(rect.shrink(0.5), CornerRadius::ZERO, Stroke::new(1.0, t.select), StrokeKind::Inside);
    }
    resp
}

/// How visible a list row is: rows that come into view (a list opens, you scroll) fade
/// in, lower ones a beat later, so lists cascade. Painters of the row's content use it too.
fn appear(ui: &Ui, t: &Theme, id: Id, rect: Rect) -> f32 {
    if t.motion <= 0.0 || !rect.intersects(ui.clip_rect()) {
        return 1.0;
    }
    let now = ui.input(|i| i.time);
    let pass = ui.ctx().cumulative_pass_nr();
    let key = id.with("appear");
    let at = match ui.data(|d| d.get_temp::<(u64, f64)>(key)) {
        Some((seen, at)) if seen + 1 >= pass => at,
        _ => {
            // (re)appearing (not drawn last pass): start after the rows above it
            let depth = ((rect.top() - ui.clip_rect().top()) / t.row_h).clamp(0.0, 20.0) as f64;
            now + depth * 0.014
        }
    };
    ui.data_mut(|d| d.insert_temp(key, (pass, at)));
    let k = ease(((now - at) as f32 / 0.18).clamp(0.0, 1.0));
    if k < 1.0 {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id.with("alpha"), k));
    k
}

/// The current fade of a row drawn with [`row`], for painting its content.
pub fn row_alpha(ui: &Ui, resp: &Response) -> f32 {
    ui.data(|d| d.get_temp::<f32>(resp.id.with("alpha"))).unwrap_or(1.0)
}

// ---------------------------------------------------------------------------
// Menus
// ---------------------------------------------------------------------------

/// The narrowest a menu gets.
const MENU_W: f32 = 200.0;

/// How every menu and dropdown sets its lines, the node menu's way: one row height, rows flush
/// edge to edge with the same inset, a tint (not the accent's fill) on the one chosen.
pub fn menu_rows(t: &Theme) -> egui::style::StyleModifier {
    let (row, pad, gap) = (t.row_h, t.pad, t.gap);
    let (hover, chosen, text) = (Theme::mix(t.bg_card, t.fg_text, 0.06), Theme::mix(t.bg_card, t.select, 0.18), t.fg_text);
    let rows = move |s: &mut egui::Style| {
        egui::containers::menu::menu_style(s);
        // a line is as long as its words: the menu widens, nothing wraps
        s.wrap_mode = Some(egui::TextWrapMode::Extend);
        s.spacing.menu_margin = egui::Margin::symmetric(0, gap as i8);
        s.spacing.button_padding = vec2(pad * 1.5, 0.0);
        s.spacing.interact_size.y = row;
        s.spacing.item_spacing.y = 0.0;
        let w = &mut s.visuals.widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            state.corner_radius = CornerRadius::ZERO;
        }
        for state in [&mut w.hovered, &mut w.active, &mut w.open] {
            (state.weak_bg_fill, state.bg_fill) = (hover, hover);
        }
        s.visuals.selection.bg_fill = chosen;
        s.visuals.selection.stroke = Stroke::new(1.0, text);
    };
    rows.into()
}

/// A dropdown under `r`, at least as wide as it, its lines as every menu's.
pub fn menu<'a>(t: &Theme, r: &Response) -> egui::Popup<'a> {
    egui::Popup::menu(r).style(menu_rows(t)).width(r.rect.width().max(MENU_W))
}

/// A right-click menu on `r`, its lines as every menu's.
pub fn context_menu<'a>(t: &Theme, r: &Response) -> egui::Popup<'a> {
    egui::Popup::context_menu(r).style(menu_rows(t)).width(MENU_W)
}

/// Back to the app's own widgets inside a menu: a search field, a scroll bar (whose handle takes
/// the widgets' corners) are rounded and framed as everywhere else, only the lines are square.
pub fn unmenu(ui: &mut Ui) {
    let style = ui.ctx().global_style();
    let s = ui.style_mut();
    (s.visuals.widgets, s.spacing.interact_size, s.spacing.button_padding) = (style.visuals.widgets.clone(), style.spacing.interact_size, style.spacing.button_padding);
}

/// A group's title in a menu, inset as its lines.
/// A menu line that is one of a set, or a switch: a tick in the icon column while it's on, its
/// label starting where the lines with an icon start theirs.
pub fn menu_tick(ui: &mut Ui, t: &Theme, on: bool, label: &str) -> Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(crate::icons::CHECK, 0.0, egui::TextFormat::simple(font.clone(), if on { t.accent } else { Color32::TRANSPARENT }));
    job.append(&format!("  {label}"), 0.0, egui::TextFormat::simple(font, Color32::PLACEHOLDER));
    ui.add(egui::Button::new(job))
}

pub fn menu_title(ui: &mut Ui, t: &Theme, text: &str) {
    let margin = egui::Margin { left: (t.pad * 1.5) as i8, right: t.pad as i8, top: (t.gap * 2.0) as i8, bottom: t.gap as i8 };
    Frame::new().inner_margin(margin).show(ui, |ui| caps(ui, t, text));
}

/// A divider across a menu, edge to edge.
pub fn menu_line(ui: &mut Ui, t: &Theme) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(ui.max_rect().x_range(), r.center().y, Stroke::new(1.0, t.line));
}

/// A menu's footer: what the keys do, small, under a divider.
pub fn menu_footer(ui: &mut Ui, t: &Theme, text: &str) {
    menu_line(ui, t);
    ui.add_space(t.gap / 2.0);
    Frame::new().inner_margin(egui::Margin::symmetric((t.pad * 1.5) as i8, (t.gap * 1.5) as i8)).show(ui, |ui| {
        ui.label(RichText::new(text).small().color(t.fg_muted));
    });
}

/// A list to choose one from, in a [`menu`], laid out as the node menu: the search on top
/// (typing filters, ↑ ↓ walk, Enter picks), the list (`items` are (group, label), each group
/// under its title, "" has none), what the keys do at the foot. The index picked; the menu
/// closes.
pub fn choose(ui: &mut Ui, t: &Theme, items: &[(&str, &str)], current: Option<usize>) -> Option<usize> {
    let id = ui.make_persistent_id("choose");
    let pass = ui.ctx().cumulative_pass_nr();
    // the search and the line walked to start over each time the menu opens
    let (mut query, mut at) = ui.data(|d| d.get_temp::<(u64, String, Option<usize>)>(id)).filter(|(seen, ..)| seen + 1 >= pass).map_or((String::new(), None), |(_, q, a)| (q, a));
    Frame::new().inner_margin(egui::Margin { left: t.pad as i8, right: t.pad as i8, top: 0, bottom: t.pad as i8 }).show(ui, |ui| {
        unmenu(ui);
        let r = ui.add(field(&mut query).hint_text(format!("{}  {}", crate::icons::MAGNIFYING_GLASS, tr!("Search…"))).desired_width(f32::INFINITY));
        r.request_focus();
        if r.changed() {
            at = Some(0);
        }
    });
    menu_line(ui, t);
    let (up, down, enter) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)));
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
    let shown: Vec<usize> = (0..items.len()).filter(|&i| words.iter().all(|w| format!("{} {}", items[i].0, items[i].1).to_lowercase().contains(w.as_str()))).collect();
    let last = shown.len().saturating_sub(1);
    let mut k = at.unwrap_or_else(|| shown.iter().position(|&i| Some(i) == current).unwrap_or(0)).min(last);
    if down {
        k = (k + 1).min(last);
    } else if up {
        k = k.saturating_sub(1);
    }
    if up || down || at.is_some() {
        at = Some(k);
    }
    let mut picked = enter.then(|| shown.get(k).copied()).flatten();
    let rows = menu_rows(t);
    ui.scope(|ui| {
        unmenu(ui);
        egui::ScrollArea::vertical().max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
            rows.apply(ui.style_mut());
            // each line the list's width, in a menu or not (the signal's picker is an Area)
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                let mut group = "";
                for (n, &i) in shown.iter().enumerate() {
                    let (g, label) = items[i];
                    if g != group {
                        group = g;
                        if !g.is_empty() {
                            menu_title(ui, t, g);
                        }
                    }
                    let under = ui.painter().add(Shape::Noop);
                    let r = ui.add(egui::Button::selectable(Some(i) == current, label));
                    if at == Some(n) {
                        ui.painter().set(under, Shape::rect_filled(r.rect, CornerRadius::ZERO, Theme::mix(t.bg_card, t.fg_text, 0.06)));
                        if up || down {
                            r.scroll_to_me(None);
                        }
                    }
                    if r.clicked() {
                        picked = Some(i);
                    }
                }
                if shown.is_empty() {
                    Frame::new().inner_margin(egui::Margin::symmetric((t.pad * 1.5) as i8, t.gap as i8)).show(ui, |ui| ui.label(RichText::new(tr!("Nothing found")).color(t.fg_muted)));
                }
            });
        });
    });
    menu_footer(ui, t, tr!("↑ ↓ choose · Enter picks · Esc closes"));
    ui.data_mut(|d| d.insert_temp(id, (pass, query, at)));
    if picked.is_some() {
        ui.close();
    }
    picked
}

// ---------------------------------------------------------------------------
// Light
// ---------------------------------------------------------------------------

/// React Bits' Spotlight Card: a soft light that follows the pointer across a card and
/// reaches cards nearby; the border catches it.
pub fn spotlight(ui: &Ui, t: &Theme, rect: Rect, radius: u8) {
    let Some(p) = ui.ctx().pointer_hover_pos() else { return };
    const REACH: f32 = 160.0;
    let distance = rect.distance_to_pos(p);
    if distance > REACH * 0.5 {
        return;
    }
    let strength = 1.0 - distance / (REACH * 0.5);
    // the glow is a radial texture laid over the card's own rounded shape: a clip rect would
    // leave it showing in the corners
    let glow = Rect::from_center_size(p, Vec2::splat(REACH * 2.0));
    let uv = Rect::from_min_max(((rect.min - glow.min) / glow.size()).to_pos2(), ((rect.max - glow.min) / glow.size()).to_pos2());
    let fill = t.fg_text.gamma_multiply(0.07 * strength);
    ui.painter().add(egui::epaint::RectShape::filled(rect, CornerRadius::same(radius), fill).with_texture(glow_texture(ui.ctx()), uv));
    ui.painter().rect_stroke(rect, CornerRadius::same(radius), Stroke::new(1.0, Theme::mix(t.line, t.fg_muted, 0.7 * strength)), StrokeKind::Inside);
}

/// A white disc fading to nothing at its edge (premultiplied), made once: the spotlight's glow.
fn glow_texture(ctx: &egui::Context) -> egui::TextureId {
    let key = Id::unique("velo-glow");
    if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return texture.id();
    }
    const N: usize = 64;
    let pixels: Vec<Color32> = (0..N * N)
        .map(|i| {
            let (x, y) = ((i % N) as f32 + 0.5, (i / N) as f32 + 0.5);
            let d: f32 = (vec2(x, y) - Vec2::splat(N as f32 / 2.0)).length() / (N as f32 / 2.0);
            Color32::from_white_alpha(((1.0 - d).max(0.0) * 255.0) as u8)
        })
        .collect();
    let texture = ctx.load_texture("velo-glow", egui::ColorImage { size: [N, N], source_size: Vec2::splat(N as f32), pixels }, egui::TextureOptions::LINEAR);
    let id = texture.id();
    ctx.data_mut(|d| d.insert_temp(key, texture));
    id
}

/// Keyboard focus ring (after React Bits' Star Border): it lands with a quick glow and
/// settles to a thin line.
pub fn focus_ring(ui: &Ui, t: &Theme, resp: &Response, radius: u8) {
    // only while the keyboard is what moves the focus (Tab, the arrows): a click focuses too,
    // and what was just clicked needs no ring round it
    let key = Id::unique("focus moved by keys");
    let (pressed, keyed) = ui.input(|i| {
        let keys = [egui::Key::Tab, egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::ArrowLeft, egui::Key::ArrowRight];
        (i.pointer.any_pressed(), keys.iter().any(|k| i.key_pressed(*k)))
    });
    let by_keys = if keyed { true } else { !pressed && ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false) };
    ui.data_mut(|d| d.insert_temp(key, by_keys));
    let k = ui.ctx().animate_bool_with_time(resp.id.with("focus"), resp.has_focus() && by_keys, t.motion);
    if k <= 0.0 {
        return;
    }
    let rect = resp.rect.expand(1.0);
    let painter = ui.painter();
    painter.rect_stroke(rect, CornerRadius::same(radius + 1), Stroke::new(1.5, t.select.gamma_multiply(k)), StrokeKind::Outside);
    let glow = rect.expand(2.0 + 3.0 * (1.0 - k));
    painter.rect_stroke(glow, CornerRadius::same(radius + 3), Stroke::new(3.0, t.select.gamma_multiply(0.15 * k)), StrokeKind::Outside);
}

// ---------------------------------------------------------------------------
// Modals and windows
// ---------------------------------------------------------------------------

/// A modal dialog: a title bar with a close button, padded content. Esc or a click outside
/// closes it. Returns the content's value and whether it should close.
pub fn modal<R>(ctx: &egui::Context, t: &Theme, id: &str, title: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    modal_flush(ctx, t, id, title, width, |ui| {
        Frame::new()
            .inner_margin(t.pad + 4.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(t.gap * 2.0, t.gap * 2.0);
                add(ui)
            })
            .inner
    })
}

/// A modal whose content runs edge to edge under the title bar (lists, with their scroll
/// bar on the right edge): the content pads itself. Returns what `add` returned and whether
/// it should close.
pub fn modal_flush<R>(ctx: &egui::Context, t: &Theme, id: &str, title: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let frame = Frame::new().fill(t.bg_card).stroke(Stroke::new(1.0, t.line)).corner_radius(t.radius_md).shadow(ctx.global_style().visuals.window_shadow);
    let out = egui::Modal::new(Id::unique(("modal", id))).frame(frame).show(ctx, |ui| {
        ui.set_width(width);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate); // never grow past the title bar
        region(ui);
        let close = bar_with(ui, t, t.bg_card, |ui| {
            ui.label(RichText::new(title).strong().color(t.fg_text));
            right(ui, |ui| icon_button(ui, t, crate::icons::X, tr!("Close (Esc)"), false).clicked())
        });
        (add(ui), close)
    });
    let close = out.inner.1 || out.should_close();
    (out.inner.0, close)
}

/// Frame for floating windows: content draws its own bars, so no inner margin.
pub fn window_frame(ctx: &egui::Context, t: &Theme) -> Frame {
    Frame::new().fill(t.bg_base).stroke(Stroke::new(1.0, t.line)).corner_radius(t.radius_md).shadow(ctx.global_style().visuals.window_shadow)
}

/// shadcn/ui's button variants.
#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    /// The one main action of a view: light on dark.
    Primary,
    Secondary,
    /// A border, no fill.
    Outline,
    /// Nothing until hovered.
    Ghost,
    /// Deletes or stops something.
    Destructive,
}

/// A text button in one of the [`Variant`]s, with an optional leading icon.
pub fn action(ui: &mut Ui, t: &Theme, variant: Variant, icon: Option<&str>, text: &str) -> Response {
    action_sized(ui, t, variant, icon, text, None)
}

/// An [`action`] `width` wide (its content centered), or as wide as its content.
pub fn action_sized(ui: &mut Ui, t: &Theme, variant: Variant, icon: Option<&str>, text: &str, width: Option<f32>) -> Response {
    let font = t.body();
    let text_w = ui.painter().layout_no_wrap(text.to_string(), font.clone(), t.fg_text).size().x;
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let natural = text_w + icon_w + 3.0 * t.pad;
    let (rect, resp) = ui.allocate_exact_size(vec2(width.unwrap_or(natural), t.control_h), Sense::click());
    let enabled = ui.is_enabled();
    let h = if enabled { hover(ui, t, &resp) } else { 0.0 };
    let (fill, stroke, color) = match variant {
        Variant::Primary => (Theme::mix(t.fg_text, t.bg_base, 0.1 * h), None, t.bg_base),
        Variant::Secondary => (Theme::mix(t.bg_control, t.fg_text, 0.08 * h), None, t.fg_text),
        Variant::Outline => (t.bg_control.gamma_multiply(h), Some(t.line), t.fg_text),
        Variant::Ghost => (t.bg_control.gamma_multiply(h), None, t.fg_text),
        Variant::Destructive => (Theme::mix(t.error, t.bg_base, 0.15 * h), None, t.fg_text),
    };
    let color = if enabled { color } else { color.gamma_multiply(0.5) };
    let shape = if resp.is_pointer_button_down_on() { rect.shrink(1.0) } else { rect };
    let painter = ui.painter();
    painter.rect_filled(shape, CornerRadius::same(t.radius_sm), fill);
    if let Some(stroke) = stroke {
        painter.rect_stroke(shape, CornerRadius::same(t.radius_sm), Stroke::new(1.0, stroke), StrokeKind::Inside);
    }
    let mut x = rect.left() + 1.5 * t.pad + ((rect.width() - natural) / 2.0).max(0.0);
    if let Some(icon) = icon {
        icon_at(painter, pos2(x + 7.0, rect.center().y), icon, t.text_md + 2.0, color);
        x += icon_w;
    }
    painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, text, font, color);
    focus_ring(ui, t, &resp, t.radius_sm);
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, text));
    resp
}

// ---------------------------------------------------------------------------
// Floating on the canvas (Figma UI3)
// ---------------------------------------------------------------------------

/// A panel docked edge to edge (the frame fills it, egui's separator is its one divider). The
/// canvas drawn under it takes neither its clicks nor its scrolls.
pub fn docked<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = ui.max_rect();
    // the panel measures itself by what its contents use: the whole of it (a child alone
    // takes no room, and the panel would snap back to its minimum after every resize)
    ui.expand_to_include_rect(rect);
    ui.interact(rect, ui.scope_id().with("docked"), Sense::click_and_drag());
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

/// A pill of controls floating over the canvas (Figma's toolbar, its zoom menu): placed by its
/// `pivot` corner at `at`.
pub fn pill<R>(ctx: &egui::Context, t: &Theme, id: &str, at: Pos2, pivot: Align2, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Area::new(Id::unique(("pill", id)))
        .order(egui::Order::Middle)
        .fixed_pos(at)
        .pivot(pivot)
        .show(ctx, |ui| {
            Frame::new()
                .fill(t.bg_panel)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_md)
                .shadow(ctx.global_style().visuals.popup_shadow)
                .inner_margin(t.gap)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // dividers (vsep) span the controls, not the screen
                        ui.set_max_height(t.control_h);
                        add(ui)
                    })
                    .inner
                })
                .inner
        })
        .inner
}

/// Frame for popovers: a card with a border and a soft shadow, padded.
pub fn popover_frame(ctx: &egui::Context, t: &Theme) -> Frame {
    Frame::new().fill(t.bg_card).stroke(Stroke::new(1.0, t.line)).corner_radius(t.radius_md).shadow(ctx.global_style().visuals.popup_shadow).inner_margin(t.pad + 4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitals_read_in_sentence_case_with_acronyms_kept() {
        assert_eq!(sentence("SCENES"), "Scenes");
        assert_eq!(sentence("SET UP"), "Set up");
        assert_eq!(sentence("AGENTS (MCP)"), "Agents (MCP)");
        assert_eq!(sentence("MIDI LEARN"), "MIDI learn");
        assert_eq!(sentence("CONNECT AN AI"), "Connect an AI");
        assert_eq!(sentence("SORGENTI"), "Sorgenti");
    }

    /// A docked panel keeps the width it was given: egui stores a panel's size from what its
    /// contents used, and a resize used to snap back to the minimum.
    #[test]
    fn docked_panels_keep_their_width() {
        let ctx = egui::Context::default();
        let mut width = 0.0;
        for _ in 0..3 {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                let r = egui::Panel::right("docked-test").resizable(true).default_size(300.0).size_range(100.0..=500.0).show(ui, |ui| docked(ui, |ui| ui.label("x")));
                width = r.response.rect.width();
            });
            out.textures_delta.clear(); // no renderer here
        }
        assert!((width - 300.0).abs() < 1.0, "{width}");
    }

    /// A hold longer than egui's max_click_duration (0.8 s) still confirms: past it, egui stops
    /// counting a press on a click-only widget as being on it.
    #[test]
    fn a_long_hold_confirms() {
        let ctx = egui::Context::default();
        let t = Theme::dark();
        let frame = |time: f64, events: Vec<egui::Event>| {
            let (mut confirmed, mut rect) = (false, Rect::NOTHING);
            let input = egui::RawInput { time: Some(time), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let r = ui.scope(|ui| hold_button(ui, &t, None, "Hold to delete all", "Deleted", 1.2));
                (confirmed, rect) = (r.inner, r.response.rect);
            });
            out.textures_delta.clear(); // no renderer here
            (confirmed, rect)
        };
        let at = frame(0.0, Vec::new()).1.center();
        let press = egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() };
        frame(0.05, vec![egui::Event::PointerMoved(at), press]);
        let confirmed = (1..=30).any(|i| frame(0.05 + i as f64 * 0.05, Vec::new()).0);
        assert!(confirmed, "held 1.5 s without confirming");
    }
}
