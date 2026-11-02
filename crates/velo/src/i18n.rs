//! The interface's languages.
//! From Ravune (bd10814), trimmed for Velo.
//! English text is the key: `tr!("Save")` is "Save" in English
//! and the catalog's line for it in any other language (English again if it has none).
//! Lookups are a hash per string drawn, on the UI thread only.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

mod it;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    English,
    Italian,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::Italian];

    /// Each language by its own name, as the picker shows it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::Italian => "Italiano",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);
static ITALIAN: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| it::STRINGS.iter().copied().collect());

/// Switches the interface language: the next frame is drawn in it.
pub fn set(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

/// `text` in the interface language. For text that isn't a literal (table entries,
/// operator labels); literals go through `tr!`.
pub fn t(text: &str) -> &str {
    lookup(text).unwrap_or(text)
}

/// The translation of `text`, if the language isn't English and the catalog has it.
pub fn lookup(text: &str) -> Option<&'static str> {
    match LANG.load(Ordering::Relaxed) {
        1 => ITALIAN.get(text).copied(),
        _ => None,
    }
}

/// A template with its `{}` filled by `args`, in order.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    for (i, part) in template.split("{}").enumerate() {
        if let Some(arg) = i.checked_sub(1).and_then(|i| args.get(i)) {
            out.push_str(&arg.to_string());
        }
        out.push_str(part);
    }
    out
}

/// `tr!("Save")` is the `&str` to show; `tr!("Deleted {} clips", n)` fills the translated
/// template (a `String`). Templates take plain `{}`: format numbers before passing them.
macro_rules! tr {
    ($text:literal) => {
        $crate::i18n::t($text)
    };
    ($text:literal, $($arg:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::t($text), &[$(&$arg as &dyn std::fmt::Display),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The string literals passed to `tr!` in a source file.
    fn tr_literals(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (at, _) in src.match_indices("tr!(\"") {
            // not the end of another macro (`include_str!`)
            if src[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let mut text = String::new();
            let mut chars = src[at + 5..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => text.push('\n'),
                        Some(other) => text.push(other),
                        None => break,
                    },
                    c => text.push(c),
                }
            }
            out.push(text);
        }
        out
    }

    fn sources(dir: &str) -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(sources(&path.to_string_lossy()));
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push((path.display().to_string(), std::fs::read_to_string(&path).unwrap()));
            }
        }
        files
    }

    #[test]
    fn fills_templates_in_order() {
        assert_eq!(fill("{} of {}", &[&1, &"two"]), "1 of two");
        assert_eq!(fill("no holes", &[&1]), "no holes");
        assert_eq!(fill("{} and {}", &[&1]), "1 and ");
    }

    #[test]
    fn italian_covers_the_interface() {
        let mut seen = HashMap::new();
        for (en, it) in it::STRINGS {
            assert!(seen.insert(*en, *it).is_none(), "{en:?} is in the Italian catalog twice");
            assert_eq!(en.matches("{}").count(), it.matches("{}").count(), "{en:?} and {it:?} fill a different number of {{}}");
            assert!(!it.trim().is_empty(), "{en:?} has an empty translation");
        }
        // the interface's sources, not this module and its catalogs
        let ui: Vec<(String, String)> = sources("src").into_iter().filter(|(file, _)| !file.contains("i18n")).collect();
        // every literal the interface translates…
        let mut missing: Vec<String> = Vec::new();
        for (file, src) in &ui {
            missing.extend(tr_literals(src).into_iter().filter(|s| !seen.contains_key(s.as_str())).map(|s| format!("{file}: {s:?}")));
        }
        // …and the tables it translates with `t`
        let ops = velo_core::OPERATORS.iter().flat_map(|o| [o.label, o.description]);
        let commands = crate::keymap::COMMANDS.iter().flat_map(|c| [c.label, c.group]);
        let fixed = crate::keymap::FIXED.iter().flat_map(|(group, keys)| std::iter::once(*group).chain(keys.iter().map(|k| k.1)));
        let params = velo_core::CANVAS.iter().map(|p| p.name).chain([velo_core::LAUNCH_BEATS.name, "On", "Off"]);
        let nodes = velo_core::NODE_KINDS.iter().flat_map(|k| [k.label, k.group()]).chain(velo_core::Category::ALL.map(velo_core::Category::label));
        let tables = ops.chain(commands).chain(fixed).chain(params).chain(nodes);
        missing.extend(tables.filter(|s| !seen.contains_key(s)).map(|s| format!("table: {s:?}")));
        assert!(missing.is_empty(), "missing Italian for:\n{}", missing.join("\n"));
        // no line is left over from text that's gone
        let mut all: String = ui.into_iter().chain(sources("../velo-core/src")).map(|(_, s)| s).collect();
        // and the built-in nodes' parameters as the inspector names them, with their choices
        let kinds = velo_engine::BUILTINS.iter().map(|(id, _)| format!("velo.{id}")).chain(velo_core::NODE_KINDS.iter().filter(|k| k.category == velo_core::Category::Signal).map(|k| k.id.to_string()));
        for shader in kinds.filter_map(|kind| crate::live::shader(&kind, None)) {
            for (param, infos) in &shader.controls {
                all.extend(infos.iter().map(|i| format!("{:?}", i.name)).chain(param.names.iter().map(|n| format!("{n:?}"))));
            }
        }
        // and the presets a Shader node starts from, their parameters as its inspector names them
        for (_, source) in velo_engine::PRESETS.iter().chain(&velo_engine::EFFECT_PRESETS) {
            for (param, infos) in crate::live::shader(velo_core::SHADER, Some(source)).map(|s| s.controls.as_slice()).unwrap_or_default() {
                all.extend(infos.iter().map(|i| format!("{:?}", i.name)).chain(param.names.iter().map(|n| format!("{n:?}"))));
            }
        }
        let stale: Vec<&&str> = seen.keys().filter(|en| !all.contains(&format!("{en:?}"))).collect();
        assert!(stale.is_empty(), "Italian lines for text the app no longer has: {stale:?}");
    }
}
