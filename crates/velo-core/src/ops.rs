//! Operators: the single entry point for every edit (UI, keymaps, the command palette, later
//! MIDI, scripts and agents).
//! From Ravune (bd10814), trimmed for Velo.
//! The macro and variation operators (set, store, recall, randomize) and the bindings' replace
//! rule from Ravune (03cb268), adapted for Velo.

use std::collections::BTreeMap;

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

use crate::*;

/// A named, documented action.
pub struct Operator {
    /// Stable id, e.g. `scene.add`. Used by keymaps, scripts and agents.
    pub id: &'static str,
    pub label: &'static str,
    /// Shown in the command palette, tooltips and agent tool docs.
    pub description: &'static str,
    /// Example props as JSON; `{}` means it runs without input (command palette).
    pub example: &'static str,
    /// Whether it changes the project (and gets an undo step).
    pub undo: bool,
    pub poll: fn(&Session) -> bool,
    pub exec: fn(&mut Session, Value) -> Result<(), String>,
}

pub fn find(id: &str) -> Option<&'static Operator> {
    OPERATORS.iter().find(|o| o.id == id)
}

fn props<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("bad props: {e}"))
}

fn always(_: &Session) -> bool {
    true
}

fn has_scenes(s: &Session) -> bool {
    !s.project.scenes.is_empty()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SceneRef {
    /// A scene id; none means the active scene.
    scene: Option<String>,
    name: Option<String>,
    icon: Option<String>,
}

#[derive(Deserialize)]
struct AddNode {
    kind: String,
    #[serde(default)]
    pos: [f32; 2],
    /// A name to start from ("Blur" becomes "Blur 2" if taken); the kind's label by default.
    #[serde(default)]
    name: Option<String>,
    /// Parameter values and mappings to start from (a duplicate's).
    #[serde(default)]
    params: BTreeMap<String, [f32; 4]>,
    #[serde(default)]
    mappings: Vec<Mapping>,
    /// A Shader node's WGSL.
    #[serde(default)]
    code: Option<String>,
    /// An Image node's file or folder.
    #[serde(default)]
    media: Option<String>,
    /// A Text node's words ("Text" if none).
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    scene: Option<String>,
}

/// Props of `node.set_text`.
#[derive(Deserialize)]
struct SetText {
    node: String,
    text: String,
    #[serde(default)]
    scene: Option<String>,
}

#[derive(Deserialize)]
struct SetMedia {
    node: String,
    /// An image, a GIF, a movie or a folder of frames (a Text node: a font); empty for none.
    media: String,
    #[serde(default)]
    scene: Option<String>,
}

#[derive(Deserialize)]
struct SetCode {
    node: String,
    code: String,
    #[serde(default)]
    scene: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NodeRef {
    node: Option<String>,
    pos: Option<[f32; 2]>,
    /// Several nodes moved together, `by` an offset (a group drag: one snapshot per frame).
    nodes: Vec<String>,
    by: Option<[f32; 2]>,
    name: Option<String>,
    icon: Option<String>,
    on: Option<bool>,
    scene: Option<String>,
}

/// Props of `tie.add`.
#[derive(Deserialize)]
struct AddTie {
    pos: [f32; 2],
    cables: Vec<String>,
    #[serde(default)]
    scene: Option<String>,
}

/// Props of `tie.move` and `tie.remove`.
#[derive(Deserialize)]
struct TieRef {
    tie: String,
    #[serde(default)]
    pos: Option<[f32; 2]>,
    #[serde(default)]
    scene: Option<String>,
}

/// Props of `group.add`, `group.rename` and `group.remove`.
#[derive(Deserialize)]
struct GroupProps {
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    nodes: Vec<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scene: Option<String>,
}

/// Props of `note.add`, `note.set` and `note.remove`.
#[derive(Deserialize)]
struct NoteProps {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    pos: Option<[f32; 2]>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    size: Option<f32>,
    #[serde(default)]
    scene: Option<String>,
}

/// Props of `node.replace`.
#[derive(Deserialize)]
struct ReplaceNode {
    node: String,
    kind: String,
    #[serde(default)]
    params: BTreeMap<String, [f32; 4]>,
    #[serde(default)]
    mappings: Vec<Mapping>,
    /// A Shader node's WGSL (its old code, if it was one, by default).
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    scene: Option<String>,
}

/// Ports as [node id, port index].
#[derive(Deserialize)]
struct Wire {
    #[serde(default)]
    from: Option<(String, u8)>,
    to: (String, u8),
    #[serde(default)]
    scene: Option<String>,
}

fn node_index(scene: &Scene, id: &str) -> Result<usize, String> {
    scene.nodes.iter().position(|n| n.id == id).ok_or_else(|| format!("no node {id}"))
}

/// A parameter value: a number, an on/off, or up to four numbers (a color, a vec2).
#[derive(Deserialize)]
#[serde(untagged)]
enum ParamValue {
    Number(f32),
    Bool(bool),
    List(Vec<f32>),
}

#[derive(Deserialize)]
struct SetParam {
    node: String,
    param: String,
    value: ParamValue,
    #[serde(default)]
    scene: Option<String>,
}

/// A mapping, by its node, parameter and source; the rest are what `mapping.set` changes.
#[derive(Deserialize)]
struct MapRef {
    node: String,
    param: String,
    source: String,
    #[serde(default)]
    amount: Option<f32>,
    #[serde(default)]
    attack_ms: Option<f32>,
    #[serde(default)]
    release_ms: Option<f32>,
    #[serde(default)]
    invert: Option<bool>,
    #[serde(default)]
    scene: Option<String>,
}

/// The node's mappings and the index of the one `m` names, if it has it.
fn mapping<'a>(s: &'a mut Session, m: &MapRef) -> Result<(&'a mut Vec<Mapping>, Option<usize>), String> {
    let scene = s.scene_mut(m.scene.as_deref())?;
    let i = node_index(scene, &m.node)?;
    // a shared source ("kick") names its Audio In's or Beat's output
    let source = scene.signal_key(&m.source).unwrap_or_else(|| m.source.clone());
    let list = &mut scene.nodes[i].mappings;
    let at = list.iter().position(|x| x.param == m.param && x.source == source);
    Ok((list, at))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SetLayer {
    scene: Option<String>,
    layer: Option<Layer>,
    blend: Option<LayerBlend>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SetAutopilot {
    scene: Option<String>,
    on: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SetLaunch {
    scene: Option<String>,
    transition: Option<Transition>,
    duration: Option<f32>,
    quantize: Option<Quantize>,
}

/// Props of the macro and variation operators: a scene (the selected one by default), a
/// macro or variation by index, and what the operator changes.
#[derive(Deserialize, Default)]
#[serde(default)]
struct MacroRef {
    scene: Option<String>,
    index: Option<usize>,
    value: Option<f32>,
    name: Option<String>,
    node: Option<String>,
    param: Option<String>,
    min: Option<f32>,
    max: Option<f32>,
    seed: Option<u64>,
    morph_beats: Option<f32>,
}

impl MacroRef {
    fn finite(&self) -> Result<(), String> {
        if [self.value, self.min, self.max, self.morph_beats].iter().flatten().any(|x| !x.is_finite()) {
            return Err("values must be finite".into());
        }
        Ok(())
    }

    /// The scene's macro this names.
    fn target<'a>(&self, s: &'a mut Session) -> Result<&'a mut Macro, String> {
        let i = self.index.ok_or("index is required")?;
        s.scene_mut(self.scene.as_deref())?.macros.get_mut(i).ok_or_else(|| format!("no macro {i}: there are {MACROS}"))
    }

    /// The node and parameter this names.
    fn param(&self) -> Result<(String, String), String> {
        let node = self.node.clone().ok_or("node is required")?;
        let param = self.param.clone().filter(|p| !p.trim().is_empty()).ok_or("a parameter id is required")?;
        Ok((node, param))
    }
}

/// The longest morph a variation takes, in beats.
pub const MAX_MORPH_BEATS: f32 = 64.0;

#[derive(Deserialize, Default)]
#[serde(default)]
struct BindingRef {
    index: Option<usize>,
    target: Option<Target>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SetCanvas {
    width: Option<f32>,
    height: Option<f32>,
    fps: Option<f32>,
}

pub static OPERATORS: &[Operator] = &[
    // ---- scenes ----
    Operator {
        id: "scene.add",
        label: "New Scene",
        description: "Add an empty scene and select it.",
        example: "{}",
        undo: true,
        poll: always,
        exec: |s, v| {
            let SceneRef { name, .. } = props(v)?;
            let p = &mut s.project;
            let id = new_id("s_", |id| p.scene(id).is_some());
            let name = name.unwrap_or_else(|| format!("Scene {}", p.scenes.len() + 1));
            p.scenes.push(Scene { id: id.clone(), name, ..Default::default() });
            s.active_scene = Some(id);
            Ok(())
        },
    },
    Operator {
        id: "scene.duplicate",
        label: "Duplicate Scene",
        description: "Copy a scene (the selected one unless scene has an id) with its nodes, wires, mappings, macros and variations, right after it, and select the copy; name gives it a name (\"Drop 2\" by default).",
        example: "{}",
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SceneRef { scene, name, .. } = props(v)?;
            let i = s.scene_index(scene.as_deref())?;
            let copy = adopt(&s.project, s.project.scenes[i].clone(), name);
            let id = copy.id.clone();
            s.project.scenes.insert(i + 1, copy);
            s.active_scene = Some(id);
            Ok(())
        },
    },
    Operator {
        id: "scene.import",
        label: "Import Scene",
        description: "Add a scene from another project, given whole as `scene` (as a project file holds it), after the others, and select it: it gets new ids, its own shader files and a free name.",
        example: r#"{"scene": {"id": "s_ab12", "name": "Drop", "nodes": [], "edges": []}}"#,
        undo: true,
        poll: always,
        exec: |s, v| {
            let scene: Scene = serde_json::from_value(v.get("scene").cloned().ok_or("a scene is required")?).map_err(|e| format!("bad scene: {e}"))?;
            let copy = adopt(&s.project, scene, None);
            let id = copy.id.clone();
            s.project.scenes.push(copy);
            s.active_scene = Some(id);
            Ok(())
        },
    },
    Operator {
        id: "scene.rename",
        label: "Rename Scene",
        description: "Give a scene a name, e.g. \"Drop\" (the selected one unless scene has an id).",
        example: r#"{"name": "Drop"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SceneRef { scene, name, .. } = props(v)?;
            let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or("name is required")?;
            let i = s.scene_index(scene.as_deref())?;
            s.project.scenes[i].name = name;
            Ok(())
        },
    },
    Operator {
        id: "scene.set_icon",
        label: "Set Scene Icon",
        description: "Show an emoji before a scene's name, e.g. \"🔥\" (the selected one unless scene has an id); empty takes it away.",
        example: r#"{"icon": "🔥"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SceneRef { scene, icon, .. } = props(v)?;
            let i = s.scene_index(scene.as_deref())?;
            s.project.scenes[i].icon = short_icon(&icon.ok_or("icon is required")?);
            Ok(())
        },
    },
    Operator {
        id: "scene.remove",
        label: "Delete Scene",
        description: "Delete a scene (the selected one unless scene has an id); the next one gets selected.",
        example: "{}",
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SceneRef { scene, .. } = props(v)?;
            let i = s.scene_index(scene.as_deref())?;
            let scenes = &mut s.project.scenes;
            let gone = scenes.remove(i);
            if s.active_scene.as_ref() == Some(&gone.id) {
                s.active_scene = scenes.get(i.min(scenes.len().saturating_sub(1))).map(|c| c.id.clone());
            }
            Ok(())
        },
    },
    Operator {
        id: "scene.move",
        label: "Move Scene",
        description: "Put a scene (the selected one unless scene has an id) at another place in the list: to is its index after the move (0 is the first; past the end means the last). Number keys and controllers launch scenes by place, so they follow the new order.",
        example: r#"{"to": 0}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            #[derive(Deserialize)]
            struct MoveScene {
                #[serde(default)]
                scene: Option<String>,
                to: usize,
            }
            let MoveScene { scene, to } = props(v)?;
            let i = s.scene_index(scene.as_deref())?;
            let scenes = &mut s.project.scenes;
            let moved = scenes.remove(i);
            scenes.insert(to.min(scenes.len()), moved);
            Ok(())
        },
    },
    Operator {
        id: "scene.select",
        label: "Select Scene",
        description: "Show a scene in the editor, by its id.",
        example: r#"{"scene": "s_7Kq2"}"#,
        undo: false,
        poll: has_scenes,
        exec: |s, v| {
            let SceneRef { scene, .. } = props(v)?;
            let i = s.scene_index(Some(scene.as_deref().ok_or("scene is required")?))?;
            s.active_scene = Some(s.project.scenes[i].id.clone());
            Ok(())
        },
    },
    Operator {
        id: "scene.set_launch",
        label: "Set Launch",
        description: "How a scene goes out when launched (the selected one unless scene has an id): transition Cut, Crossfade, Wipe, Zoom, Glitch or Flash, duration in beats (1 to 32, every transition but Cut takes it), quantize Off, Beat, Bar or Bars4 (the boundary it waits for).",
        example: r#"{"transition": "Crossfade", "duration": 8, "quantize": "Bar"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetLaunch { scene, transition, duration, quantize } = props(v)?;
            if duration.is_some_and(|d| !d.is_finite()) {
                return Err("duration must be finite".into());
            }
            let launch = &mut s.scene_mut(scene.as_deref())?.launch;
            if let Some(t) = transition {
                launch.transition = t;
            }
            if let Some(d) = duration {
                launch.duration = LAUNCH_BEATS.clamp(d);
            }
            if let Some(q) = quantize {
                launch.quantize = q;
            }
            Ok(())
        },
    },
    Operator {
        id: "scene.set_layer",
        label: "Set Layer",
        description: "Make a scene (the selected one unless scene has an id) a layer that shows over (Top) or under (Bottom) whichever scene goes out, or a Scene launched like the others again; blend Over, Add, Screen (lights on black, the default) or Multiply says how it goes with the rest.",
        example: r#"{"layer": "Top", "blend": "Screen"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetLayer { scene, layer, blend } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            if let Some(layer) = layer {
                scene.layer = layer;
            }
            if let Some(blend) = blend {
                scene.blend = blend;
            }
            Ok(())
        },
    },
    Operator {
        id: "scene.set_autopilot",
        label: "Set Autopilot",
        description: "Whether the autopilot takes a scene (the selected one unless scene has an id) in its turns: on true or false. Layers never are.",
        example: r#"{"on": false}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetAutopilot { scene, on } = props(v)?;
            s.scene_mut(scene.as_deref())?.manual = !on;
            Ok(())
        },
    },
    // ---- nodes (the canvas selects them: selection is UI state, not an edit) ----
    Operator {
        id: "node.add",
        label: "Add Node",
        description: "Add a node of a kind (\"velo.noise\", \"velo.blur\"…) at a canvas position, in the selected scene unless scene has an id.",
        example: r#"{"kind": "velo.noise", "pos": [0, 0]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let AddNode { kind, pos, name, mut params, mappings, code, media, text, icon, scene: scene_id } = props(v)?;
            if kind == AUDIO {
                params.entry("hidden".into()).or_insert([AUDIO_HIDDEN, 0.0, 0.0, 0.0]);
            }
            let info = node_kind(&kind).ok_or_else(|| format!("unknown node kind {kind}"))?;
            let id = new_id("n_", |id| s.project.scenes.iter().any(|c| c.node(id).is_some()));
            let scene = s.scene_mut(scene_id.as_deref())?;
            let base = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| info.label.to_string());
            let taken = |name: &str| scene.nodes.iter().any(|n| n.name == name);
            let name = if taken(&base) { (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap_or(base) } else { base };
            // an ISF shader (GLSL) keeps ISF's extension, so editors know it
            let isf = code.as_deref().is_some_and(|c| c.contains("void main") && !c.contains("fn main_image"));
            let file = is_shader(&kind).then(|| shader_file(&s.project, &name, if isf { "fs" } else { "wgsl" }));
            let scene = s.scene_mut(scene_id.as_deref())?;
            let text = first_text(&kind).map(|first| text.unwrap_or(first));
            scene.nodes.push(Node { id, kind, name, off: false, pos, params, mappings, code, file, media, text, icon: short_icon(&icon) });
            Ok(())
        },
    },
    Operator {
        id: "node.move",
        label: "Move Node",
        description: "Put a node somewhere else on the canvas (its top-left corner, in canvas units), or move several nodes by an offset.",
        example: r#"{"node": "n_ab12", "pos": [240, 0]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NodeRef { node, pos, nodes, by, scene, .. } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            match (node, pos, by) {
                (Some(node), Some(pos), _) => {
                    let i = node_index(scene, &node)?;
                    scene.nodes[i].pos = pos;
                }
                (None, None, Some([dx, dy])) => {
                    for node in nodes {
                        let i = node_index(scene, &node)?;
                        let p = &mut scene.nodes[i].pos;
                        *p = [p[0] + dx, p[1] + dy];
                    }
                }
                _ => return Err("node and pos, or nodes and by, are required".into()),
            }
            Ok(())
        },
    },
    Operator {
        id: "tie.add",
        label: "Tie Cables",
        description: "Gather cables into a bundle with a tie at a point on the canvas (canvas units). Each cable by where it ends: a wire by its input (\"n_ab12/0\"), a signal's line by the node it moves and its source (\"n_cd34 n_ef56/2\").",
        example: r#"{"pos": [320, 40], "cables": ["n_ab12/0", "n_cd34/1"]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let AddTie { pos, cables, scene } = props(v)?;
            if cables.is_empty() {
                return Err("cables are required".into());
            }
            let scene = s.scene_mut(scene.as_deref())?;
            let id = new_id("t_", |id| scene.ties.iter().any(|t| t.id == id));
            scene.ties.push(Tie { id, pos, cables });
            Ok(())
        },
    },
    Operator {
        id: "tie.move",
        label: "Move Tie",
        description: "Slide a cable tie somewhere else on the canvas, its cables with it.",
        example: r#"{"tie": "t_ab12", "pos": [360, 40]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let TieRef { tie, pos, scene } = props(v)?;
            let pos = pos.ok_or("pos is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            scene.ties.iter_mut().find(|t| t.id == tie).ok_or_else(|| format!("no tie {tie}"))?.pos = pos;
            Ok(())
        },
    },
    Operator {
        id: "tie.remove",
        label: "Untie Cables",
        description: "Take a cable tie off: its cables go their own ways again.",
        example: r#"{"tie": "t_ab12"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let TieRef { tie, scene, .. } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let before = scene.ties.len();
            scene.ties.retain(|t| t.id != tie);
            if scene.ties.len() == before {
                return Err(format!("no tie {tie}"));
            }
            Ok(())
        },
    },
    Operator {
        id: "group.add",
        label: "Group Nodes",
        description: "Frame nodes together on the canvas under a name (the frame follows them; it changes nothing in the picture). A node sits in one group: it leaves the one it was in.",
        example: r#"{"nodes": ["n_ab12", "n_cd34"], "name": "Smoke"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let GroupProps { nodes, name, scene, .. } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let nodes: Vec<String> = nodes.into_iter().filter(|n| scene.node(n).is_some()).collect();
            if nodes.is_empty() {
                return Err("nodes of this scene are required".into());
            }
            for g in &mut scene.groups {
                g.nodes.retain(|n| !nodes.contains(n));
            }
            scene.groups.retain(|g| !g.nodes.is_empty());
            let id = new_id("g_", |id| scene.groups.iter().any(|g| g.id == id));
            let name = name.unwrap_or_else(|| format!("Group {}", scene.groups.len() + 1));
            scene.groups.push(Group { id, name, nodes });
            Ok(())
        },
    },
    Operator {
        id: "group.rename",
        label: "Rename Group",
        description: "Give a group of nodes another name.",
        example: r#"{"group": "g_ab12", "name": "Smoke"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let GroupProps { group, name, scene, .. } = props(v)?;
            let (group, name) = (group.ok_or("group is required")?, name.ok_or("name is required")?);
            let scene = s.scene_mut(scene.as_deref())?;
            scene.groups.iter_mut().find(|g| g.id == group).ok_or_else(|| format!("no group {group}"))?.name = name;
            Ok(())
        },
    },
    Operator {
        id: "group.remove",
        label: "Ungroup Nodes",
        description: "Take the frame off a group of nodes: the nodes stay.",
        example: r#"{"group": "g_ab12"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let GroupProps { group, scene, .. } = props(v)?;
            let group = group.ok_or("group is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            let before = scene.groups.len();
            scene.groups.retain(|g| g.id != group);
            if scene.groups.len() == before {
                return Err(format!("no group {group}"));
            }
            Ok(())
        },
    },
    Operator {
        id: "note.add",
        label: "Add Note",
        description: "Write words on the canvas at a point (canvas units): a title over a part of the graph, a note to self. size scales them (1 is a heading, 0.6 a label, up to 8 for a title read from far out).",
        example: r#"{"pos": [-400, -120], "text": "SMOKE", "size": 1}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NoteProps { pos, text, size, scene, .. } = props(v)?;
            let pos = pos.ok_or("pos is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            let id = new_id("x_", |id| scene.notes.iter().any(|n| n.id == id));
            scene.notes.push(Note { id, pos, text: text.unwrap_or_default(), size: size.unwrap_or(1.0).clamp(0.3, 8.0) });
            Ok(())
        },
    },
    Operator {
        id: "note.set",
        label: "Change Note",
        description: "Change a note on the canvas: its words, where it is, how large.",
        example: r#"{"note": "x_ab12", "text": "SMOKE", "pos": [-400, -120]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NoteProps { note, pos, text, size, scene } = props(v)?;
            let note = note.ok_or("note is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            let n = scene.notes.iter_mut().find(|n| n.id == note).ok_or_else(|| format!("no note {note}"))?;
            if let Some(pos) = pos {
                n.pos = pos;
            }
            if let Some(text) = text {
                n.text = text;
            }
            if let Some(size) = size {
                n.size = size.clamp(0.3, 8.0);
            }
            Ok(())
        },
    },
    Operator {
        id: "note.remove",
        label: "Delete Note",
        description: "Take a note off the canvas.",
        example: r#"{"note": "x_ab12"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NoteProps { note, scene, .. } = props(v)?;
            let note = note.ok_or("note is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            let before = scene.notes.len();
            scene.notes.retain(|n| n.id != note);
            if scene.notes.len() == before {
                return Err(format!("no note {note}"));
            }
            Ok(())
        },
    },
    Operator {
        id: "node.rename",
        label: "Rename Node",
        description: "Give a node a name, e.g. \"Tunnel\".",
        example: r#"{"node": "n_ab12", "name": "Tunnel"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NodeRef { node, name, scene, .. } = props(v)?;
            let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).ok_or("name is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node.ok_or("node is required")?)?;
            scene.nodes[i].name = name;
            Ok(())
        },
    },
    Operator {
        id: "node.toggle",
        label: "Turn Node On or Off",
        description: "Turn one or more nodes on or off. Without on, each node flips. Output nodes stay on.",
        example: r#"{"nodes": ["n_ab12"], "on": false}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NodeRef { nodes, on, scene, .. } = props(v)?;
            if nodes.is_empty() {
                return Err("nodes are required".into());
            }
            let scene = s.scene_mut(scene.as_deref())?;
            for id in nodes {
                let i = node_index(scene, &id)?;
                let node = &mut scene.nodes[i];
                if !node_kind(&node.kind).is_some_and(|k| k.category == Category::Output) {
                    node.off = on.map_or(!node.off, |on| !on);
                }
            }
            Ok(())
        },
    },
    Operator {
        id: "node.set_icon",
        label: "Set Node Icon",
        description: "Show an emoji before a node's name, e.g. \"🌊\"; empty takes it away.",
        example: r#"{"node": "n_ab12", "icon": "🌊"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NodeRef { node, icon, scene, .. } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node.ok_or("node is required")?)?;
            scene.nodes[i].icon = short_icon(&icon.ok_or("icon is required")?);
            Ok(())
        },
    },
    Operator {
        id: "node.remove",
        label: "Delete Node",
        description: "Delete a node and its wires.",
        example: r#"{"node": "n_ab12"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let NodeRef { node, scene, .. } = props(v)?;
            let node = node.ok_or("node is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node)?;
            scene.nodes.remove(i);
            scene.edges.retain(|e| e.from.0 != node && e.to.0 != node);
            for m in &mut scene.macros {
                m.targets.retain(|t| t.node != node);
            }
            // out of its group too, the group with it once it's empty
            for g in &mut scene.groups {
                g.nodes.retain(|n| *n != node);
            }
            scene.groups.retain(|g| !g.nodes.is_empty());
            s.project.bindings.retain(|b| !matches!(&b.target, Target::Param { node: n, .. } if *n == node));
            Ok(())
        },
    },
    Operator {
        id: "node.set_param",
        label: "Set Parameter",
        description: "Set one of a node's parameters by its id: a number, true/false, or a list of up to four numbers (a color, a vec2). Shaders clamp to their own ranges.",
        example: r#"{"node": "n_ab12", "param": "speed", "value": 0.5}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetParam { node, param, value, scene } = props(v)?;
            let mut slot = [0.0; 4];
            match value {
                ParamValue::Number(x) => slot[0] = x,
                ParamValue::Bool(b) => slot[0] = f32::from(u8::from(b)),
                ParamValue::List(list) if (1..=4).contains(&list.len()) => slot[..list.len()].copy_from_slice(&list),
                ParamValue::List(_) => return Err("value takes one to four numbers".into()),
            }
            if param.trim().is_empty() || slot.iter().any(|x| !x.is_finite()) {
                return Err("a parameter id and finite values are required".into());
            }
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node)?;
            scene.nodes[i].params.insert(param, slot);
            Ok(())
        },
    },
    Operator {
        id: "node.set_code",
        label: "Edit Shader Code",
        description: "Replace a Shader node's WGSL: a /*{ JSON header }*/ declaring name, params and inputs, then fn main_image(uv: vec2f, px: vec2f) -> vec4f (docs/plugins.md). Errors come back from the build; the output keeps the last code that compiled.",
        example: r#"{"node": "n_ab12", "code": "/*{ \"name\": \"Red\" }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(1.0, 0.0, 0.0, 1.0); }"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetCode { node, code, scene } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node)?;
            if !is_shader(&scene.nodes[i].kind) {
                return Err(format!("{node} isn't a Shader node"));
            }
            scene.nodes[i].code = Some(code);
            Ok(())
        },
    },
    Operator {
        id: "node.replace",
        label: "Replace Node",
        description: "Turn a node into another kind where it stands, e.g. a Blur into a Kaleido: params and mappings to start from (none by default), its name if it was the old kind's, and the wires that still fit; the rest of its wires, and mappings of outputs it no longer has, go.",
        example: r#"{"node": "n_ab12", "kind": "velo.kaleido"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let ReplaceNode { node, kind, params, mappings, code, scene } = props(v)?;
            let new = node_kind(&kind).ok_or_else(|| format!("unknown node kind {kind}"))?;
            let c = s.scene_index(scene.as_deref())?;
            let i = node_index(&s.project.scenes[c], &node)?;
            let old = s.project.scenes[c].nodes[i].clone();
            if old.kind == kind {
                return Ok(());
            }
            // a name that only said the old kind says the new one ("Blur 2" becomes "Kaleido")
            let said_kind = node_kind(&old.kind).is_some_and(|k| old.name == k.label || old.name.strip_prefix(k.label).is_some_and(|rest| rest.trim().parse::<u32>().is_ok()));
            let name = if said_kind {
                let taken = |name: &str| s.project.scenes[c].nodes.iter().any(|n| n.id != node && n.name == name);
                if taken(new.label) { (2..).map(|n| format!("{} {n}", new.label)).find(|n| !taken(n)).unwrap_or_else(|| new.label.into()) } else { new.label.into() }
            } else {
                old.name.clone()
            };
            let file = is_shader(&kind).then(|| old.file.clone().unwrap_or_else(|| shader_file(&s.project, &name, "wgsl")));
            let code = if is_shader(&kind) { old.code.clone().or(code) } else { None };
            let media = if shows_media(&kind) { old.media.clone() } else { None };
            let text = first_text(&kind).map(|first| old.text.clone().unwrap_or(first));
            let scene = &mut s.project.scenes[c];
            scene.nodes[i] = Node { kind, name, off: old.off && new.category != Category::Output, params, mappings, code, file, media, text, ..old };
            // the wires that still fit (each checked as if made now), the rest go
            let edges = std::mem::take(&mut scene.edges);
            for e in edges {
                let touches = e.from.0 == node || e.to.0 == node;
                if !touches || scene.can_connect((&e.from.0, e.from.1), (&e.to.0, e.to.1)).is_ok() {
                    scene.edges.push(e);
                }
            }
            // what other nodes took from its outputs, while it still has them as a signal node
            let outputs = if new.category == Category::Signal { new.outputs.len() } else { 0 };
            for n in &mut scene.nodes {
                n.mappings.retain(|m| signal_output(&m.source).is_none_or(|(id, port)| id != node || usize::from(port) < outputs));
            }
            Ok(())
        },
    },
    Operator {
        id: "node.set_media",
        label: "Set Image",
        description: "Set what an Image or Video node shows: an image file (PNG, JPEG, WebP…), an animated GIF, a movie (.mov or .mp4: HAP, and on macOS H.264, HEVC or ProRes), or a folder of numbered frames played as an image sequence, by absolute path. On a Particles node, the picture each particle is, or a 3D model (.obj or .ply) they are born on with Shape Model; on a Mesh node, the 3D model (.obj or .ply with faces) it draws; on a Media List, its files, a path a line; on a Text or Text List node, its own font (.ttf or .otf), which its Font set to File shows; on a Live node, its source, ndi:<name>, spout:<name> (Windows) or camera:<name> (Windows and macOS). Empty clears it.",
        example: r#"{"node": "n_ab12", "media": "/Users/me/Loops/tunnel.gif"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetMedia { node, media, scene } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node)?;
            if !shows_media(&scene.nodes[i].kind) && !sets_text(&scene.nodes[i].kind) && ![MEDIA_LIST, PARTICLES, MESH, LIVE].contains(&scene.nodes[i].kind.as_str()) {
                return Err(format!("{node} isn't an Image, Video, Live, Media List, Particles, Mesh, Text or Text List node"));
            }
            scene.nodes[i].media = Some(media.trim().to_string()).filter(|m| !m.is_empty());
            Ok(())
        },
    },
    Operator {
        id: "node.set_text",
        label: "Set Text",
        description: "Set the words a Text node shows; a newline starts another line. On a Text List each item starts with the record separator (U+001E) and may run over lines; words without one are an item a line.",
        example: r#"{"node": "n_ab12", "text": "VELO"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let SetText { node, text, scene } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let i = node_index(scene, &node)?;
            if !sets_text(&scene.nodes[i].kind) {
                return Err(format!("{node} isn't a Text or Text List node"));
            }
            scene.nodes[i].text = Some(text);
            Ok(())
        },
    },
    // ---- mappings (docs/graph.md §3): a source moves a parameter ----
    Operator {
        id: "mapping.add",
        label: "Map Source",
        description: "Let a signal node's output move one of a node's parameters, at half its range with 5 ms attack and 150 ms release. The source is \"node_id/output\" (an Audio In's 0 level to 6 hat and 7 melody, a Beat's 0 beat and 1 bar, any other signal node's 0), or level, bass, mid, high, kick, snare, hat, melody, beat, bar_phase: the scene's Audio In or Beat output of that name, the node added if the scene has none. Mapping the same source twice does nothing.",
        example: r#"{"node": "n_ab12", "param": "zoom", "source": "kick"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let mut m: MapRef = props(v)?;
            if m.param.trim().is_empty() {
                return Err("a parameter id is required".into());
            }
            let c = s.scene_index(m.scene.as_deref())?;
            node_index(&s.project.scenes[c], &m.node)?;
            // a shared source ("kick") comes from the scene's Audio In or Beat, added if missing
            if let Some(key) = signal_source(&mut s.project, c, &m.source) {
                m.source = key;
            }
            // a signal node's output: the node in this scene, the output one it has
            let signal = signal_output(&m.source).is_some_and(|(id, port)| s.project.scenes[c].node(id).and_then(|n| node_kind(&n.kind)).is_some_and(|k| k.category == Category::Signal && usize::from(port) < k.outputs.len()));
            if !signal {
                return Err(format!("unknown source {}", m.source));
            }
            let (list, at) = mapping(s, &m)?;
            if at.is_none() {
                list.push(Mapping { param: m.param, source: m.source, amount: Mapping::AMOUNT, attack_ms: Mapping::ATTACK_MS, release_ms: Mapping::RELEASE_MS, invert: false });
            }
            Ok(())
        },
    },
    Operator {
        id: "mapping.set",
        label: "Change Mapping",
        description: "Change how a source moves a parameter: amount (-1..1 of the parameter's range), attack_ms and release_ms (smoothing), invert.",
        example: r#"{"node": "n_ab12", "param": "zoom", "source": "kick", "amount": -0.25}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let m: MapRef = props(v)?;
            let numbers = [m.amount, m.attack_ms, m.release_ms];
            if numbers.iter().flatten().any(|x| !x.is_finite()) {
                return Err("values must be finite".into());
            }
            let (list, at) = mapping(s, &m)?;
            let x = &mut list[at.ok_or_else(|| format!("{} isn't mapped to {}", m.source, m.param))?];
            if let Some(a) = m.amount {
                x.amount = a.clamp(-1.0, 1.0);
            }
            if let Some(ms) = m.attack_ms {
                x.attack_ms = ms.clamp(0.0, Mapping::MAX_MS);
            }
            if let Some(ms) = m.release_ms {
                x.release_ms = ms.clamp(0.0, Mapping::MAX_MS);
            }
            if let Some(i) = m.invert {
                x.invert = i;
            }
            Ok(())
        },
    },
    Operator {
        id: "mapping.remove",
        label: "Remove Mapping",
        description: "Stop a source moving a parameter: it goes back to its base value.",
        example: r#"{"node": "n_ab12", "param": "zoom", "source": "kick"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let m: MapRef = props(v)?;
            let (list, at) = mapping(s, &m)?;
            list.remove(at.ok_or_else(|| format!("{} isn't mapped to {}", m.source, m.param))?);
            Ok(())
        },
    },
    Operator {
        id: "edge.connect",
        label: "Connect",
        description: "Wire an output into an input, each as [node, port]; the input's old wire goes. Loops only through a Feedback node.",
        example: r#"{"from": ["n_ab12", 0], "to": ["n_cd34", 0]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let Wire { from, to, scene } = props(v)?;
            let from = from.ok_or("from is required")?;
            let scene = s.scene_mut(scene.as_deref())?;
            scene.can_connect((&from.0, from.1), (&to.0, to.1))?;
            scene.edges.retain(|e| e.to != to);
            scene.edges.push(Edge { from, to });
            Ok(())
        },
    },
    Operator {
        id: "edge.disconnect",
        label: "Disconnect",
        description: "Remove the wire into an input, given as [node, port].",
        example: r#"{"to": ["n_cd34", 0]}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let Wire { to, scene, .. } = props(v)?;
            let scene = s.scene_mut(scene.as_deref())?;
            let before = scene.edges.len();
            scene.edges.retain(|e| e.to != to);
            if scene.edges.len() == before { Err("nothing is wired into that input".into()) } else { Ok(()) }
        },
    },
    // ---- macros and variations (docs/scenes.md §2; Ravune's model) ----
    Operator {
        id: "macro.set",
        label: "Set Macro",
        description: "Turn one of a scene's eight macros (index 0 to 7) to a value from 0 to 1: every parameter it drives follows.",
        example: r#"{"index": 0, "value": 0.5}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            p.finite()?;
            let value = p.value.ok_or("value is required")?;
            p.target(s)?.value = value.clamp(0.0, 1.0);
            Ok(())
        },
    },
    Operator {
        id: "macro.rename",
        label: "Rename Macro",
        description: "Give a macro a name, e.g. \"Energy\" (empty: back to \"Macro n\").",
        example: r#"{"index": 0, "name": "Energy"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            let name = p.name.as_deref().ok_or("name is required")?.trim().to_string();
            p.target(s)?.name = name;
            Ok(())
        },
    },
    Operator {
        id: "macro.assign",
        label: "Assign to Macro",
        description: "Let a macro drive one of a node's parameters, from min (macro at 0) to max (macro at 1), in the parameter's units; the inspector passes the parameter's value and its maximum (left out: its stored value, else 0, and 1). Another macro driving it lets go.",
        example: r#"{"index": 0, "node": "n_ab12", "param": "zoom", "min": 1.0, "max": 4.0}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            p.finite()?;
            let (node, param) = p.param()?;
            let index = p.index.ok_or("index is required")?;
            let scene = s.scene_mut(p.scene.as_deref())?;
            let stored = {
                let n = &scene.nodes[node_index(scene, &node)?];
                let (id, axis) = match param.rsplit_once('.') {
                    Some((id, "y")) => (id, 1),
                    Some((id, "x")) => (id, 0),
                    _ => (param.as_str(), 0),
                };
                n.params.get(id).map(|v| v[axis])
            };
            if index >= MACROS {
                return Err(format!("no macro {index}: there are {MACROS}"));
            }
            for m in &mut scene.macros {
                m.targets.retain(|t| !(t.node == node && t.param == param));
            }
            let (min, max) = (p.min.or(stored).unwrap_or(0.0), p.max.unwrap_or(1.0));
            scene.macros[index].targets.push(MacroTarget { node, param, min, max });
            Ok(())
        },
    },
    Operator {
        id: "macro.unassign",
        label: "Unassign Macro",
        description: "Stop a macro driving a parameter: it goes back to its own value.",
        example: r#"{"index": 0, "node": "n_ab12", "param": "zoom"}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            let (node, param) = p.param()?;
            let m = p.target(s)?;
            let before = m.targets.len();
            m.targets.retain(|t| !(t.node == node && t.param == param));
            if m.targets.len() == before { Err(format!("the macro doesn't drive {param}")) } else { Ok(()) }
        },
    },
    Operator {
        id: "macro.target",
        label: "Set Macro Range",
        description: "Change where a macro takes a parameter: min with the macro at 0, max at 1, in the parameter's units.",
        example: r#"{"index": 0, "node": "n_ab12", "param": "zoom", "max": 2.5}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            p.finite()?;
            let (node, param) = p.param()?;
            let m = p.target(s)?;
            let t = m.targets.iter_mut().find(|t| t.node == node && t.param == param).ok_or_else(|| format!("the macro doesn't drive {param}"))?;
            t.min = p.min.unwrap_or(t.min);
            t.max = p.max.unwrap_or(t.max);
            Ok(())
        },
    },
    Operator {
        id: "macro.randomize",
        label: "Randomize Macros",
        description: "Turn every macro that drives something to a random position (again for another; a seed repeats one).",
        example: "{}",
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            let mut x = p.seed.unwrap_or_else(random_seed) | 1;
            let scene = s.scene_mut(p.scene.as_deref())?;
            let mut used = scene.macros.iter_mut().filter(|m| !m.targets.is_empty()).peekable();
            if used.peek().is_none() {
                return Err("no macro drives anything yet: right-click a parameter to assign one".into());
            }
            for m in used {
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                m.value = ((x.wrapping_mul(0x2545F4914F6CDD1D) >> 40) as f32 / (1u64 << 24) as f32 * 100.0).round() / 100.0;
            }
            Ok(())
        },
    },
    Operator {
        id: "variation.store",
        label: "Store Variation",
        description: "Keep where the scene's macros are now as a variation (A, B, C…), to recall or morph back to.",
        example: "{}",
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            let scene = s.scene_mut(p.scene.as_deref())?;
            if scene.variations.len() >= VARIATIONS {
                return Err("8 variations at most: remove one first".into());
            }
            let macros = scene.macros.each_ref().map(|m| m.value);
            scene.variations.push(Variation { name: p.name.map(|n| n.trim().to_string()).unwrap_or_default(), macros });
            Ok(())
        },
    },
    Operator {
        id: "variation.recall",
        label: "Recall Variation",
        description: "Put the scene's macros where a variation (index 0 is A) had them: at once, or gliding there over morph_beats on the beat clock.",
        example: r#"{"index": 0, "morph_beats": 4}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            p.finite()?;
            let i = p.index.ok_or("index is required")?;
            let scene = s.scene_mut(p.scene.as_deref())?;
            let values = scene.variations.get(i).ok_or("no such variation")?.macros;
            for (m, value) in scene.macros.iter_mut().zip(values) {
                m.value = value.clamp(0.0, 1.0);
            }
            let id = scene.id.clone();
            let beats = p.morph_beats.unwrap_or(0.0).clamp(0.0, MAX_MORPH_BEATS);
            s.morph = (beats > 0.0).then_some((id, beats));
            Ok(())
        },
    },
    Operator {
        id: "variation.remove",
        label: "Remove Variation",
        description: "Delete one of a scene's variations (index 0 is A): the later ones move up a letter.",
        example: r#"{"index": 0}"#,
        undo: true,
        poll: has_scenes,
        exec: |s, v| {
            let p: MacroRef = props(v)?;
            let i = p.index.ok_or("index is required")?;
            let list = &mut s.scene_mut(p.scene.as_deref())?.variations;
            if i >= list.len() {
                return Err("no such variation".into());
            }
            list.remove(i);
            Ok(())
        },
    },
    // ---- bindings (docs/control.md §2): what controllers do ----
    Operator {
        id: "binding.add",
        label: "Bind Control",
        description: "Make a controller's knob, key or pad do something: a macro, a parameter, launching a scene, a variation, blackout, tap tempo. A key of the computer's keyboard is a control too: {\"Key\": \"4\"} (a letter, a digit, F1 to F12), with Trigger or Toggle. It replaces that control's and that target's old binding.",
        example: r#"{"control": {"Midi": {"port": null, "channel": 0, "msg": {"Cc": 21}}}, "target": {"Macro": 0}, "behavior": {"Absolute": {"takeover": "Pickup"}}}"#,
        undo: true,
        poll: always,
        exec: |s, v| {
            let b: Binding = props(v)?;
            match &b.control {
                Control::Midi { channel, msg, .. } => {
                    let number = match msg {
                        MidiMsg::Cc(n) | MidiMsg::Note(n) => *n,
                        MidiMsg::ProgramChange => 0,
                        MidiMsg::Cc14(n) if *n >= 32 => return Err("a 14-bit controller is CC 0 to 31 (its fine half is CC n + 32)".into()),
                        MidiMsg::Cc14(n) => *n,
                    };
                    if *channel > 15 || number > 127 {
                        return Err("channel is 0 to 15, controllers and notes 0 to 127".into());
                    }
                }
                Control::Key(name) => {
                    if !Control::key_name(name) {
                        return Err(r#"a key is a letter ("A"), a digit ("4") or a function key ("F1" to "F12")"#.into());
                    }
                    // a key has no travel: it can only press
                    if !matches!(b.behavior, Behavior::Trigger | Behavior::Toggle) {
                        return Err("a key presses: its behavior is Trigger or Toggle".into());
                    }
                }
            }
            if matches!(b.behavior, Behavior::Relative { step, .. } if !(step > 0.0 && step <= 1.0)) {
                return Err("an encoder's step is a share of the travel, above 0 and at most 1".into());
            }
            if matches!(b.target, Target::Macro(i) if usize::from(i) >= MACROS) {
                return Err(format!("there are {MACROS} macros"));
            }
            s.project.bindings.retain(|x| x.control != b.control && x.target != b.target);
            s.project.bindings.push(b);
            Ok(())
        },
    },
    Operator {
        id: "binding.remove",
        label: "Remove Binding",
        description: "Forget a controller binding, by its index in the list or by its target.",
        example: r#"{"target": "Blackout"}"#,
        undo: true,
        poll: |s| !s.project.bindings.is_empty(),
        exec: |s, v| {
            let BindingRef { index, target } = props(v)?;
            let list = &mut s.project.bindings;
            let i = match (index, target) {
                (Some(i), _) => Some(i).filter(|&i| i < list.len()),
                (None, Some(t)) => list.iter().position(|b| b.target == t),
                (None, None) => return Err("index or target is required".into()),
            };
            list.remove(i.ok_or("no such binding")?);
            Ok(())
        },
    },
    // ---- project ----
    Operator {
        id: "project.set_canvas",
        label: "Set Canvas",
        description: "What every output renders: width and height in pixels (past about 4K's pixels, brought down to them, its shape kept), frames per second.",
        example: r#"{"width": 1920, "height": 1080, "fps": 60}"#,
        undo: true,
        poll: always,
        exec: |s, v| {
            let SetCanvas { width, height, fps } = props(v)?;
            let c = &mut s.project.canvas;
            for (value, field, info) in [(width, &mut c.width, &CANVAS[0]), (height, &mut c.height, &CANVAS[1]), (fps, &mut c.fps, &CANVAS[2])] {
                if let Some(v) = value {
                    *field = info.clamp(v).round() as u32;
                }
            }
            (c.width, c.height) = fit_canvas(c.width, c.height);
            Ok(())
        },
    },
    // ---- history ----
    Operator {
        id: "edit.undo",
        label: "Undo",
        description: "Take back the last change.",
        example: "{}",
        undo: false,
        poll: |s| !s.undo.is_empty(),
        exec: |s, _| {
            s.step(true);
            Ok(())
        },
    },
    Operator {
        id: "edit.redo",
        label: "Redo",
        description: "Bring back the change you just undid.",
        example: "{}",
        undo: false,
        poll: |s| !s.redo.is_empty(),
        exec: |s, _| {
            s.step(false);
            Ok(())
        },
    },
];

fn random_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new().build_hasher().finish()
}

/// A shared source ("kick", "bar_phase") as the output of scene `c`'s Audio In or Beat node,
/// the node added left of the graph when the scene has none; `None` for any other source.
/// What a new Audio In's card leaves out (its `hidden` parameter, a bit per output): kick, snare,
/// hat and melody, so it lists level, bass, mid and high; any in use shows anyway.
const AUDIO_HIDDEN: f32 = 240.0;

/// Format 2 → 3: a scene whose picture went out through a Grid (the last output something was
/// wired into, else the first, as it was) gets it into its Output node, added beside the grid
/// where there's none.
pub(crate) fn grid_out(scene: &mut Scene) {
    let outputs: Vec<usize> = (0..scene.nodes.len()).filter(|&i| scene.nodes[i].kind == OUTPUT || scene.nodes[i].kind == GRID).collect();
    let wired = |i: usize| scene.edges.iter().any(|e| e.to.0 == scene.nodes[i].id);
    let out = outputs.iter().rev().copied().find(|&i| wired(i)).or(outputs.first().copied());
    let Some(g) = out.filter(|&i| scene.nodes[i].kind == GRID) else { return };
    let grid = scene.nodes[g].id.clone();
    let output = match scene.nodes.iter().find(|n| n.kind == OUTPUT) {
        Some(n) => n.id.clone(),
        None => {
            let id = new_id("n_", |id| scene.node(id).is_some());
            let pos = [scene.nodes[g].pos[0] + 260.0, scene.nodes[g].pos[1]];
            scene.nodes.push(Node { id: id.clone(), kind: OUTPUT.into(), name: "Output".into(), off: false, pos, params: BTreeMap::new(), mappings: Vec::new(), code: None, file: None, media: None, text: None, icon: String::new() });
            id
        }
    };
    scene.edges.retain(|e| e.to.0 != output);
    scene.edges.push(Edge { from: (grid, 0), to: (output, 0) });
}

/// Format 4: a Shader node that read an input (one is wired in, or its header asks for a texture
/// or an image) becomes a Shader FX.
pub(crate) fn shader_fx(scene: &mut Scene) {
    let wired: Vec<String> = scene.edges.iter().map(|e| e.to.0.clone()).collect();
    for n in scene.nodes.iter_mut().filter(|n| n.kind == SHADER) {
        let header = n.code.as_deref().map_or("", |c| c.split_once("*/").map_or(c, |(h, _)| h));
        if wired.contains(&n.id) || header.contains("\"texture\"") || header.contains("\"image\"") {
            n.kind = SHADER_FX.into();
        }
    }
}

pub(crate) fn signal_source(project: &mut Project, c: usize, source: &str) -> Option<String> {
    let (kind, port) = shared_signal(source)?;
    if let Some(key) = project.scenes[c].signal_key(source) {
        return Some(key);
    }
    let id = new_id("n_", |id| project.scenes.iter().any(|c| c.node(id).is_some()));
    let nodes = &mut project.scenes[c].nodes;
    let [x, y] = nodes.iter().fold([f32::INFINITY; 2], |[x, y], n| [x.min(n.pos[0]), y.min(n.pos[1])]);
    let pos = if x.is_finite() { [x - 260.0, y] } else { [0.0, 0.0] };
    let name = node_kind(kind).map_or(kind, |k| k.label).to_string();
    nodes.push(Node {
        id: id.clone(),
        kind: kind.into(),
        name,
        off: false,
        pos,
        params: if kind == AUDIO { BTreeMap::from([("hidden".into(), [AUDIO_HIDDEN, 0.0, 0.0, 0.0])]) } else { BTreeMap::new() },
        mappings: Vec::new(),
        code: None,
        file: None,
        media: None,
        text: None,
        icon: String::new(),
    });
    Some(format!("{id}/{port}"))
}

/// An icon as kept: one character ("❄️" is ❄ and a variation selector the icon font draws
/// as nothing).
fn short_icon(icon: &str) -> String {
    icon.trim().chars().next().map(String::from).unwrap_or_default()
}

/// A fresh id, `prefix` and four random letters or digits, unlike any `taken` one
/// (xorshift64*, no extra crates).
/// `scene` made ready to join `p`: a new id and a free name (`name`, else its own), new node
/// ids with everything that named the old ones following them, and a shader file of its own for
/// each Shader node.
fn adopt(p: &Project, mut scene: Scene, name: Option<String>) -> Scene {
    scene.id = new_id("s_", |id| p.scene(id).is_some());
    let base = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| scene.name.clone());
    let taken = |name: &str| p.scenes.iter().any(|c| c.name == name);
    scene.name = if taken(&base) { (2..).map(|k| format!("{base} {k}")).find(|n| !taken(n)).unwrap_or(base) } else { base };
    // the files are picked one by one against the project with the ones picked so far in it:
    // two nodes of the same name never share one
    let mut files = p.clone();
    files.scenes.push(Scene::default());
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    for n in &mut scene.nodes {
        let new = new_id("n_", |id| p.scenes.iter().any(|c| c.node(id).is_some()) || ids.values().any(|v| v == id));
        ids.insert(std::mem::replace(&mut n.id, new.clone()), new);
        if let Some(file) = &n.file {
            let ext = file.rsplit_once('.').map_or("wgsl", |(_, e)| e);
            n.file = Some(shader_file(&files, &n.name, ext));
            files.scenes.last_mut().expect("pushed").nodes.push(n.clone());
        }
    }
    let renamed = |id: &mut String| {
        if let Some(new) = ids.get(id.as_str()) {
            *id = new.clone();
        }
    };
    for e in &mut scene.edges {
        renamed(&mut e.from.0);
        renamed(&mut e.to.0);
    }
    for m in scene.nodes.iter_mut().flat_map(|n| &mut n.mappings) {
        if let Some(new) = signal_output(&m.source).and_then(|(id, port)| Some(format!("{}/{port}", ids.get(id)?))) {
            m.source = new;
        }
    }
    for t in scene.macros.iter_mut().flat_map(|m| &mut m.targets) {
        renamed(&mut t.node);
    }
    scene
}

fn new_id(prefix: &str, taken: impl Fn(&str) -> bool) -> String {
    const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut x = random_seed() | 1;
    let mut rnd = || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        CHARS[(x.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as usize % CHARS.len()] as char
    };
    loop {
        let id: String = prefix.chars().chain((0..4).map(|_| rnd())).collect();
        if !taken(&id) {
            return id;
        }
    }
}
