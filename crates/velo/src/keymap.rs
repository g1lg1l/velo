//! Keymaps: every command a key can run, the presets, and the user's changes on top.
//! From Ravune (bd10814), trimmed for Velo.
//! `app.*` ids are handled by the app, `canvas.*` ones by the node canvas (only while it has
//! the keyboard or the pointer); anything else is an operator id. The stage keys of
//! docs/product.md §7.6 (1-9, Space, T, B, F, L, P) are kept free for the stage. A binding on
//! Space fires when it's tapped: held, Space pans the canvas.

use std::collections::BTreeMap;

use eframe::egui::{Key, KeyboardShortcut, ModifierNames, Modifiers};
use serde::{Deserialize, Serialize};

pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
}

const fn cmd(group: &'static str, id: &'static str, label: &'static str) -> Command {
    Command { id, label, group }
}

/// In the shortcut sheet's order: the canvas first, it's where you work.
pub const COMMANDS: [Command; 53] = [
    cmd("Nodes", "canvas.add", "Add a node"),
    cmd("Nodes", "canvas.find", "Find a node"),
    cmd("Nodes", "canvas.home", "Zoom to fit"),
    cmd("Nodes", "canvas.zoom_selection", "Zoom to selection"),
    cmd("Nodes", "canvas.zoom_100", "Zoom to 100%"),
    cmd("Nodes", "canvas.zoom_in", "Zoom in"),
    cmd("Nodes", "canvas.zoom_out", "Zoom out"),
    cmd("Nodes", "canvas.select_all", "Select all nodes"),
    cmd("Nodes", "canvas.toggle", "Turn the selection off or on"),
    cmd("Nodes", "canvas.duplicate", "Duplicate the selection"),
    cmd("Nodes", "canvas.delete", "Delete the selection"),
    cmd("Nodes", "canvas.import_isf", "Import an ISF shader"),
    cmd("Stage", "stage.preview", "Show or hide the preview"),
    cmd("Stage", "stage.blackout", "Blackout"),
    cmd("Stage", "stage.freeze", "Freeze"),
    cmd("Stage", "stage.pattern", "Test pattern"),
    cmd("Stage", "stage.tap", "Tap tempo"),
    cmd("Stage", "stage.learn", "MIDI learn"),
    cmd("Stage", "stage.launch", "Launch the selected scene"),
    cmd("Stage", "stage.scene_1", "Launch scene 1"),
    cmd("Stage", "stage.scene_2", "Launch scene 2"),
    cmd("Stage", "stage.scene_3", "Launch scene 3"),
    cmd("Stage", "stage.scene_4", "Launch scene 4"),
    cmd("Stage", "stage.scene_5", "Launch scene 5"),
    cmd("Stage", "stage.scene_6", "Launch scene 6"),
    cmd("Stage", "stage.scene_7", "Launch scene 7"),
    cmd("Stage", "stage.scene_8", "Launch scene 8"),
    cmd("Stage", "stage.scene_9", "Launch scene 9"),
    cmd("View", "app.toggle_scenes", "Show or hide the scenes"),
    cmd("View", "app.toggle_inspector", "Show or hide the inspector"),
    cmd("View", "app.toggle_sources", "Show or hide the macros and sources"),
    cmd("View", "app.focus", "Focus: hide or bring back every panel"),
    cmd("View", "app.split", "Split the canvas: the picture beside the nodes, under them, or off"),
    cmd("View", "app.float_inspector", "Inspector in a window of its own, or docked"),
    cmd("View", "app.hud", "Show or hide the frame times"),
    cmd("View", "app.chat", "Show or hide the AI"),
    cmd("Project", "app.new", "New project"),
    cmd("Project", "app.open", "Open"),
    cmd("Project", "app.save", "Save"),
    cmd("Project", "app.save_as", "Save as"),
    cmd("Project", "app.import", "Import the scenes of another project"),
    cmd("Project", "app.export", "Export the project with its media, to share"),
    cmd("Scenes", "app.export_scene", "Export the scene as a project of its own"),
    cmd("Project", "app.demo", "Open the demo"),
    cmd("Edit", "edit.undo", "Undo"),
    cmd("Edit", "edit.redo", "Redo"),
    cmd("Scenes", "scene.add", "New scene"),
    cmd("Scenes", "app.rename", "Rename the scene"),
    cmd("Scenes", "scene.remove", "Delete the scene"),
    cmd("Find", "app.palette", "Command palette"),
    cmd("Find", "app.shortcuts", "Keyboard shortcuts"),
    cmd("Find", "app.preferences", "Preferences"),
    cmd("Find", "app.specimen", "Specimen board"),
];

/// Keys that aren't commands. Shown on the shortcut sheet after the commands.
pub const FIXED: [(&str, &[(&str, &str)]); 2] = [
    ("Node canvas", &[("Esc", "Deselect"), ("Shift Click", "Add to the selection"), ("Double-click", "Rename a node, or add one"), ("Space Drag", "Pan (or middle-drag, or scroll)"), ("Cmd/Ctrl Scroll", "Zoom (or pinch)")]),
    (
        "Keyboard control",
        &[("Esc", "Close a dialog"), ("Tab", "Next control (Shift: previous)"), ("Arrows", "Move between controls"), ("Enter", "Press / toggle"), ("Up Down", "Turn a focused knob (Shift: fine)"), ("Del", "Reset a focused knob")],
    ),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    #[default]
    Velo,
}

impl Preset {
    pub fn name(self) -> &'static str {
        match self {
            Preset::Velo => "Velo",
        }
    }

    /// The preset's keys for a command.
    pub fn keys(self, id: &str) -> Vec<KeyboardShortcut> {
        const C: Modifiers = Modifiers::COMMAND;
        const CS: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
        const CA: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };
        const S: Modifiers = Modifiers::SHIFT;
        const N: Modifiers = Modifiers::NONE;
        let k = |m: Modifiers, key: Key| KeyboardShortcut::new(m, key);
        match (self, id) {
            (Preset::Velo, "app.new") => vec![k(C, Key::N)],
            (Preset::Velo, "app.open") => vec![k(C, Key::O)],
            (Preset::Velo, "app.save") => vec![k(C, Key::S)],
            (Preset::Velo, "app.save_as") => vec![k(CS, Key::S)],
            (Preset::Velo, "app.import") => vec![k(CA, Key::I)],
            (Preset::Velo, "app.export") => vec![k(C, Key::E)],
            (Preset::Velo, "app.export_scene") => vec![k(CS, Key::E)],
            (Preset::Velo, "app.demo") => vec![k(CS, Key::O)],
            (Preset::Velo, "app.hud") => vec![k(CS, Key::H)],
            (Preset::Velo, "app.chat") => vec![k(C, Key::I)],
            (Preset::Velo, "edit.undo") => vec![k(C, Key::Z)],
            (Preset::Velo, "edit.redo") => vec![k(CS, Key::Z), k(C, Key::Y)],
            (Preset::Velo, "scene.add") => vec![k(CS, Key::N)],
            (Preset::Velo, "app.rename") => vec![k(N, Key::F2)],
            (Preset::Velo, "scene.remove") => vec![k(C, Key::Backspace)],
            // TouchDesigner's: Tab for the node menu, H for home; then Figma's view keys
            (Preset::Velo, "canvas.add") => vec![k(N, Key::Tab)],
            // Cmd F, and Ctrl F on a Mac too
            (Preset::Velo, "canvas.find") if cfg!(target_os = "macos") => vec![k(C, Key::F), k(Modifiers::CTRL, Key::F)],
            (Preset::Velo, "canvas.find") => vec![k(C, Key::F)],
            (Preset::Velo, "canvas.home") => vec![k(N, Key::H), k(S, Key::Num1)],
            (Preset::Velo, "canvas.zoom_selection") => vec![k(S, Key::Num2)],
            (Preset::Velo, "canvas.zoom_100") => vec![k(S, Key::Num0)],
            (Preset::Velo, "canvas.zoom_in") => vec![k(C, Key::Equals), k(C, Key::Plus)],
            (Preset::Velo, "canvas.zoom_out") => vec![k(C, Key::Minus)],
            (Preset::Velo, "canvas.select_all") => vec![k(C, Key::A)],
            (Preset::Velo, "canvas.toggle") => vec![k(N, Key::E)],
            (Preset::Velo, "canvas.duplicate") => vec![k(C, Key::D)],
            (Preset::Velo, "canvas.delete") => vec![k(N, Key::Delete), k(N, Key::Backspace)],
            (Preset::Velo, "canvas.import_isf") => vec![k(CS, Key::I)],
            // the stage keys (docs/product.md §7.6)
            (Preset::Velo, "stage.preview") => vec![k(N, Key::P)],
            (Preset::Velo, "stage.blackout") => vec![k(N, Key::B)],
            (Preset::Velo, "stage.freeze") => vec![k(N, Key::F)],
            (Preset::Velo, "stage.pattern") => vec![k(CS, Key::T)],
            (Preset::Velo, "stage.tap") => vec![k(N, Key::T)],
            (Preset::Velo, "stage.learn") => vec![k(N, Key::L)],
            (Preset::Velo, "stage.launch") => vec![k(N, Key::Space)],
            (Preset::Velo, id) if id.starts_with("stage.scene_") => {
                const DIGITS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
                let n: usize = id["stage.scene_".len()..].parse().unwrap_or(0);
                n.checked_sub(1).and_then(|i| DIGITS.get(i)).map(|&d| vec![k(N, d)]).unwrap_or_default()
            }
            // VS Code's panel keys
            (Preset::Velo, "app.toggle_scenes") => vec![k(C, Key::B)],
            (Preset::Velo, "app.toggle_inspector") => vec![k(CA, Key::B)],
            (Preset::Velo, "app.toggle_sources") => vec![k(C, Key::J)],
            // bare \ or | (the same key on Italian keyboards), and Figma's Cmd+\
            (Preset::Velo, "app.focus") => vec![k(N, Key::Backslash), k(N, Key::Pipe), k(C, Key::Backslash)],
            (Preset::Velo, "app.split") => vec![k(CS, Key::P)],
            (Preset::Velo, "app.float_inspector") => vec![k(CS, Key::B)],
            (Preset::Velo, "app.palette") => vec![k(C, Key::K)],
            (Preset::Velo, "app.shortcuts") => vec![k(N, Key::F1)],
            (Preset::Velo, "app.preferences") => vec![k(C, Key::Comma)],
            (Preset::Velo, "app.specimen") => vec![k(CS, Key::D)],
            _ => Vec::new(),
        }
    }
}

/// A preset plus the commands the user rebound (so new commands still get defaults).
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keymap {
    pub preset: Preset,
    pub changed: BTreeMap<String, Vec<KeyboardShortcut>>,
}

impl Keymap {
    pub fn keys(&self, id: &str) -> Vec<KeyboardShortcut> {
        self.changed.get(id).cloned().unwrap_or_else(|| self.preset.keys(id))
    }

    pub fn bindings(&self) -> Vec<(&'static str, KeyboardShortcut)> {
        COMMANDS.iter().flat_map(|c| self.keys(c.id).into_iter().map(move |k| (c.id, k))).collect()
    }

    /// "Ctrl K", or "" if unbound.
    pub fn label(&self, id: &str) -> String {
        self.keys(id).iter().map(text).collect::<Vec<_>>().join(" · ")
    }
}

/// "Ctrl Shift S": modifier and key names, space-separated (how the shortcut sheet draws keys).
pub fn text(k: &KeyboardShortcut) -> String {
    let names = ModifierNames { concat: " ", ..ModifierNames::NAMES };
    let mods = names.format(&k.modifiers, cfg!(target_os = "macos"));
    // "," rather than "Comma", but names where egui's symbol isn't in our font subset (arrows)
    // and Delete as keyboards print it ("Delete   Delete" in a menu otherwise)
    let symbol = k.logical_key.symbol_or_name();
    let key = match k.logical_key {
        Key::Delete => "Del",
        _ if symbol.is_ascii() => symbol,
        _ => k.logical_key.name(),
    };
    if mods.is_empty() { key.to_string() } else { format!("{mods} {key}") }
}

/// Whether a key event fires a binding: the same modifiers, but a symbol typed with Shift
/// (| on most layouts) matches its binding without it.
pub fn fires(key: Key, mods: Modifiers, binding: &KeyboardShortcut) -> bool {
    use Key::*;
    let shifted = matches!(key, Pipe | Plus | Colon | Questionmark | Exclamationmark | OpenCurlyBracket | CloseCurlyBracket) && !binding.modifiers.shift;
    binding.logical_key == key && if shifted { Modifiers { shift: false, ..mods }.matches_exact(binding.modifiers) } else { mods.matches_exact(binding.modifiers) }
}

/// The key a chord is matched on. Ctrl+Alt is AltGr on European layouts, and Shift turns the
/// digits into symbols that differ by layout ("!" or "=" for Shift+1 or Shift+0 in Italian):
/// those chords go by where the key is.
pub fn chord_key(key: Key, physical: Option<Key>, mods: Modifiers) -> Key {
    use Key::*;
    let digit = matches!(physical, Some(Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9));
    if (mods.ctrl && mods.alt) || (mods.shift && digit) { physical.unwrap_or(key) } else { key }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_have_no_clashes_and_leave_the_stage_keys_free() {
        use Key::*;
        // Tab went to the node menu (S4); the preview toggle is P now
        let stage = [Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9, Space, T, B, F, L, P];
        let map = Keymap::default();
        let all = map.bindings();
        for (i, (id, k)) in all.iter().enumerate() {
            assert!(id.starts_with("stage.") || !(k.modifiers.is_none() && stage.contains(&k.logical_key)), "{id} takes the stage key {}", text(k));
            assert!(all[i + 1..].iter().all(|(_, other)| other != k), "{} used twice", text(k));
        }
        assert!(COMMANDS.iter().all(|c| !map.keys(c.id).is_empty()), "every command has a key");
        // bare keys only work where the canvas has the keyboard: elsewhere they're typing,
        // focus moves or stage keys
        for (id, k) in &all {
            assert!(!k.modifiers.is_none() || id.starts_with("canvas.") || id.starts_with("stage.") || matches!(k.logical_key, F1 | F2 | Backslash | Pipe), "{id} takes the bare key {}", text(k));
        }
        // Figma's Shift+1 is "!" in the event on any layout: it's matched where the key is
        assert_eq!(chord_key(Exclamationmark, Some(Num1), Modifiers::SHIFT), Num1);
        assert_eq!(chord_key(Equals, Some(Num0), Modifiers::SHIFT), Num0, "Italian Shift+0");
        assert_eq!(chord_key(A, Some(Q), Modifiers::SHIFT), A, "letters follow the layout");
        // | is typed with Shift: it still fires a bare | binding, but Shift+K isn't K
        let bare = |key| KeyboardShortcut::new(Modifiers::NONE, key);
        assert!(fires(Pipe, Modifiers::SHIFT, &bare(Pipe)) && !fires(K, Modifiers::SHIFT, &bare(K)));
        assert!(!fires(Pipe, Modifiers::COMMAND | Modifiers::SHIFT, &bare(Pipe)), "other modifiers still count");
        // the user's changes win over the preset
        let mut map = Keymap::default();
        map.changed.insert("app.save".into(), vec![KeyboardShortcut::new(Modifiers::NONE, Key::F5)]);
        assert_eq!(map.label("app.save"), "F5");
        assert_eq!(map.label("app.palette"), if cfg!(target_os = "macos") { "Cmd K" } else { "Ctrl K" });
    }
}
