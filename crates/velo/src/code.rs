//! The code editor for Shader nodes (docs/plugins.md §2): a floating window, like Preferences,
//! with the node's WGSL in the mono font. Every keystroke is `node.set_code` (typing merges into
//! one undo step); the scene rebuilds as you type, the line that broke is tinted and its error
//! shown, and the output keeps the last code that compiled.

use std::sync::{Arc, LazyLock};

use std::ops::Range;

use eframe::egui::text::{CCursor, CCursorRange, LayoutJob};
use eframe::egui::{self, Color32, Id, RichText, Ui, pos2};
use serde_json::json;
use velo_core::{SHADER_FX, is_shader};

use crate::theme::Theme;
use crate::{App, i18n, icons, kit};

/// The preset shaders by their names (from their headers): the pictures a Shader starts from,
/// then the effects a Shader FX does (they read its input).
static PRESETS: LazyLock<Vec<(String, &'static str, bool)>> = LazyLock::new(|| {
    let named = |list: &'static [(&'static str, &'static str)], effect: bool| list.iter().map(move |(slug, code)| (velo_engine::ShaderNode::parse(code).map_or_else(|_| slug.to_string(), |n| n.name), *code, effect));
    named(&velo_engine::PRESETS, false).chain(named(&velo_engine::EFFECT_PRESETS, true)).collect()
});

impl App {
    /// The inspector's PRESET section for a Shader or Shader FX node: which of its presets it runs
    /// ("Custom" once its code is its own); picking one puts its code in, edited as any other.
    pub fn preset_section(&mut self, ui: &mut Ui, t: &Theme, node: &str) {
        let Some(n) = self.session.project.scenes.iter().find_map(|s| s.node(node)) else { return };
        let code = n.code.as_deref().unwrap_or_default();
        let effects = n.kind == SHADER_FX;
        let now = PRESETS.iter().find(|(_, c, e)| *c == code && *e == effects).map(|(name, ..)| name.as_str());
        // a name that only said the kind or the last preset follows the preset picked
        let plain = n.name == "Shader" || n.name.starts_with("Shader ") || now.is_some_and(|p| n.name == p);
        let mut picked = None;
        kit::section(ui, t, icons::SPARKLE, tr!("PRESET"), false, |ui| {
            let r = kit::select(ui, t, None, now.unwrap_or(tr!("Custom")));
            r.clone().on_hover_text(tr!("Pick one to start from, or edit the code: it becomes your own."));
            kit::menu(t, &r).show(|ui| {
                let presets: Vec<&(String, &str, bool)> = PRESETS.iter().filter(|p| p.2 == effects).collect();
                // Custom first: the template, a start for code of your own
                let items: Vec<(&str, &str)> = std::iter::once(("", tr!("Custom"))).chain(presets.iter().map(|p| ("", p.0.as_str()))).collect();
                let current = presets.iter().position(|p| now == Some(p.0.as_str())).map_or(0, |i| i + 1);
                match kit::choose(ui, t, &items, Some(current)) {
                    Some(0) => picked = Some((i18n::t(if effects { "Shader FX" } else { "Shader" }).to_string(), if effects { velo_engine::EFFECT_TEMPLATE } else { velo_engine::TEMPLATE })),
                    Some(i) => picked = Some((presets[i - 1].0.clone(), presets[i - 1].1)),
                    None => {}
                }
            });
        });
        if let Some((name, code)) = picked {
            self.run_merge("node.set_code", json!({ "node": node, "code": code }), "preset");
            if plain {
                self.run_merge("node.rename", json!({ "node": node, "name": name }), "preset");
            }
            self.session.close_merge();
        }
    }

    /// The inspector's CODE section for a Shader node: the build's error, or that it runs, and
    /// the button that opens the editor.
    pub fn code_section(&mut self, ui: &mut Ui, t: &Theme, node: &str) {
        let error = self.live.as_ref().and_then(|l| l.errors.get(node)).map(ToString::to_string);
        let isf = self.session.project.scenes.iter().find_map(|s| s.node(node)).and_then(|n| n.code.as_deref()).is_some_and(velo_engine::isf::is_isf);
        let (edit, outside) = kit::section(ui, t, icons::CODE, tr!("CODE"), false, |ui| {
            let (text, color) = match &error {
                Some(e) => (tr!("Doesn't compile, the output keeps the last code that did: {}", e), t.error),
                None if isf => (tr!("An ISF shader (GLSL), rebuilt as you type and whenever its file is saved.").to_string(), t.fg_muted),
                None => (tr!("WGSL, rebuilt as you type and whenever its file is saved.").to_string(), t.fg_muted),
            };
            ui.label(RichText::new(text).small().color(color));
            let shown = kit::actions(ui, t, &[(kit::Variant::Outline, Some(icons::PENCIL_SIMPLE), tr!("Edit code")), (kit::Variant::Outline, Some(icons::FOLDER_OPEN), tr!("Open in editor"))]);
            (shown[0].clicked(), shown[1].clone().on_hover_text(tr!("Its file in the project's shaders/ folder, in your code editor (Preferences → Code)")).clicked())
        });
        if edit {
            self.editing = Some(node.to_string());
        }
        if outside {
            self.edit_outside(node);
        }
    }

    pub fn code_editor(&mut self, ctx: &egui::Context) {
        let Some(id) = self.editing.clone() else { return };
        let Some(node) = self.session.project.scenes.iter().find_map(|s| s.node(&id)).filter(|n| is_shader(&n.kind)) else {
            self.editing = None;
            return;
        };
        let (name, mut code) = (node.name.clone(), node.code.clone().unwrap_or_default());
        let scene = self.session.project.scenes.iter().find(|s| s.node(&id).is_some()).map(|s| s.id.clone());
        let error = self.live.as_ref().and_then(|l| l.errors.get(&id)).cloned();
        let t = self.theme.clone();
        let mut close = false;
        let before = code.clone();
        egui::Window::new("Code")
            .id(Id::unique("code-editor"))
            .title_bar(false)
            .frame(kit::window_frame(ctx, &t))
            .resizable(true)
            .default_size([640.0, 480.0])
            .min_size([360.0, 220.0])
            .default_pos(pos2(ctx.content_rect().left() + 220.0, 60.0))
            .show(ctx, |ui| {
                kit::region(ui);
                kit::bar(ui, &t, |ui| {
                    ui.label(RichText::new(format!("{}  {}", icons::PENCIL_SIMPLE, tr!("{} · code", name))).strong());
                    kit::right(ui, |ui| close = kit::icon_button(ui, &t, icons::X, tr!("Close"), false).clicked());
                });
                let (status, ink) = match &error {
                    Some(e) => (e.to_string(), t.error),
                    None => (tr!("Compiled: it's in the preview.").to_string(), t.fg_muted),
                };
                egui::Frame::new().inner_margin(egui::Margin::symmetric(t.pad as i8, 6)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(status).small().color(ink)).truncate());
                });
                let broken = error.and_then(|e| e.line);
                let font = Theme::mono(t.text_mono);
                // colors by token, the line that broke tinted; long lines scroll, never wrap
                let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
                    let mut job = LayoutJob::default();
                    let mut state = Lex::Code;
                    for (i, line) in text.as_str().split_inclusive('\n').enumerate() {
                        let background = if broken == Some(i + 1) { t.error.gamma_multiply(0.25) } else { egui::Color32::TRANSPARENT };
                        for (range, syntax) in lex_line(line, &mut state) {
                            let format = egui::TextFormat { background, ..egui::TextFormat::simple(font.clone(), color(&t, syntax)) };
                            job.append(&line[range], 0.0, format);
                        }
                    }
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                egui::ScrollArea::both().id_salt("code").auto_shrink(false).show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // the gutter: wide enough for the last line's number
                        let digits = (code.split('\n').count().ilog10() + 1).max(2) as f32;
                        let gutter_w = digits * ui.fonts_mut(|f| f.glyph_width(&font, '0')) + t.pad;
                        let (gutter, _) = ui.allocate_exact_size(egui::vec2(gutter_w, 0.0), egui::Sense::hover());
                        let field = Id::unique(("code-text", &id));
                        let typed = code.clone();
                        let edit = egui::TextEdit::multiline(&mut code).id(field).code_editor().frame(egui::Frame::new().inner_margin(egui::Margin::same(t.pad as i8))).desired_width(f32::INFINITY).desired_rows(24).layouter(&mut layouter);
                        let out = edit.show(ui);
                        // a tab is four spaces, a new line keeps the indentation
                        if code != typed
                            && let Some(cursor) = out.cursor_range
                        {
                            let at;
                            (code, at) = tidy(&typed, std::mem::take(&mut code), cursor.primary.index.0);
                            let mut state = out.state.clone();
                            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
                            state.store(ui.ctx(), field);
                        }
                        let mut line = 1;
                        let mut starts = true;
                        for row in &out.galley.rows {
                            if starts {
                                let ink = if broken == Some(line) { t.error } else { t.fg_muted.gamma_multiply(0.7) };
                                let at = pos2(gutter.right() - t.pad / 2.0, out.galley_pos.y + row.rect().min.y);
                                ui.painter().text(at, egui::Align2::RIGHT_TOP, line, font.clone(), ink);
                                line += 1;
                            }
                            starts = row.ends_with_newline;
                        }
                    });
                });
            });
        if code != before {
            self.run_merge("node.set_code", json!({ "scene": scene, "node": id, "code": code }), &format!("code {id}"));
        }
        if close {
            self.editing = None;
        }
    }

    /// Preferences → Code: which outside editor opens shader files.
    pub fn prefs_code(&mut self, ui: &mut Ui, t: &Theme) {
        kit::card(ui, t, tr!("CODE"), |ui| {
            ui.label(RichText::new(tr!("Editor for shader files")).font(t.body()).color(t.fg_text));
            ui.add_space(2.0);
            kit::hint(ui, t, tr!("A Shader node's code opens in it. Empty, the system picks."));
            ui.add_space(t.gap + 2.0);
            let hint = if cfg!(target_os = "macos") { tr!("The system's choice, or an app: Visual Studio Code, Cursor, Zed…") } else { tr!("The system's choice, or a command: code, cursor, zed…") };
            ui.add(kit::field(&mut self.settings.editor).hint_text(hint).desired_width(f32::INFINITY));
            ui.add_space(t.pad);
        });
    }
}

// ---- the editor's comforts: colors, indentation --------------------------------------------

/// What a piece of WGSL is, for its color.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Syntax {
    Plain,
    Comment,
    Keyword,
    Type,
    Number,
    /// In the JSON header.
    String,
}

/// Syntax colors from the theme's own tokens, so every theme gets them.
fn color(t: &Theme, syntax: Syntax) -> Color32 {
    match syntax {
        Syntax::Plain => t.fg_text,
        Syntax::Comment => t.fg_muted,
        Syntax::Keyword => t.accent,
        Syntax::Type => t.cat_generator,
        Syntax::Number => t.cat_composite,
        Syntax::String => t.cat_effect,
    }
}

/// Where a line starts: in code, inside a `/* */` comment, or inside the `/*{ }*/` header.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lex {
    Code,
    Comment,
    Header,
}

const KEYWORDS: &[&str] = &["fn", "let", "var", "const", "override", "return", "if", "else", "for", "while", "loop", "break", "continue", "switch", "case", "default", "discard", "struct", "true", "false"];
const TYPES: &[&str] = &[
    "f32",
    "f16",
    "i32",
    "u32",
    "bool",
    "vec2",
    "vec3",
    "vec4",
    "vec2f",
    "vec3f",
    "vec4f",
    "vec2i",
    "vec3i",
    "vec4i",
    "vec2u",
    "vec3u",
    "vec4u",
    "mat2x2f",
    "mat3x3f",
    "mat4x4f",
    "mat2x2",
    "mat3x3",
    "mat4x4",
    "array",
    "texture_2d",
    "sampler",
];

/// One line of WGSL (with its newline, if any) as byte ranges and what each is; `state` carries
/// comments and the header from line to line. Whitespace goes with what's around it.
fn lex_line(line: &str, state: &mut Lex) -> Vec<(Range<usize>, Syntax)> {
    let mut out: Vec<(Range<usize>, Syntax)> = Vec::new();
    let mut push = |range: Range<usize>, syntax: Syntax| match out.last_mut() {
        Some(last) if last.1 == syntax || (syntax == Syntax::Plain && line[range.clone()].trim().is_empty()) => last.0.end = range.end,
        _ => out.push((range, syntax)),
    };
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &line[i..];
        if *state == Lex::Comment {
            let end = rest.find("*/").map_or(line.len(), |e| i + e + 2);
            push(i..end, Syntax::Comment);
            if end < line.len() || rest.contains("*/") {
                *state = Lex::Code;
            }
            i = end;
            continue;
        }
        let c = bytes[i];
        let word_end = |from: usize| from + line[from..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len() - from);
        if *state == Lex::Header && rest.starts_with("}*/") {
            push(i..i + 3, Syntax::Comment);
            *state = Lex::Code;
            i += 3;
        } else if *state == Lex::Code && rest.starts_with("/*{") {
            push(i..i + 3, Syntax::Comment);
            *state = Lex::Header;
            i += 3;
        } else if *state == Lex::Code && rest.starts_with("/*") {
            *state = Lex::Comment;
        } else if rest.starts_with("//") {
            push(i..line.len(), Syntax::Comment);
            i = line.len();
        } else if c == b'"' {
            let end = rest[1..].find('"').map_or(line.len(), |e| i + e + 2);
            push(i..end, Syntax::String);
            i = end;
        } else if c.is_ascii_digit() || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            // 1.5, 2e-3, 0x1F, 4u: digits, letters and dots, a sign right after an e
            let mut end = i + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'.' || (matches!(bytes[end], b'+' | b'-') && matches!(bytes[end - 1], b'e' | b'E'))) {
                end += 1;
            }
            push(i..end, Syntax::Number);
            i = end;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let end = word_end(i);
            let word = &line[i..end];
            let syntax = if KEYWORDS.contains(&word) {
                Syntax::Keyword
            } else if TYPES.contains(&word) {
                Syntax::Type
            } else {
                Syntax::Plain
            };
            push(i..end, syntax);
            i = end;
        } else {
            let end = i + rest.chars().next().map_or(1, char::len_utf8);
            push(i..end, Syntax::Plain);
            i = end;
        }
    }
    out
}

/// What a keystroke did, finished the way code editors do: a tab becomes four spaces, and a
/// new line starts at the indentation of the one before (a level deeper after `{`). `after`
/// is the code once typed, `cursor` its character index; returns both, tidied.
fn tidy(before: &str, after: String, cursor: usize) -> (String, usize) {
    let tabs = after.chars().take(cursor).filter(|&c| c == '\t').count();
    let (mut code, mut cursor) = (after.replace('\t', "    "), cursor + 3 * tabs);
    let typed_newline = after.chars().count() == before.chars().count() + 1 && cursor > 0 && code.chars().nth(cursor - 1) == Some('\n');
    if typed_newline {
        let at = code.char_indices().nth(cursor).map_or(code.len(), |(b, _)| b);
        let above = code[..at - 1].rsplit('\n').next().unwrap_or_default();
        let mut indent: String = above.chars().take_while(|c| *c == ' ').collect();
        if above.trim_end().ends_with('{') {
            indent.push_str("    ");
        }
        cursor += indent.chars().count();
        code.insert_str(at, &indent);
    }
    (code, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(code: &str) -> Vec<(&str, Syntax)> {
        let mut state = Lex::Code;
        code.split_inclusive('\n').flat_map(|line| lex_line(line, &mut state).into_iter().map(move |(r, s)| (line[r].trim(), s))).filter(|(text, _)| !text.is_empty()).collect()
    }

    #[test]
    fn wgsl_is_colored_and_the_header_too() {
        use Syntax::*;
        let header = "/*{\n  \"name\": \"X\", \"default\": 0.5\n}*/\n";
        assert_eq!(kinds(header), [("/*{", Comment), ("\"name\"", String), (":", Plain), ("\"X\"", String), (",", Plain), ("\"default\"", String), (":", Plain), ("0.5", Number), ("}*/", Comment)]);
        let body = "let p = vec2f(1e-3, 4u); // note\n/* a\nb */ return x;";
        assert_eq!(
            kinds(body),
            [("let", Keyword), ("p =", Plain), ("vec2f", Type), ("(", Plain), ("1e-3", Number), (",", Plain), ("4u", Number), (");", Plain), ("// note", Comment), ("/* a", Comment), ("b */", Comment), ("return", Keyword), ("x;", Plain)]
        );
        // every byte is somewhere, once, in order
        let mut state = Lex::Code;
        for line in body.split_inclusive('\n') {
            let ranges = lex_line(line, &mut state);
            assert_eq!(ranges.first().map(|r| r.0.start), Some(0));
            assert_eq!(ranges.last().map(|r| r.0.end), Some(line.len()));
            assert!(ranges.windows(2).all(|w| w[0].0.end == w[1].0.start));
        }
    }

    #[test]
    fn tabs_become_spaces_and_new_lines_keep_the_indentation() {
        assert_eq!(tidy("ab", "a\tb".into(), 2), ("a    b".into(), 5));
        // after "{": one level deeper; else the same as the line above
        let before = "fn f() {";
        assert_eq!(tidy(before, "fn f() {\n".into(), 9), ("fn f() {\n    ".into(), 13));
        let before = "    let x = 1;";
        assert_eq!(tidy(before, "    let x = 1;\n".into(), 15), ("    let x = 1;\n    ".into(), 19));
        // a paste with new lines is left as it is
        assert_eq!(tidy("", "a\nb".into(), 3), ("a\nb".into(), 3));
    }
}
