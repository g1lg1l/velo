//! Theme tokens: the only place colors, fonts and sizes are defined.
//! From Ravune (bd10814), trimmed for Velo.
//! CI fails if `Color32::` shows up in any other file (CONTRIBUTING.md, UI rules).
//! Themes are data: three built in plus Velo Stage (a switch of its own), the rest imported from `.velo-theme` files (RON, colors as
//! "#rrggbb"). A file missing a token takes Velo Dark's.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke, TextStyle};
use serde::{Deserialize, Serialize};

pub const THEME_EXTENSION: &str = "velo-theme";

/// Colors as "#rrggbb" in theme files.
mod hex {
    use eframe::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(c: &Color32, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color32, D::Error> {
        let s = String::deserialize(d)?;
        Color32::from_hex(&s).map_err(|_| D::Error::custom(format!("{s:?} is not a #rrggbb color")))
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    /// Dark or light base for what egui draws itself (text cursor, scroll bars).
    pub dark: bool,
    /// Window background behind everything.
    #[serde(with = "hex")]
    pub bg_base: Color32,
    /// Bars and side panels.
    #[serde(with = "hex")]
    pub bg_panel: Color32,
    /// Cards, dialogs, popovers.
    #[serde(with = "hex")]
    pub bg_card: Color32,
    /// Buttons, knob bodies, hover fills.
    #[serde(with = "hex")]
    pub bg_control: Color32,
    /// Recessed fields: knob tracks, text inputs.
    #[serde(with = "hex")]
    pub bg_field: Color32,
    /// 1-px borders and dividers (there are no shadows on controls).
    #[serde(with = "hex")]
    pub line: Color32,
    #[serde(with = "hex")]
    pub fg_text: Color32,
    #[serde(with = "hex")]
    pub fg_muted: Color32,
    /// The one accent: values, active toggles, the active scene.
    #[serde(with = "hex")]
    pub accent: Color32,
    /// Selection and the keyboard focus ring.
    #[serde(with = "hex")]
    pub select: Color32,
    /// Status colors, only for state.
    #[serde(with = "hex")]
    pub warn: Color32,
    #[serde(with = "hex")]
    pub error: Color32,
    /// The toasts' fill: a dark blob in every theme (Sileo's look), so the tones read on it.
    #[serde(with = "hex")]
    pub toast: Color32,
    /// Node categories on the canvas: the dot on a node, the tint of its wires.
    #[serde(with = "hex")]
    pub cat_generator: Color32,
    #[serde(with = "hex")]
    pub cat_effect: Color32,
    #[serde(with = "hex")]
    pub cat_composite: Color32,
    pub cat_control: Color32,
    #[serde(with = "hex")]
    pub cat_output: Color32,
    /// Signal nodes (an LFO) and their wires to what they move.
    #[serde(with = "hex")]
    pub cat_signal: Color32,
    /// Controls.
    pub radius_sm: u8,
    /// Cards.
    pub radius_md: u8,
    /// Padding inside containers (4-px grid). There are no outer margins anywhere: regions
    /// touch and are split by 1-px dividers, and space lives inside elements.
    pub pad: f32,
    /// Space between siblings in a row or column.
    pub gap: f32,
    /// Header and tool bars.
    pub bar_h: f32,
    /// List rows (palette, menus, theme list).
    pub row_h: f32,
    /// Buttons, tabs, segmented switches, fields.
    pub control_h: f32,
    /// Type scale. Readouts (every number) use `text_mono` in JetBrains Mono.
    pub text_xs: f32,
    pub text_sm: f32,
    pub text_md: f32,
    pub text_lg: f32,
    pub text_mono: f32,
    /// Seconds for hover / selection / tab transitions; 0 turns motion off.
    pub motion: f32,
    /// How bright the pictures in the UI (previews, scene cards, node viewers) and the fixed
    /// colors that aren't tokens (the TE caps, the toast tones) are drawn, 1 as they are. The
    /// output itself is never dimmed.
    pub brightness: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// `Color32::from_rgb` for a 0xrrggbb literal.
const fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Teenage Engineering's colors: the OP-1's encoder caps (blue, green, white, orange; its white
/// the theme's text, so it reads on a light theme too), the tape's orange on the Audio In card.
pub const TE_BLUE: Color32 = rgb(0x4c8aff);
pub const TE_GREEN: Color32 = rgb(0x3cc882);
pub const TE_ORANGE: Color32 = rgb(0xf47860);
/// What's printed on a lit cap or key.
pub const TE_INK: Color32 = rgb(0x181818);

impl Theme {
    /// Images drawn as they are (the brand icon): the painter's "tint" that changes nothing.
    pub const NO_TINT: Color32 = Color32::WHITE;
    /// Toast tones, Sileo's bright palette (the toast is dark in every theme).
    pub const TONE_DONE: Color32 = rgb(0x22c55e);
    pub const TONE_ACTION: Color32 = rgb(0x3b82f6);
    pub const TONE_ERROR: Color32 = rgb(0xef4444);
    /// A toast's message: white at half strength.
    pub const TOAST_TEXT: Color32 = Color32::from_rgba_premultiplied(128, 128, 128, 128);

    /// The built-in themes, Velo Dark first. Velo Stage isn't among them: it has a switch of
    /// its own, so it can be flipped on before a set and off again to the theme picked here.
    pub fn builtins() -> [Theme; 3] {
        [Self::dark(), Self::oled(), Self::light()]
    }

    /// Ravune's neutral dark greys (the user's pick over pure black, 2026-10-29), 1-px borders,
    /// one electric cyan.
    pub fn dark() -> Self {
        Self {
            name: "Velo Dark".into(),
            dark: true,
            bg_base: rgb(0x171717),
            bg_panel: rgb(0x212121),
            bg_card: rgb(0x2a2a2a),
            bg_control: rgb(0x3a3a3a),
            bg_field: rgb(0x121212),
            line: rgb(0x383838),
            fg_text: rgb(0xd6d6d6),
            // 4.7:1 on the cards (#2a2a2a), as small text needs
            fg_muted: rgb(0x969696),
            accent: rgb(0x22d3ee),
            select: rgb(0xc8c8c8),
            warn: rgb(0xf59e0b),
            error: rgb(0xef4444),
            toast: rgb(0x0a0a0a),
            // Tailwind 400s a quarter-turn apart on the wheel, clear of the cyan accent
            cat_generator: rgb(0xa78bfa),
            cat_effect: rgb(0x34d399),
            cat_composite: rgb(0xfbbf24),
            cat_control: rgb(0x60a5fa),
            cat_output: rgb(0xf472b6),
            cat_signal: rgb(0xa3e635),
            radius_sm: 6,
            radius_md: 10,
            pad: 8.0,
            gap: 4.0,
            bar_h: 36.0,
            row_h: 28.0,
            control_h: 28.0,
            text_xs: 10.0,
            text_sm: 11.0,
            text_md: 13.0,
            text_lg: 15.0,
            text_mono: 12.0,
            motion: 0.16,
            brightness: 1.0,
        }
    }

    /// Pure black for OLED screens and the darkest rooms: zinc greys on #000.
    pub fn oled() -> Self {
        Self {
            name: "Velo OLED".into(),
            bg_base: rgb(0x000000),
            bg_panel: rgb(0x000000),
            bg_card: rgb(0x111113),
            bg_control: rgb(0x222225),
            bg_field: rgb(0x0a0a0b),
            line: rgb(0x1f1f22),
            fg_text: rgb(0xfafafa),
            fg_muted: rgb(0xa1a1aa),
            select: rgb(0xd4d4d8),
            toast: rgb(0x18181b),
            ..Self::dark()
        }
    }

    /// The same greys inverted, the cyan darkened to read on white.
    pub fn light() -> Self {
        Self {
            name: "Velo Light".into(),
            dark: false,
            bg_base: rgb(0xffffff),
            bg_panel: rgb(0xffffff),
            bg_card: rgb(0xfafafa),
            bg_control: rgb(0xf4f4f5),
            bg_field: rgb(0xf4f4f5),
            line: rgb(0xe4e4e7),
            fg_text: rgb(0x09090b),
            fg_muted: rgb(0x71717a),
            accent: rgb(0x0891b2),
            select: rgb(0x18181b),
            warn: rgb(0xd97706),
            error: rgb(0xdc2626),
            toast: rgb(0x18181b),
            cat_generator: rgb(0x7c3aed),
            cat_effect: rgb(0x059669),
            cat_composite: rgb(0xd97706),
            cat_control: rgb(0x2563eb),
            cat_output: rgb(0xdb2777),
            cat_signal: rgb(0x65a30d),
            ..Self::dark()
        }
    }

    /// For a dark club: near-black panels, dim warm greys for text (never white), one amber
    /// accent (amber and red spoil night vision least), warnings yellower and errors redder so
    /// LIVE and danger stay apart, and the pictures in the UI at 65%, so the laptop doesn't
    /// light up the performer's face.
    pub fn stage() -> Self {
        Self {
            name: "Velo Stage".into(),
            bg_base: rgb(0x050505),
            bg_panel: rgb(0x080808),
            bg_card: rgb(0x0e0d0c),
            bg_control: rgb(0x1a1817),
            bg_field: rgb(0x030303),
            line: rgb(0x1c1a19),
            // 4.5:1 for small text on the cards, the least the muted grey can be
            fg_text: rgb(0x9c968f),
            fg_muted: rgb(0x837d77),
            accent: rgb(0xc27a38),
            select: rgb(0xa8a199),
            warn: rgb(0xb3a03c),
            error: rgb(0xcc4a40),
            toast: rgb(0x000000),
            // Velo Dark's categories, darker and greyer
            cat_generator: rgb(0x7e6eaa),
            cat_effect: rgb(0x4a9a7a),
            cat_composite: rgb(0xa08a48),
            cat_control: rgb(0x5a7fa8),
            cat_output: rgb(0xa8628a),
            cat_signal: rgb(0x82a048),
            brightness: 0.65,
            ..Self::dark()
        }
    }

    /// The painter's tint for a picture in the UI, at the theme's brightness.
    pub fn picture_tint(&self) -> Color32 {
        Color32::from_gray((255.0 * self.brightness.clamp(0.0, 1.0)).round() as u8)
    }

    /// One of the fixed colors (a TE cap, a toast tone) at the theme's brightness.
    pub fn lit(&self, c: Color32) -> Color32 {
        let k = self.brightness.clamp(0.0, 1.0);
        let dim = |v: u8| (f32::from(v) * k).round() as u8;
        Color32::from_rgba_premultiplied(dim(c.r()), dim(c.g()), dim(c.b()), c.a())
    }

    /// A few colors that show what a theme looks like (theme lists).
    pub fn swatches(&self) -> [Color32; 5] {
        [self.bg_base, self.bg_card, self.fg_text, self.accent, self.select]
    }

    /// A node category's color.
    pub fn category(&self, c: velo_core::Category) -> Color32 {
        use velo_core::Category::*;
        match c {
            Generator => self.cat_generator,
            Effect => self.cat_effect,
            Composite => self.cat_composite,
            Control => self.cat_control,
            Output => self.cat_output,
            Signal => self.cat_signal,
        }
    }

    /// Mixes two token colors (`t` = 0 → a, 1 → b).
    pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
        a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
    }

    /// Monospace for numbers, values and readouts.
    pub fn mono(size: f32) -> FontId {
        FontId::monospace(size)
    }

    pub fn body(&self) -> FontId {
        FontId::proportional(self.text_md)
    }

    pub fn small(&self) -> FontId {
        FontId::proportional(self.text_sm)
    }

    pub fn readout(&self) -> FontId {
        FontId::monospace(self.text_mono)
    }

    /// Generates egui's visuals and style from our tokens.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.bg_panel;
        v.window_fill = self.bg_card;
        v.faint_bg_color = self.bg_card;
        v.extreme_bg_color = self.bg_field;
        v.code_bg_color = self.bg_field;
        v.hyperlink_color = self.accent;
        v.warn_fg_color = self.warn;
        v.error_fg_color = self.error;
        v.window_stroke = Stroke::new(1.0, self.line);
        v.window_corner_radius = CornerRadius::same(self.radius_md);
        v.menu_corner_radius = CornerRadius::same(self.radius_md);
        let shade = if self.dark { 120 } else { 30 };
        v.window_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(shade) };
        v.popup_shadow = Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(shade / 5 * 4) };
        // Active toggles: the accent with dark text, like a lit hardware button.
        v.selection.bg_fill = self.accent;
        v.selection.stroke = Stroke::new(1.0, self.bg_base);
        let hover = Self::mix(self.bg_control, self.fg_text, 0.1);
        let w = &mut v.widgets;
        for (state, fill, stroke, text) in [
            (&mut w.noninteractive, self.bg_card, self.line, self.fg_muted),
            (&mut w.inactive, self.bg_control, self.bg_control, self.fg_text),
            (&mut w.hovered, hover, Self::mix(self.line, self.fg_text, 0.3), self.fg_text),
            (&mut w.active, Self::mix(self.bg_control, self.accent, 0.4), self.accent, self.fg_text),
            (&mut w.open, self.bg_control, self.line, self.fg_text),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::new(1.0, stroke);
            state.fg_stroke = Stroke::new(1.0, text);
            state.corner_radius = CornerRadius::same(self.radius_sm);
            state.expansion = 0.0;
        }
        // egui keeps a style for the OS's dark mode and one for its light mode and follows the
        // OS, which it may only learn after this runs: both get ours, or a light Windows
        // shows egui's own white widgets until the theme is applied again
        ctx.all_styles_mut(|s| {
            s.visuals = v.clone();
            s.spacing.item_spacing = egui::vec2(self.gap, self.gap);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.interact_size.y = self.control_h - 6.0;
            s.spacing.menu_margin = egui::Margin::same(4);
            // scroll bars sit flush on the region's edge (the divider), no gaps around them
            s.spacing.scroll = egui::style::ScrollStyle { bar_width: 8.0, handle_min_length: 24.0, bar_inner_margin: 0.0, bar_outer_margin: 0.0, content_margin: egui::Margin::ZERO, ..egui::style::ScrollStyle::solid() };
            s.animation_time = self.motion;
            s.text_styles = [(TextStyle::Heading, FontId::proportional(self.text_lg)), (TextStyle::Body, self.body()), (TextStyle::Button, self.body()), (TextStyle::Small, self.small()), (TextStyle::Monospace, self.readout())].into();
        });
    }
}

/// Inter for UI text, JetBrains Mono for values and readouts (both SIL OFL), Phosphor for
/// icons (MIT). All three are pinned to the one weight we draw and subset to the glyphs we
/// use (~150 KB together); egui's own fallback fonts are left out. See assets/fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let font = |bytes: &'static [u8]| Arc::new(FontData::from_static(bytes));
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("inter".into(), font(include_bytes!("../assets/fonts/Inter.ttf")));
    fonts.font_data.insert("jbmono".into(), font(include_bytes!("../assets/fonts/JetBrainsMono.ttf")));
    // Phosphor icons (MIT) sit in the Private Use Area. They go first: Inter has a few
    // private glyphs of its own there, and Phosphor has nothing else to shadow.
    fonts.font_data.insert("phosphor".into(), font(include_bytes!("../assets/fonts/Phosphor.ttf")));
    // Noto Emoji (OFL, monochrome): the icons people pick for nodes and scenes, last
    fonts.font_data.insert("emoji".into(), font(include_bytes!("../assets/fonts/NotoEmoji.ttf")));
    for (family, text) in [(FontFamily::Proportional, "inter"), (FontFamily::Monospace, "jbmono")] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "phosphor".into());
        list.insert(1, text.into());
        list.push("emoji".into());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_round_trip_through_files() {
        for theme in Theme::builtins().into_iter().chain([Theme::stage()]) {
            let text = ron::ser::to_string_pretty(&theme, ron::ser::PrettyConfig::default()).unwrap();
            assert!(text.contains("accent: \"#"), "colors are written as hex");
            assert!(ron::from_str::<Theme>(&text).unwrap() == theme, "{}", theme.name);
        }
        // a file with only a few tokens takes the rest from Velo Dark
        let partial: Theme = ron::from_str(r##"(name: "Mine", accent: "#00ff00")"##).unwrap();
        assert_eq!(partial.accent, rgb(0x00ff00));
        assert_eq!(partial.bg_base, Theme::dark().bg_base);
        assert!(ron::from_str::<Theme>(r#"(accent: "green")"#).is_err());
    }

    /// WCAG's contrast ratio between two colors (1 to 21).
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let lin = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_theme_keeps_its_text_readable() {
        for t in Theme::builtins().into_iter().chain([Theme::stage()]) {
            for bg in [t.bg_base, t.bg_panel, t.bg_card, t.bg_control, t.bg_field] {
                assert!(contrast(t.fg_text, bg) >= 4.5, "{}: text on {bg:?}", t.name);
            }
            for bg in [t.bg_panel, t.bg_card] {
                assert!(contrast(t.fg_muted, bg) >= 4.5, "{}: muted text on {bg:?}", t.name);
            }
            // state colors and the focus ring stand out from the cards
            for (what, c) in [("accent", t.accent), ("warn", t.warn), ("error", t.error), ("select", t.select)] {
                assert!(contrast(c, t.bg_card) >= 3.0, "{}: {what}", t.name);
            }
        }
    }

    #[test]
    fn the_stage_theme_stays_dim() {
        let (stage, dark) = (Theme::stage(), Theme::dark());
        assert!(contrast(stage.fg_text, dark.fg_text) > 1.5, "text dimmer than Velo Dark's");
        for c in [stage.bg_base, stage.bg_panel, stage.bg_card, stage.bg_field] {
            assert!(c.r().max(c.g()).max(c.b()) <= 0x10, "near black: {c:?}");
        }
        for c in [stage.fg_text, stage.fg_muted, stage.select, stage.accent, stage.warn, stage.error] {
            assert!(c.r().max(c.g()).max(c.b()) <= 0xd0, "nothing near white: {c:?}");
        }
        // the live accent, warnings and errors are three different hues
        let hue = |c: Color32| egui::ecolor::Hsva::from(c).h;
        let (a, w, e) = (hue(stage.accent), hue(stage.warn), hue(stage.error));
        assert!((w - a).abs() > 0.04 && (a - e).abs() > 0.04, "{a} {w} {e}");
        assert!(stage.brightness < 1.0 && dark.brightness == 1.0);
        assert_eq!(stage.picture_tint(), Color32::from_gray(166));
        assert_eq!(stage.lit(Color32::WHITE), Color32::from_gray(166));
        assert_eq!(dark.lit(TE_BLUE), TE_BLUE);
    }

    #[test]
    fn the_theme_holds_whichever_mode_the_os_is_in() {
        let ctx = egui::Context::default();
        Theme::dark().apply(&ctx);
        // the OS turns out to be light after the theme went on
        ctx.set_theme(egui::Theme::Light);
        assert_eq!(ctx.global_style().visuals.panel_fill, Theme::dark().bg_panel);
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.panel_fill, Theme::dark().bg_panel);
    }
}
