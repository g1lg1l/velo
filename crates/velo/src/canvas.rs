//! The node canvas (spike S4): the active scene's graph on an endless plane, Velo's default
//! view. TouchDesigner's model in Velo's look: a viewer on every node, Tab for the node menu,
//! H to frame everything, wires dragged from outputs to inputs.
//! Figma's view on top (the user's ask): an endless plane, zoom eased about the pointer up to
//! 8×, never further out than the view that shows every node, Shift+1 / 2 / 0, Cmd/Ctrl+= / -,
//! and a zoom menu in the corner.
//! The plane is one egui widget. Nodes, ports and wires are hit-tested here, so a node costs
//! a few shapes instead of a widget per port, and every drag follows one state machine.
//! Ideas from FigCraft (storytold/figcraft, MIT OR Apache-2.0): Figma's zoom steps, the fit
//! margin, two-finger pan on touch screens.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use eframe::egui::{self, Align2, CornerRadius, CursorIcon, EventFilter, FontId, Frame, Id, Key, Margin, Mesh, Modifiers, Painter, PointerButton, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::{Value, json};
use velo_core::{NODE_KINDS, NodeKind, Scene, Session, node_kind};

use crate::shell::Tone;
use crate::theme::Theme;
use crate::{App, i18n, icons, keymap, kit, shell, widgets};

/// A node's card in canvas units: a header with its name over a 16:9 viewer (TouchDesigner
/// shows what every node makes).
pub const NODE_W: f32 = 160.0;
const HEADER_H: f32 = 28.0;
const VIEWER_H: f32 = NODE_W * 9.0 / 16.0;
/// The strip under the content: the kind, what moves it, the chevron that drops its
/// parameters down.
const FOOTER_H: f32 = 20.0;
pub const NODE_H: f32 = HEADER_H + VIEWER_H + FOOTER_H;
/// An Audio In's tape deck, under its header.
const DECK_H: f32 = 84.0;
/// A line of the jacks under a card's footer: a signal plugged into it.
const JACK_H: f32 = 22.0;
/// An output's row in a card that lists them (an Audio In's Level, Bass…).
const ROW_H: f32 = 60.0;
/// Samples in a row's curve: a second and a half.
const TRAIL: usize = 45;
/// Room the canvas leaves between nodes it places itself (append, duplicate, the demo).
pub const SPACING: f32 = 56.0;
/// Zoom limits. Going out, the real limit is the view that shows every node ([`min_zoom`]).
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 8.0;
/// However few the nodes, you can zoom out at least this far.
const ZOOM_FLOOR: f32 = 0.5;
/// Figma's zoom steps (Cmd/Ctrl+= and -), via FigCraft.
const STEPS: [f32; 10] = [0.02, 0.03, 0.06, 0.13, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// Screen room around the nodes when the view fits them.
const FIT_MARGIN: f32 = 64.0;
/// How fast a wheel or pinch zoom catches up with the pointer (seconds; Figma-like easing).
const ZOOM_EASE: f32 = 0.05;
/// Below this, names would be too small to read: cards keep their color and viewer only.
const TEXT_ZOOM: f32 = 0.5;
/// Below this, ports would be specks: only the one under the pointer shows (wires still end
/// where they plug in).
const PORT_ZOOM: f32 = 0.4;
const GRID: f32 = 24.0;
/// How near (screen points) the pointer must be to grab a port or a wire.
const PORT_REACH: f32 = 9.0;
const WIRE_REACH: f32 = 5.0;
/// The bubble round a port a dragged wire is drawn to (screen points at 100 %): the wire snaps
/// from [`PULL`] times as far, a neck joining its end to the bubble.
const SNAP: f32 = 26.0;
const PULL: f32 = 1.7;
/// A wire end's own small bubble, which melts into the port's.
const TIP: f32 = 6.0;
/// The node menu: its width, its list's height and a line's (a name over what it does).
const MENU_W: f32 = 680.0;
const LIST_H: f32 = 420.0;
const MENU_ROW_H: f32 = 46.0;

/// The canvas widget's id: it holds the keyboard while you work on the graph.
pub fn id() -> Id {
    Id::unique("node-canvas")
}

/// Where the canvas looks: the plane's origin sits `pan` from the canvas' center, scaled by
/// `zoom`. Centered, so panels opening and closing leave the graph where it was.
#[derive(Clone, Copy, PartialEq)]
struct Camera {
    pan: Vec2,
    zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0 }
    }
}

impl Camera {
    fn screen(&self, center: Pos2, p: Pos2) -> Pos2 {
        center + self.pan + p.to_vec2() * self.zoom
    }

    fn graph(&self, center: Pos2, s: Pos2) -> Pos2 {
        ((s - center - self.pan) / self.zoom).to_pos2()
    }

    /// At `zoom`, keeping the point under `at` where it is.
    fn zoomed(self, center: Pos2, at: Pos2, zoom: f32) -> Self {
        let g = self.graph(center, at);
        Self { zoom, pan: at - center - g.to_vec2() * zoom }
    }

    fn lerp(a: Self, b: Self, k: f32) -> Self {
        Self { pan: a.pan + (b.pan - a.pan) * k, zoom: a.zoom + (b.zoom - a.zoom) * k }
    }
}

/// What the pointer is over: a port (node index, port, is it an output), a node, a wire (edge
/// index), a signal's line to a card it moves (its index in the frame's lines).
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Port(usize, u8, bool),
    Node(usize),
    Wire(usize),
    Map(usize),
    /// A cable tie (its index in the scene's), and the × over one that takes it off.
    Tie(usize),
    Untie(usize),
    Empty,
}

/// What a node's (or the canvas') right-button menu asked for.
enum Act {
    Props(String),
    /// Turn a node into another kind: the node menu opens at the pointer to pick it.
    Replace(String, Pos2),
    Rename(String, String),
    Duplicate,
    Toggle(bool),
    Disconnect(String),
    Delete,
    Add(Pos2),
    Fit,
    Arrange,
}

/// A zoom asked for by a key or the zoom menu, done (eased) where the canvas knows its size.
#[derive(Clone, Copy, PartialEq)]
pub enum ZoomTo {
    /// Every node (H, Shift+1).
    Fit,
    /// The selected nodes (Shift+2).
    Selection,
    /// A level, about the view's center (Shift+0 is 1.0).
    Level(f32),
    /// Figma's next step in or out, about the pointer if it's on the canvas.
    In,
    Out,
}

enum Drag {
    /// The view, from where it was when the drag began.
    Pan { origin: Pos2, pan: Vec2 },
    /// The selection: `anchor` is the grabbed node and where it started, `origin` the pointer
    /// on the plane. One undo step: the merge closes on release.
    Nodes { anchor: (String, [f32; 2]), origin: Pos2 },
    /// A wire from an output; `lifted` is the input it was pulled off, if any.
    Wire { from: (String, u8), lifted: Option<(String, u8)> },
    /// A wire drawn back from a free input, to an output.
    Back { into: (String, u8) },
    /// A selection rectangle from `origin`; `base` was selected before (Shift adds).
    Band { origin: Pos2, base: Vec<String> },
    /// A cable tie slid along, `grab` where it was held from its middle (on the plane). One
    /// undo step.
    Tie { id: String, grab: Vec2 },
    /// A line drawn across cables with Alt: let go, a tie gathers those it crosses.
    Cut { origin: Pos2 },
    /// A signal's cable taken out of a card's jack (`node`, the `source`), from the signal's
    /// output `from`: onto another card it moves one of its values, onto nothing it's gone.
    Signal { from: (String, u8), node: String, source: String },
}

/// The node menu (TouchDesigner's OP Create dialog): search, pick, Enter.
pub struct Menu {
    /// Where a new node goes on the plane.
    graph: Pos2,
    /// A wire dropped on empty canvas: the new node gets wired from it.
    from: Option<(String, u8)>,
    /// A wire's input the new node goes into (its + button): the node goes in between.
    into: Option<(String, u8)>,
    /// A node to turn into the kind picked, instead of adding one (its menu's Replace).
    replace: Option<String>,
    query: String,
    /// The group whose tab is picked; None is All.
    tab: Option<&'static str>,
    selected: usize,
    /// Scroll the list to its top this frame.
    top: bool,
}

impl Menu {
    fn new(graph: Pos2, from: Option<(String, u8)>, into: Option<(String, u8)>, replace: Option<String>) -> Self {
        Menu { graph, from, into, replace, query: String::new(), tab: None, selected: 0, top: true }
    }
}

/// The canvas' UI state: the view per scene and the selection. Not saved, not undoable.
#[derive(Default)]
pub struct State {
    /// The listed outputs' recent values (node id, output), a sample every 1/30 s, and when
    /// the last was taken: the curves in Audio In's and Beat's rows.
    trails: HashMap<(String, u8), VecDeque<f32>>,
    trail_at: f64,
    cameras: HashMap<String, Camera>,
    anim: Option<(Camera, Camera, f64)>,
    /// Where a wheel or pinch zoom is headed, and the screen point it zooms about.
    goal: Option<(f32, Pos2)>,
    pub selected: Vec<String>,
    /// A selected wire, by the input it ends at.
    pub wire: Option<(String, u8)>,
    /// A selected signal's line: the node it moves (one value or several), the source.
    pub map: Option<(String, String)>,
    /// Wires just made, by their input, and when: a light runs along each.
    pulses: Vec<((String, u8), f64)>,
    /// Ports a dragged wire is drawn to: a bubble round each, growing or going.
    bubbles: Vec<Bubble>,
    /// Wires just plugged in by hand: the end slides home, a ring spreads, lights run round the card.
    landings: Vec<Landing>,
    /// A wire let go of nowhere, springing back into its port.
    loose: Option<Loose>,
    /// The nodes a dragged wire can't go into: faded while it's dragged, and back after.
    dim: Vec<String>,
    /// The scene the selection belongs to.
    scene: Option<String>,
    drag: Option<Drag>,
    pub menu: Option<Menu>,
    /// Asked for by a key or a button; done where the canvas knows its size.
    pub open_menu: bool,
    pub zoom_to: Option<ZoomTo>,
    reveal: Option<String>,
    /// Under the pointer last frame (the keymap's canvas keys work then).
    pub hovered: bool,
    /// The node whose parameters drop down under its card (its header's chevron).
    pub props: Option<String>,
    /// What the right button was pressed on: a node, or the empty canvas, and where.
    context: Option<(Option<String>, Pos2)>,
    /// The Grid cell its inspector edits (from 0).
    pub grid_cell: usize,
    /// A signal dropped on a card: its output ("n_ab12/4"), the card's node, where: the
    /// parameter it'll move is picked from a menu there.
    pick: Option<(String, String, Pos2)>,
    /// The part of the canvas no panel covers (last frame's): what fitting fills, where the
    /// zoom menu and empty state go. The panels float over the rest.
    pub view: Option<Rect>,
    /// Asked for by the toolbar or the menu: the nodes arranged, done where the canvas runs.
    pub arrange: bool,
    /// Nodes just arranged: where each was and when, it slides to its place.
    glides: HashMap<String, ([f32; 2], f64)>,
    /// Signals just plugged into a card (its node, the source) and when: a ring leaves its jack.
    pub plug_ins: Vec<((String, String), f64)>,
    /// How far each card's open slot is out (0..1), by node.
    slots: HashMap<String, f32>,
    /// Ties just made, where and when: their cables cinch in.
    cinch: Vec<([f32; 2], f64)>,
    /// The toolbar's tie tool: a plain drag across cables ties them (as Alt+drag does).
    pub tie_tool: bool,
    /// The nodes hidden (the toolbar's eye): the picture behind, nothing over it.
    pub hidden: bool,
    /// The tie clicked last: Delete takes it off.
    tie: Option<String>,
    /// A signal's cable taken off one card and dropped on another (that card, the source): once
    /// a value of the new card is picked, the old card's lose it.
    repick: Option<(String, String)>,
    /// The Audio In's tape reels: their angle; the loudest its level was lately, falling.
    reel: f32,
    peak: f32,
}

/// A bubble round a port a dragged wire is drawn to: its size (0..1) on its way from `from` to
/// `to` since `at`, in the wire's color.
struct Bubble {
    port: (String, u8, bool),
    color: egui::Color32,
    from: f32,
    to: f32,
    at: f64,
}

impl Bubble {
    /// It pops in on a spring and shrinks away quicker.
    fn length(&self, motion: f32) -> f32 {
        motion * if self.to > self.from { 3.0 } else { 1.2 }
    }

    fn size(&self, now: f64, motion: f32) -> f32 {
        let length = self.length(motion);
        if length <= 0.0 {
            return self.to;
        }
        let k = (now - self.at) as f32 / length;
        self.from + (self.to - self.from) * if self.to > self.from { kit::spring(k) } else { kit::ease(k) }
    }

    fn moving(&self, now: f64, motion: f32) -> bool {
        ((now - self.at) as f32) < self.length(motion)
    }
}

/// A wire plugged in by hand: its input (what finds it), which end went in (the output, for one
/// drawn back from an input), where it was let go on the plane, its color, when.
struct Landing {
    to: (String, u8),
    output: bool,
    drop: Pos2,
    color: egui::Color32,
    at: f64,
}

/// A wire let go of nowhere: the port it springs back into (node, port, an output), where it
/// was let go on the plane, its color, when.
struct Loose {
    port: (String, u8, bool),
    drop: Pos2,
    color: egui::Color32,
    at: f64,
}

/// The chevron in a card's footer that drops its parameters down under it.
fn props_button(r: Rect, zoom: f32) -> Rect {
    Rect::from_center_size(pos2(r.right() - 14.0 * zoom, r.bottom() - FOOTER_H * zoom / 2.0), vec2(20.0, 18.0) * zoom)
}

fn power_button(r: Rect, zoom: f32) -> Rect {
    Rect::from_center_size(pos2(r.right() - 14.0 * zoom, r.top() + HEADER_H * zoom / 2.0), vec2(20.0, 18.0) * zoom)
}

/// A card's content: its picture, meters or curve, between the header and the footer.
fn viewer_rect(r: Rect, zoom: f32) -> Rect {
    Rect::from_min_max(pos2(r.left() + 1.0, r.top() + HEADER_H * zoom + 0.5), pos2(r.right() - 1.0, r.bottom() - FOOTER_H * zoom - 0.5))
}

/// The middle of a listed output's row (under the deck, when the card has one).
fn row_y(r: Rect, zoom: f32, deck: f32, row: usize) -> f32 {
    r.top() + (HEADER_H + deck + (row as f32 + 0.5) * ROW_H) * zoom
}

/// A card's width on the plane: an Audio In's twice the others', for its deck and its curves.
fn node_w(kind: &str) -> f32 {
    if kind == velo_core::AUDIO { 2.0 * NODE_W } else { NODE_W }
}

/// How tall the tape deck under a card's header is (an Audio In's), on the plane.
fn deck_h(kind: &str) -> f32 {
    if kind == velo_core::AUDIO { DECK_H } else { 0.0 }
}

/// A card on the plane at `pos`, its jacks apart: as tall as its rows or its inputs need. Its
/// place is a usual card's top left: a wider one reaches further left, its outputs where they were.
fn plane_rect(scene: &Scene, n: &velo_core::Node, pos: [f32; 2]) -> Rect {
    let w = node_w(&n.kind);
    let h = match rows(scene, n) {
        Some(list) => HEADER_H + deck_h(&n.kind) + list.len().max(1) as f32 * ROW_H + FOOTER_H,
        None => NODE_H + ports_span(velo_core::inputs_shown(scene, n)) - VIEWER_H,
    };
    Rect::from_min_size(pos2(pos[0] + NODE_W - w, pos[1]), vec2(w, h))
}

/// The signals plugged into a card, a jack each under its footer: their sources, in order.
fn jacks(n: &velo_core::Node) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for m in n.mappings.iter().filter(|m| velo_core::signal_output(&m.source).is_some()) {
        if !out.contains(&m.source.as_str()) {
            out.push(&m.source);
        }
    }
    out
}

/// A card on the plane with its jacks: what it takes of the canvas.
fn plane_room(scene: &Scene, n: &velo_core::Node) -> Rect {
    let r = plane_rect(scene, n, n.pos);
    Rect::from_min_size(r.min, r.size() + vec2(0.0, jacks(n).len() as f32 * JACK_H))
}

/// Where each node goes for the graph to read left to right: a column per step from where it
/// starts (a signal before what it moves, the Output last), each column's nodes in the order of
/// what feeds them, right-aligned and spaced, the whole centered where it was. The nodes that
/// move, and where to.
pub fn arrange(scene: &Scene) -> Vec<(String, [f32; 2])> {
    let nodes = &scene.nodes;
    let index: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    // what feeds what: wires, and signals into what they move
    let mut links: Vec<(usize, usize)> = scene.edges.iter().filter_map(|e| Some((*index.get(e.from.0.as_str())?, *index.get(e.to.0.as_str())?))).collect();
    for (j, n) in nodes.iter().enumerate() {
        links.extend(n.mappings.iter().filter_map(|m| velo_core::signal_output(&m.source).and_then(|(id, _)| index.get(id)).map(|&i| (i, j))));
    }
    links.retain(|(a, b)| a != b);
    links.sort_unstable();
    links.dedup();
    // a node's column: one past the furthest of what feeds it (a loop stops growing at the count)
    let mut col = vec![0usize; nodes.len()];
    for _ in 0..nodes.len() {
        let mut moved = false;
        for &(a, b) in &links {
            if col[b] <= col[a] && col[a] + 1 < nodes.len() {
                (col[b], moved) = (col[a] + 1, true);
            }
        }
        if !moved {
            break;
        }
    }
    let last = col.iter().copied().max().unwrap_or(0);
    for (i, n) in nodes.iter().enumerate() {
        if node_kind(&n.kind).is_some_and(|k| k.category == velo_core::Category::Output) {
            col[i] = last;
        }
    }
    // a node nothing feeds sits just before the first one it feeds, near it, not far left
    for i in (0..nodes.len()).filter(|&i| !links.iter().any(|&(_, b)| b == i)) {
        if let Some(first) = links.iter().filter(|&&(a, _)| a == i).map(|&(_, b)| col[b]).min() {
            col[i] = col[i].max(first.saturating_sub(1));
        }
    }
    let columns: Vec<Vec<usize>> = (0..=last).map(|c| (0..nodes.len()).filter(|&i| col[i] == c).collect()).collect();
    // each node level with what it's linked to, pushed down past the one above it: left to
    // right with what feeds it, then right to left the ones nothing feeds, with what they feed
    const GAP_X: f32 = 2.0 * SPACING;
    const GAP_Y: f32 = 40.0;
    let size = |i: usize| plane_room(scene, &nodes[i]).size();
    let mut center = vec![0.0f32; nodes.len()];
    let mean = |ys: Vec<f32>| (!ys.is_empty()).then(|| ys.iter().sum::<f32>() / ys.len() as f32);
    let stack = |list: &[usize], want: &[Option<f32>], center: &mut Vec<f32>| {
        let mut order: Vec<usize> = (0..list.len()).collect();
        order.sort_by(|&a, &b| want[a].unwrap_or(f32::MAX).total_cmp(&want[b].unwrap_or(f32::MAX)).then(nodes[list[a]].pos[1].total_cmp(&nodes[list[b]].pos[1])));
        let mut floor: Option<f32> = None;
        for k in order {
            let (i, h) = (list[k], size(list[k]).y);
            let top = match (want[k].map(|c| c - h / 2.0), floor) {
                (Some(w), Some(f)) => w.max(f),
                (Some(w), None) => w,
                (None, Some(f)) => f,
                (None, None) => 0.0,
            };
            center[i] = top + h / 2.0;
            floor = Some(top + h + GAP_Y);
        }
    };
    for list in &columns {
        let want: Vec<Option<f32>> = list.iter().map(|&i| mean(links.iter().filter(|&&(a, b)| b == i && col[a] < col[i]).map(|&(a, _)| center[a]).collect())).collect();
        stack(list, &want, &mut center);
    }
    for list in columns.iter().rev() {
        let want: Vec<Option<f32>> = list
            .iter()
            .map(|&i| match links.iter().any(|&(_, b)| b == i) {
                true => Some(center[i]),
                false => mean(links.iter().filter(|&&(a, b)| a == i && col[b] > col[i]).map(|&(_, b)| center[b]).collect()).or(Some(center[i])),
            })
            .collect();
        stack(list, &want, &mut center);
    }
    // the columns side by side, cards right-aligned in theirs: a card's place is a usual card's
    // top left, wider ones reach left
    let mut placed: Vec<(usize, [f32; 2])> = Vec::new();
    let mut x = 0.0;
    for list in columns.iter().filter(|l| !l.is_empty()) {
        let width = list.iter().map(|&i| size(i).x).fold(0.0, f32::max);
        placed.extend(list.iter().map(|&i| (i, [x + width - NODE_W, center[i] - size(i).y / 2.0])));
        x += width + GAP_X;
    }
    let room = |i: usize, p: [f32; 2]| Rect::from_min_size(pos2(p[0] + NODE_W - size(i).x, p[1]), size(i));
    let new = placed.iter().map(|&(i, p)| room(i, p)).reduce(Rect::union);
    let shift = match (bounds(scene, None), new) {
        (Some(old), Some(new)) => old.center() - new.center(),
        _ => Vec2::ZERO,
    };
    placed
        .into_iter()
        .map(|(i, p)| (i, [(p[0] + shift.x).round(), (p[1] + shift.y).round()]))
        .filter(|&(i, p)| nodes[i].pos != p)
        .map(|(i, p)| (nodes[i].id.clone(), p))
        .collect()
}

/// How long a tie keeps its cables straight and level, side by side, and how far apart.
const TIE_RUN: f32 = 30.0;
const TIE_GAP: f32 = 4.0;

/// A cable from `a` to `b` through the ties holding it (screen points: its own place in each
/// bundle, left to right), straight through each, curving between.
fn routed(a: Pos2, b: Pos2, through: &[Pos2], zoom: f32) -> Vec<Pos2> {
    let half = vec2(TIE_RUN * zoom / 2.0, 0.0);
    let (mut pts, mut from) = (Vec::new(), a);
    for &p in through {
        pts.extend(bezier(from, p - half, zoom));
        from = p + half;
    }
    pts.extend(bezier(from, b, zoom));
    pts
}

/// Where a cable starts and ends on the plane, as a tie names it, each node at `at(node)`: an
/// output, and an input or a jack.
fn plane_ends(scene: &Scene, key: &str, at: &dyn Fn(&velo_core::Node) -> [f32; 2]) -> Option<(Pos2, Pos2)> {
    let output = |id: &str, p: u8| {
        let n = scene.node(id)?;
        let r = plane_rect(scene, n, at(n));
        Some(match rows(scene, n) {
            Some(list) => pos2(r.right(), row_y(r, 1.0, deck_h(&n.kind), list.iter().position(|&q| q == p).unwrap_or(0))),
            None => port_pos(r, 1.0, node_kind(&n.kind).map_or(1, |k| k.outputs.len()), p, true),
        })
    };
    match key.split_once(' ') {
        Some((node, source)) => {
            let (from, p) = velo_core::signal_output(source)?;
            let n = scene.node(node)?;
            let r = plane_rect(scene, n, at(n));
            let k = jacks(n).iter().position(|s| *s == source)?;
            Some((output(from, p)?, jack_socket(Rect::from_min_size(r.left_bottom(), vec2(r.width(), 0.0)), 1.0, k)))
        }
        None => {
            let e = scene.edges.iter().find(|e| velo_core::Tie::wire(&e.to) == key)?;
            let n = scene.node(&e.to.0)?;
            let to = port_pos(plane_rect(scene, n, at(n)), 1.0, velo_core::inputs_shown(scene, n), e.to.1, false);
            Some((output(&e.from.0, e.from.1)?, to))
        }
    }
}

/// The bundles worth a tie, nodes at `at(node)`: two or more cables from one output, tied just
/// past it, and two or more into one node, tied just before it; none a tie already holds all of.
fn bundles(scene: &Scene, at: &dyn Fn(&velo_core::Node) -> [f32; 2]) -> Vec<(Pos2, Vec<String>)> {
    let keys: Vec<String> = scene.edges.iter().map(|e| velo_core::Tie::wire(&e.to)).chain(scene.nodes.iter().flat_map(|n| jacks(n).into_iter().map(|s| velo_core::Tie::line(&n.id, s)))).collect();
    let mut from: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut into: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in keys {
        let nodes = cable_nodes(scene, &key);
        if let [source, target] = nodes[..] {
            let out = match key.split_once(' ') {
                Some((_, s)) => s.to_string(),
                None => scene.edges.iter().find(|e| velo_core::Tie::wire(&e.to) == key).map_or_else(|| source.to_string(), |e| velo_core::Tie::wire(&e.from)),
            };
            from.entry(out).or_default().push(key.clone());
            into.entry(target.to_string()).or_default().push(key);
        }
    }
    let held = |cables: &[String]| scene.ties.iter().any(|t| cables.iter().all(|c| t.cables.contains(c)));
    let mut out = Vec::new();
    for (side, groups) in [(1.0, from), (-1.0, into)] {
        for cables in groups.into_values().filter(|c| c.len() >= 2 && !held(c)) {
            let ends: Vec<(Pos2, Pos2)> = cables.iter().filter_map(|c| plane_ends(scene, c, at)).collect();
            if ends.is_empty() {
                continue;
            }
            // past the output's edge, or before the node's, level with where the cables meet it
            let edge: Vec<Pos2> = ends.iter().map(|(a, b)| if side > 0.0 { *a } else { *b }).collect();
            let y = edge.iter().map(|p| p.y).sum::<f32>() / edge.len() as f32;
            out.push((pos2(edge[0].x + side * SPACING, y), cables));
        }
    }
    out
}

/// The nodes a cable joins, as a tie names it: a wire's two, a signal's line's source and card.
fn cable_nodes<'a>(scene: &'a Scene, key: &'a str) -> Vec<&'a str> {
    match key.split_once(' ') {
        Some((node, source)) => velo_core::signal_output(source).map_or(vec![node], |(from, _)| vec![from, node]),
        None => {
            let to = key.rsplit_once('/').map_or(key, |(node, _)| node);
            let from = scene.edges.iter().find(|e| velo_core::Tie::wire(&e.to) == key).map(|e| e.from.0.as_str());
            from.into_iter().chain([to]).collect()
        }
    }
}

/// A tie on screen round its `n` cables `gap` apart: a capsule standing across them.
fn tie_rect(at: Pos2, n: usize, gap: f32, zoom: f32) -> Rect {
    Rect::from_center_size(at, vec2(8.0 * zoom.max(0.6), n.max(1) as f32 * gap + 10.0 * zoom.max(0.6)))
}

/// The × over a tie that takes it off (shown while the pointer is near it or it's chosen).
fn untie_badge(tie: Rect) -> Rect {
    Rect::from_center_size(tie.center_top() - vec2(0.0, 11.0), vec2(14.0, 14.0))
}

/// Whether the segments `a`–`b` and `c`–`d` cross.
fn crosses(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> bool {
    let side = |p: Pos2, q: Pos2, r: Pos2| (q - p).x * (r - p).y - (q - p).y * (r - p).x;
    side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
}

/// Where a jack's socket is, the `k`th line of a card's strip of them: on the card's left edge,
/// as its inputs are.
fn jack_socket(strip: Rect, zoom: f32, k: usize) -> Pos2 {
    pos2(strip.left(), strip.top() + (k as f32 + 0.5) * JACK_H * zoom)
}

fn node_rect(cam: &Camera, center: Pos2, pos: [f32; 2]) -> Rect {
    Rect::from_min_size(cam.screen(center, pos2(pos[0], pos[1])), vec2(NODE_W, NODE_H) * cam.zoom)
}

/// How far down a card its ports spread: along the viewer, or further when they're many (a
/// Blend of sixteen), a port at least every [`PORT_GAP`].
fn ports_span(count: usize) -> f32 {
    VIEWER_H.max((count as f32 + 1.0) * PORT_GAP)
}

const PORT_GAP: f32 = 9.0;

/// Inputs down the left edge, outputs down the right, spread along the viewer.
fn port_pos(r: Rect, zoom: f32, count: usize, i: u8, output: bool) -> Pos2 {
    let top = r.top() + HEADER_H * zoom;
    let y = top + ports_span(count) * zoom * (f32::from(i) + 1.0) / (count as f32 + 1.0);
    pos2(if output { r.right() } else { r.left() }, y)
}

/// A multi-output signal node's outputs, listed as rows in its card: those not hidden (its
/// `hidden` parameter, a bit per output), and any in use whatever it says.
pub fn rows(scene: &Scene, n: &velo_core::Node) -> Option<Vec<u8>> {
    let k = node_kind(&n.kind).filter(|k| k.category == velo_core::Category::Signal && k.outputs.len() > 1)?;
    let hidden = n.params.get("hidden").map_or(0, |v| v[0] as u32);
    Some((0..k.outputs.len() as u8).filter(|&p| hidden & (1 << p) == 0 || output_used(scene, &n.id, p)).collect())
}

/// Whether a node's output is wired or mapped.
pub fn output_used(scene: &Scene, node: &str, port: u8) -> bool {
    scene.edges.iter().any(|e| e.from.0 == node && e.from.1 == port) || scene.nodes.iter().flat_map(|m| &m.mappings).any(|m| velo_core::signal_output(&m.source) == Some((node, port)))
}

/// The nodes' bounds on the plane (some ids only, or all).
fn bounds(scene: &Scene, only: Option<&[String]>) -> Option<Rect> {
    let mut nodes = scene.nodes.iter().filter(|n| only.is_none_or(|ids| ids.contains(&n.id))).map(|n| plane_room(scene, n));
    let first = nodes.next()?;
    Some(nodes.fold(first, |a, r| a.union(r)))
}

/// The zoom that fits `b` in `view` with a margin.
fn fit_zoom(b: Rect, view: Rect) -> f32 {
    ((view.width() - 2.0 * FIT_MARGIN).max(50.0) / b.width()).min((view.height() - 2.0 * FIT_MARGIN).max(50.0) / b.height())
}

/// The view that centers `b` in `view` (the uncovered part of the canvas, whose center is
/// `center`'s offset), no closer than `max` zoom.
fn fit(b: Option<Rect>, center: Pos2, view: Rect, max: f32) -> Camera {
    let Some(b) = b else { return Camera { pan: view.center() - center, zoom: 1.0 } };
    let zoom = fit_zoom(b, view).clamp(MIN_ZOOM, max);
    Camera { pan: view.center() - center - b.center().to_vec2() * zoom, zoom }
}

/// How far out you can zoom: until every node shows (Figma-style, recomputed as the graph
/// grows), and at least to [`ZOOM_FLOOR`] so an empty scene doesn't zoom out forever.
fn min_zoom(scene: &Scene, view: Rect) -> f32 {
    bounds(scene, None).map_or(ZOOM_FLOOR, |b| fit_zoom(b, view).min(ZOOM_FLOOR)).max(MIN_ZOOM)
}

/// Figma's next zoom step in or out from `z`.
fn step(z: f32, up: bool) -> f32 {
    if up { STEPS.into_iter().find(|s| *s > z * 1.001).unwrap_or(MAX_ZOOM) } else { STEPS.into_iter().rev().find(|s| *s < z * 0.999).unwrap_or(MIN_ZOOM) }
}

/// A wire's curve, leaving the output and entering the input horizontally.
fn bezier(a: Pos2, b: Pos2, zoom: f32) -> Vec<Pos2> {
    let [c1, c2] = handles(a, b, zoom);
    let n = ((a.distance(b) / 12.0) as usize).clamp(8, 40);
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            (a.to_vec2() * (u * u * u) + c1.to_vec2() * (3.0 * u * u * t) + c2.to_vec2() * (3.0 * u * t * t) + b.to_vec2() * (t * t * t)).to_pos2()
        })
        .collect()
}

fn handles(a: Pos2, b: Pos2, zoom: f32) -> [Pos2; 2] {
    let dx = ((b.x - a.x).abs() * 0.5).max(48.0 * zoom);
    [a + vec2(dx, 0.0), b - vec2(dx, 0.0)]
}

fn near_polyline(p: Pos2, pts: &[Pos2], reach: f32) -> bool {
    pts.windows(2).any(|s| {
        let (a, b) = (s[0], s[1]);
        let ab = b - a;
        let k = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
        (a + ab * k).distance(p) <= reach
    })
}

/// The point a fraction `k` of the way along a polyline.
fn along(pts: &[Pos2], k: f32) -> Pos2 {
    let lengths: Vec<f32> = pts.windows(2).map(|w| w[0].distance(w[1])).collect();
    let mut left = lengths.iter().sum::<f32>() * k.clamp(0.0, 1.0);
    for (w, l) in pts.windows(2).zip(&lengths) {
        if left <= *l {
            return w[0] + (w[1] - w[0]) * (left / l.max(1e-6));
        }
        left -= l;
    }
    pts.last().copied().unwrap_or_default()
}

/// Where light `k` of `n`, crawling along a wire at `speed` px a second, is at `now`: slowly,
/// whatever the wire's length, a sign of what flows rather than its tempo.
fn crawl(pts: &[Pos2], now: f64, speed: f32, k: usize, n: usize) -> Pos2 {
    let length: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    along(pts, ((now * f64::from(speed) / f64::from(length.max(1.0))).fract() as f32 + k as f32 / n as f32).fract())
}

/// How wide a link's popover is.
const LINK_W: f32 = 280.0;

/// A link popover's header: what it joins, and × to close it (true when clicked), over a divider.
/// The popover moves by it.
fn link_header(ui: &mut Ui, t: &Theme, title: &str) -> bool {
    let (rect, header) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_h + 2.0 * t.gap), Sense::hover());
    let x = Rect::from_center_size(pos2(rect.right() - t.gap - 14.0, rect.center().y), vec2(t.control_h, t.control_h));
    if header.hovered() && !ui.rect_contains_pointer(x) {
        ui.ctx().set_cursor_icon(if ui.input(|i| i.pointer.primary_down()) { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }
    let close = ui.put(x, |ui: &mut Ui| kit::icon_button(ui, t, icons::X, tr!("Close"), false)).clicked();
    kit::fit_text(ui.painter(), pos2(rect.left() + t.pad, rect.center().y), Align2::LEFT_CENTER, title, t.body(), t.fg_text, x.left() - rect.left() - 2.0 * t.pad);
    kit::menu_line(ui, t);
    close
}

/// The number a mapping key names on a node (a vec2 by axis), as its slider shows it, and its
/// base value; none for a color (a signal turns its hue).
fn param_info(n: &velo_core::Node, key: &str) -> Option<(&'static velo_core::ParamInfo, f32)> {
    let (id, axis) = match key.rsplit_once('.') {
        Some((id, "x")) => (id, 0),
        Some((id, "y")) => (id, 1),
        _ => (key, 0),
    };
    let shader = crate::live::shader(&n.kind, n.code.as_deref())?;
    let (p, infos) = shader.controls.iter().find(|(p, _)| p.id == id)?;
    if p.ty == velo_engine::ParamType::Color {
        return None;
    }
    Some((infos.get(axis)?, n.params.get(id).copied().unwrap_or(p.default)[axis]))
}

/// What a signal can move on a node: its numbers (a vec2 by axis) and its colors (by their hue),
/// by mapping key, as its inspector names them.
fn mappable(n: &velo_core::Node) -> Vec<(String, String)> {
    use velo_engine::ParamType;
    let Some(shader) = crate::live::shader(&n.kind, n.code.as_deref()) else { return Vec::new() };
    shader
        .controls
        .iter()
        .filter(|(p, _)| (matches!(p.ty, ParamType::Float | ParamType::Int | ParamType::Vec2) && p.names.is_empty()) || p.ty == ParamType::Color)
        .flat_map(|(p, infos)| {
            infos.iter().enumerate().map(move |(a, info)| {
                let key = if p.ty == ParamType::Vec2 { format!("{}.{}", p.id, ["x", "y"][a]) } else { p.id.clone() };
                (key, if p.ty == ParamType::Color { tr!("{} (hue)", i18n::t(info.name)) } else { i18n::t(info.name).to_string() })
            })
        })
        .collect()
}

/// Text sizes that follow the zoom, in half points: glyphs get rasterized per size, so a
/// smooth zoom shouldn't ask for a new one every frame.
fn font(size: f32) -> FontId {
    FontId::proportional((size * 2.0).round() / 2.0)
}

/// Each node's tint: its first input's, a little further round the wheel, so a chain reads as
/// one image flowing through (until the engine draws real viewers). Chains start from their
/// generator's kind (two Noises look alike, a Gradient differs), nudged by its id.
fn hues(scene: &Scene, index: &HashMap<&str, usize>) -> Vec<f32> {
    let mut feed = vec![None; scene.nodes.len()];
    for e in scene.edges.iter().filter(|e| e.to.1 == 0) {
        if let (Some(&a), Some(&b)) = (index.get(e.from.0.as_str()), index.get(e.to.0.as_str())) {
            feed[b] = Some(a);
        }
    }
    (0..scene.nodes.len())
        .map(|i| {
            let (mut at, mut steps) = (i, 0);
            // a Feedback loop has no start: stop going round it
            while let Some(up) = feed[at]
                && steps < 16
            {
                at = up;
                steps += 1;
            }
            let root = &scene.nodes[at];
            let kind = NODE_KINDS.iter().position(|k| k.id == root.kind).unwrap_or(0);
            (kind as f32 * 0.618_034 + 0.08 * widgets::hue_of(&root.id) + 0.06 * steps as f32).fract()
        })
        .collect()
}

/// A rounded rectangle washed from `a` (top-left) to `b` (bottom-right): a fan from the center.
fn wash(painter: &Painter, r: Rect, radius: f32, (a, b): (egui::Color32, egui::Color32)) {
    let at = |p: Pos2| Theme::mix(a, b, (p.x - r.left()) / r.width() * 0.6 + (p.y - r.top()) / r.height() * 0.4);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(r.center(), at(r.center()));
    let radius = radius.min(r.width() / 2.0).min(r.height() / 2.0);
    let corners = [r.right_bottom() + vec2(-radius, -radius), r.left_bottom() + vec2(radius, -radius), r.left_top() + vec2(radius, radius), r.right_top() + vec2(-radius, radius)];
    const STEPS: usize = 4;
    for (c, corner) in corners.into_iter().enumerate() {
        for s in 0..=STEPS {
            let angle = std::f32::consts::FRAC_PI_2 * (c as f32 + s as f32 / STEPS as f32);
            let p = corner + vec2(angle.cos(), angle.sin()) * radius;
            mesh.colored_vertex(p, at(p));
        }
    }
    let last = mesh.vertices.len() as u32 - 1;
    for i in 1..last {
        mesh.add_triangle(0, i, i + 1);
    }
    mesh.add_triangle(0, last, 1);
    painter.add(Shape::mesh(mesh));
}

/// A bubble round a port, and the end of a wire drawn to it: one blob while they're near, a neck
/// between them thinning as they part until it snaps (Hiroyuki Sato's metaballs, from Paper.js'
/// examples, MIT).
fn goo(painter: &Painter, (c1, r1): (Pos2, f32), tip: Option<(Pos2, f32)>, color: egui::Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    // how far apart, in radii, the neck still holds
    const HOLD: f32 = 1.9;
    let (fill, stroke) = (color.gamma_multiply(0.2), Stroke::new(1.0, color.gamma_multiply(0.7)));
    let neck = tip.map(|(c2, r2)| (c2, r2, c1.distance(c2))).filter(|&(_, r2, d)| r1 >= 1.0 && d > (r1 - r2).abs() && d < (r1 + r2) * HOLD);
    let Some((c2, r2, d)) = neck else {
        if r1 >= 0.5 {
            painter.circle(c1, r1, fill, stroke);
        }
        return;
    };
    // drawn along the line between the two (x) and across it (y): the top half, mirrored
    let (u1, u2) = if d < r1 + r2 { (((r1 * r1 + d * d - r2 * r2) / (2.0 * r1 * d)).clamp(-1.0, 1.0).acos(), ((r2 * r2 + d * d - r1 * r1) / (2.0 * r2 * d)).clamp(-1.0, 1.0).acos()) } else { (0.0, 0.0) };
    // the neck's spread: wide while they touch, none as it snaps
    let v = 0.5 * (1.0 - ((d - r1 - r2) / ((r1 + r2) * (HOLD - 1.0))).clamp(0.0, 1.0));
    let spread = ((r1 - r2) / d).clamp(-1.0, 1.0).acos();
    let (a1, a2) = (u1 + (spread - u1) * v, PI - u2 - (PI - u2 - spread) * v);
    let on = |c: Vec2, r: f32, a: f32| c + vec2(a.cos(), a.sin()) * r;
    let (p1, p2) = (on(Vec2::ZERO, r1, a1), on(vec2(d, 0.0), r2, a2));
    let handle = (v * 2.4).min((p1 - p2).length() / (r1 + r2)) * (d * 2.0 / (r1 + r2)).min(1.0);
    let (h1, h2) = (on(p1, r1 * handle, a1 - FRAC_PI_2), on(p2, r2 * handle, a2 + FRAC_PI_2));
    // round the bubble's back to the neck, the neck, round the tip's front
    let mut top: Vec<Vec2> = (0..=24).map(|s| on(Vec2::ZERO, r1, PI + (a1 - PI) * s as f32 / 24.0)).collect();
    top.extend((1..12).map(|s| {
        let (k, u) = (s as f32 / 12.0, 1.0 - s as f32 / 12.0);
        p1 * (u * u * u) + h1 * (3.0 * u * u * k) + h2 * (3.0 * u * k * k) + p2 * (k * k * k)
    }));
    top.extend((0..=8).map(|s| on(vec2(d, 0.0), r2, a2 * (1.0 - s as f32 / 8.0))));
    // a strip across: every x once, so no triangle overlaps another
    let mut x = f32::MIN;
    for p in &mut top {
        x = x.max(p.x);
        (p.x, p.y) = (x, p.y.max(0.0));
    }
    let ex = (c2 - c1) / d;
    let at = |p: &Vec2, side: f32| c1 + ex * p.x + ex.rot90() * (p.y * side);
    let mut mesh = Mesh::default();
    for p in &top {
        mesh.colored_vertex(at(p, 1.0), fill);
        mesh.colored_vertex(at(p, -1.0), fill);
    }
    for i in 0..top.len() as u32 - 1 {
        mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
        mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
    }
    painter.add(Shape::mesh(mesh));
    let edge = top.iter().map(|p| at(p, 1.0)).chain(top[1..top.len() - 1].iter().rev().map(|p| at(p, -1.0)));
    painter.add(Shape::closed_line(edge.collect(), stroke));
}

/// A card's outline from `from`, on its left or right edge, round and back: what lights run along.
fn outline(r: Rect, radius: f32, from: Pos2) -> Vec<Pos2> {
    let corners = [r.right_bottom() + vec2(-radius, -radius), r.left_bottom() + vec2(radius, -radius), r.left_top() + vec2(radius, radius), r.right_top() + vec2(-radius, radius)];
    // clockwise: down from the right edge, up from the left
    let first = if from.x < r.center().x { 2 } else { 0 };
    let round = (0..4).flat_map(move |c| {
        let c = (c + first) % 4;
        (0..=4).map(move |s| {
            let a = std::f32::consts::FRAC_PI_2 * (c as f32 + s as f32 / 4.0);
            corners[c] + vec2(a.cos(), a.sin()) * radius
        })
    });
    std::iter::once(from).chain(round).chain(std::iter::once(from)).collect()
}

/// The dot grid: one mesh of tiny squares, sparser as you zoom out.
fn grid(painter: &Painter, t: &Theme, rect: Rect, origin: Pos2, zoom: f32) {
    let mut step = GRID * zoom;
    while step < 14.0 {
        step *= 2.0;
    }
    while step > 56.0 {
        step /= 2.0;
    }
    let start = |o: f32, edge: f32| o + ((edge - o) / step).floor() * step;
    let color = Theme::mix(t.bg_base, t.fg_muted, 0.22);
    let mut mesh = Mesh::default();
    let mut y = start(origin.y, rect.top());
    while y <= rect.bottom() {
        let mut x = start(origin.x, rect.left());
        while x <= rect.right() {
            mesh.add_colored_rect(Rect::from_center_size(pos2(x, y), vec2(1.5, 1.5)), color);
            x += step;
        }
        y += step;
    }
    painter.add(Shape::mesh(mesh));
}

/// A port's name in a small pill beside it, outside the card.
fn port_label(painter: &Painter, t: &Theme, p: Pos2, text: &str, output: bool) {
    let galley = painter.layout_no_wrap(text.to_string(), Theme::mono(t.text_sm), t.fg_text);
    let size = galley.size() + vec2(10.0, 4.0);
    let x = if output { p.x + 10.0 } else { p.x - 10.0 - size.x };
    let pill = Rect::from_min_size(pos2(x, p.y - size.y / 2.0), size);
    painter.rect_filled(pill, CornerRadius::same(t.radius_sm / 2), t.bg_control);
    painter.galley(pill.min + vec2(5.0, 2.0), galley, t.fg_text);
}

/// A node as the canvas draws it this frame.
struct Look<'a> {
    name: &'a str,
    kind: Option<&'a NodeKind>,
    selected: bool,
    off: bool,
    hue: f32,
    /// What the viewer shows instead of its tint: the engine's preview (the Output node).
    picture: Option<(egui::TextureId, Rect)>,
    /// No shader yet: a "coming soon" badge on the viewer.
    soon: bool,
    /// Its parameters' chevron (hovered or selected), and whether they're dropped down.
    props: Option<bool>,
    /// Its outputs listed as rows instead of a viewer.
    rows: bool,
    /// Its emoji, or none.
    icon: &'a str,
    /// How many mappings move it, and how much the strongest of their sources reads now.
    maps: usize,
    glow: f32,
    /// What it cost the GPU lately, in ms, where the GPU says.
    cost: Option<f32>,
    /// Nothing it feeds shows now: it sleeps, its picture the last it drew.
    asleep: bool,
    /// The card with its jacks in its foot: what its fill, its border and its glow go round.
    shell: Rect,
}

/// An LFO's card: its shape over one cycle, and a dot where it is now.
fn paint_lfo(painter: &Painter, t: &Theme, r: Rect, zoom: f32, lfo: &velo_engine::signal::Lfo, beats: f64) {
    let viewer = viewer_rect(r, zoom);
    painter.rect_filled(viewer, 0, t.bg_field);
    let area = viewer.shrink(8.0 * zoom);
    // the curve against the cycle: x is the way through it, at the shape's value
    let cycle = f64::from(lfo.beats);
    let start = (beats / cycle).floor() * cycle - f64::from(lfo.phase) * cycle;
    let point = |x: f32| pos2(area.left() + x * area.width(), area.bottom() - lfo.at(start + f64::from(x) * cycle) * area.height());
    let points: Vec<Pos2> = (0..=64).map(|k| point(k as f32 / 64.0)).collect();
    painter.add(Shape::line(points, Stroke::new(1.5 * zoom.sqrt(), t.cat_signal)));
    let now = ((beats - start) / cycle).rem_euclid(1.0) as f32;
    painter.circle_filled(point(now), 3.5 * zoom.sqrt(), t.fg_text);
}

/// One listed output as its card draws it: its name, whether it's a hit (kick, snare, hat,
/// beat), what it reads now and lately.
struct Row<'a> {
    name: &'a str,
    hit: bool,
    /// From the drum model: a badge after its name.
    model: bool,
    now: f32,
    trail: Option<&'a VecDeque<f32>>,
}

/// A card's outputs as rows filling all of it between its header and its footer: a graph a row
/// with a line between each, its last second and a half as a filled curve running to its port, a
/// dot that swells with it, its name inside at the top left; a hit lights its row up. Returns
/// where the model's badges went (their hover says what they mean).
fn paint_rows(painter: &Painter, t: &Theme, r: Rect, zoom: f32, deck: f32, rows: &[Row]) -> Vec<Rect> {
    let mut badges = Vec::new();
    let top = r.top() + (HEADER_H + deck) * zoom;
    let panel = Rect::from_min_max(pos2(r.left() + 1.0, top), pos2(r.right() - 1.0, top + rows.len() as f32 * ROW_H * zoom));
    painter.rect_filled(panel, 0, t.bg_field);
    for (k, row) in rows.iter().enumerate() {
        let (y, v) = (row_y(r, zoom, deck, k), row.now.clamp(0.0, 1.0));
        let band = Rect::from_min_max(pos2(panel.left(), y - ROW_H * zoom / 2.0), pos2(panel.right(), y + ROW_H * zoom / 2.0));
        if k > 0 {
            painter.hline(band.x_range(), band.top(), Stroke::new(1.0, t.line));
        }
        if row.hit && v > 0.05 {
            painter.rect_filled(band, 0, t.cat_signal.gamma_multiply(0.18 * v));
        }
        // the curve, oldest at the card's left edge, now at its right edge where the port is
        let area = Rect::from_min_max(pos2(band.left(), band.top() + 6.0 * zoom), pos2(band.right(), band.bottom() - 1.0));
        let trail: Vec<f32> = row.trail.map(|tr| tr.iter().copied().collect()).unwrap_or_default();
        if trail.len() >= 2 {
            let step = area.width() / (TRAIL - 1) as f32;
            let start = area.right() - step * (trail.len() - 1) as f32;
            let line: Vec<Pos2> = trail.iter().enumerate().map(|(i, x)| pos2(start + step * i as f32, area.bottom() - x.clamp(0.0, 1.0) * area.height())).collect();
            let mut fill = Mesh::default();
            for (i, p) in line.iter().enumerate() {
                fill.colored_vertex(*p, t.cat_signal.gamma_multiply(0.3));
                fill.colored_vertex(pos2(p.x, area.bottom()), t.cat_signal.gamma_multiply(0.03));
                if i > 0 {
                    let b = 2 * i as u32;
                    fill.add_triangle(b - 2, b - 1, b);
                    fill.add_triangle(b - 1, b + 1, b);
                }
            }
            painter.add(fill);
            painter.add(Shape::line(line, Stroke::new(1.3 * zoom.sqrt(), t.cat_signal)));
        }
        let now = pos2(area.right(), area.bottom() - v * area.height());
        painter.circle_filled(now, (1.5 + 2.5 * v) * zoom.sqrt(), Theme::mix(t.cat_signal, t.fg_text, 0.6 * v));
        // its name inside, at the top left
        if zoom >= TEXT_ZOOM {
            // on a pill of the panel's color, readable over the curve
            let galley = painter.layout_no_wrap(row.name.to_string(), FontId::proportional(10.5 * zoom), Theme::mix(t.fg_muted, t.fg_text, 0.4 + 0.6 * v));
            let name = Rect::from_min_size(band.left_top() + vec2(8.0, 5.0) * zoom, galley.size());
            painter.rect_filled(name.expand2(vec2(3.0, 1.0) * zoom), CornerRadius::same((3.0 * zoom) as u8), t.bg_field.gamma_multiply(0.85));
            painter.galley(name.min, galley, t.fg_text);
            if row.model {
                let text = painter.text(pos2(name.right() + 4.0 * zoom, name.center().y), Align2::LEFT_CENTER, widgets::MODEL_BADGE, Theme::mono(7.0 * zoom), t.fg_muted);
                let badge = text.expand2(vec2(2.0, 1.0) * zoom);
                painter.rect_stroke(badge, CornerRadius::same((2.0 * zoom) as u8), Stroke::new(1.0, t.line), StrokeKind::Inside);
                badges.push(badge);
            }
        }
    }
    badges
}

/// What an Audio In's deck shows.
struct Deck<'a> {
    /// The reels' angle: they turn faster as it's louder.
    reel: f32,
    /// How far the tape has run from the left reel to the right one (0..1).
    tape: f32,
    level: f32,
    /// The loudest lately, falling slowly.
    peak: f32,
    /// What it listens to, and the tempo with its beat's hit (0..1).
    source: &'a str,
    bpm: Option<(f32, f32)>,
}

/// An Audio In's tape deck under its header, on the card itself: a cassette drawn in a divider's
/// grey, a real one's proportions (its body, the label with its orange band and ruled lines, the
/// window, the head opening along its foot with its pins and pad), two reels passing the tape
/// from one to the other, their teeth turning with the level, the tape between them trembling
/// with it, the band brightening as it's loud and a glint sweeping the window now and then.
/// Beside it what it listens to, the tempo pulsing on the beat, and the level as green bars with
/// a peak that falls slowly.
fn paint_deck(painter: &Painter, t: &Theme, room: Rect, zoom: f32, deck: &Deck) {
    const ORANGE: egui::Color32 = crate::theme::TE_ORANGE;
    let (z, level) = (zoom, deck.level.clamp(0.0, 1.0));
    // a grid of units over the cassette: its body runs from (3, 5) to (21, 16.5), about a real
    // one's 100 by 63 mm
    let u = 5.4 * z;
    let origin = room.left_top() + vec2(14.0 * z - 3.0 * u, 11.0 * z - 5.0 * u);
    let at = |x: f32, y: f32| origin + vec2(x, y) * u;
    let grey = Theme::mix(t.line, t.fg_muted, 0.55);
    let (line, thin) = (Stroke::new(2.0 * z, grey), Stroke::new(1.0 * z, Theme::mix(t.line, t.fg_muted, 0.3)));
    let body = Rect::from_min_max(at(3.0, 5.0), at(21.0, 16.5));
    painter.rect_filled(body, CornerRadius::same((1.4 * u) as u8), Theme::mix(t.bg_card, t.fg_text, 0.03));
    // the label: a band lit as it's loud, the name, two ruled lines to write on
    let label = Rect::from_min_max(at(4.2, 6.0), at(19.8, 12.6));
    painter.rect_filled(label, CornerRadius::same((0.8 * u) as u8), Theme::mix(t.bg_card, t.fg_text, 0.06));
    painter.rect_stroke(label, CornerRadius::same((0.8 * u) as u8), thin, StrokeKind::Inside);
    let band = Rect::from_min_max(at(4.2, 6.0), at(19.8, 7.1));
    let r = (0.8 * u) as u8;
    painter.rect_filled(band, CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, Theme::mix(Theme::mix(ORANGE, t.bg_card, 0.25), ORANGE, level));
    if zoom >= TEXT_ZOOM {
        painter.text(at(5.0, 6.55), Align2::LEFT_CENTER, "VELO", FontId::monospace(0.85 * u), t.bg_base);
        painter.text(at(19.0, 6.55), Align2::RIGHT_CENTER, "A", FontId::monospace(0.85 * u), t.bg_base);
    }
    for y in [7.8, 12.0] {
        painter.hline(at(5.2, y).x..=at(18.8, y).x, at(0.0, y).y, thin);
    }
    // the window, the tape wound on each reel (seen through it), the tape running between
    let window = Rect::from_min_max(at(6.2, 8.4), at(17.8, 11.2));
    painter.rect_filled(window, CornerRadius::same((1.4 * u) as u8), t.bg_base);
    let (a, b) = (at(8.4, 9.8), at(15.6, 9.8));
    // (each reel's tape from its hub to the window's height)
    let (wound_a, wound_b) = ((0.95 + 0.45 * (1.0 - deck.tape)) * u, (0.95 + 0.45 * deck.tape) * u);
    let inside = painter.with_clip_rect(window.intersect(painter.clip_rect()));
    inside.circle_filled(a, wound_a, ORANGE);
    inside.circle_filled(b, wound_b, Theme::mix(ORANGE, t.bg_base, 0.25));
    let wave: Vec<Pos2> = (0..=24)
        .map(|s| {
            let k = s as f32 / 24.0;
            let x = a.x + (b.x - a.x) * k;
            let y = a.y + wound_a + (wound_b - wound_a) * k;
            pos2(x, y + (k * std::f32::consts::PI).sin() * (deck.reel * 3.0 + k * 20.0).sin() * level * 0.3 * u)
        })
        .collect();
    inside.add(Shape::line(wave, Stroke::new(1.2 * z, Theme::mix(ORANGE, t.fg_text, 0.3))));
    // a glint sweeping the window every few turns
    let sweep = (deck.reel / std::f32::consts::TAU * 0.5).fract() * 2.0 - 0.5;
    let gx = window.left() + window.width() * sweep;
    let glint = vec![pos2(gx, window.top()), pos2(gx + 1.2 * u, window.top()), pos2(gx + 0.4 * u, window.bottom()), pos2(gx - 0.8 * u, window.bottom())];
    inside.add(Shape::convex_polygon(glint, t.fg_text.gamma_multiply(0.07), Stroke::NONE));
    painter.rect_stroke(window, CornerRadius::same((1.4 * u) as u8), thin, StrokeKind::Inside);
    // the hubs, six teeth each, turning
    for c in [a, b] {
        painter.circle_filled(c, 0.75 * u, t.bg_card);
        for s in 0..6 {
            let d = egui::Vec2::angled(deck.reel + s as f32 * std::f32::consts::TAU / 6.0);
            painter.line_segment([c + d * 0.38 * u, c + d * 0.72 * u], Stroke::new(1.1 * z, grey));
        }
        painter.circle_stroke(c, 0.75 * u, Stroke::new(1.4 * z, grey));
    }
    // the head opening along the foot: its slope, two pins, the pad in the middle
    painter.add(Shape::line(vec![at(5.4, 16.5), at(6.9, 13.6), at(17.1, 13.6), at(18.6, 16.5)], line));
    for x in [8.4, 15.6] {
        painter.rect_stroke(Rect::from_center_size(at(x, 15.0), vec2(0.8, 0.8) * u), CornerRadius::same((0.2 * u) as u8), thin, StrokeKind::Middle);
    }
    painter.rect_stroke(Rect::from_min_max(at(10.9, 14.3), at(13.1, 15.7)), CornerRadius::same((0.3 * u) as u8), thin, StrokeKind::Middle);
    // the outline over it all
    painter.rect_stroke(body, CornerRadius::same((1.4 * u) as u8), line, StrokeKind::Middle);
    // beside it: the source and the tempo, the level, its figure
    let (left, right) = (body.right() + 22.0 * z, room.right() - 16.0 * z);
    let mono = FontId::monospace(t.text_xs * z);
    if zoom >= TEXT_ZOOM {
        let top = room.top() + 15.0 * z;
        let tempo = deck.bpm.map(|(bpm, hit)| {
            let ink = Theme::mix(t.fg_text, t.cat_signal, hit.clamp(0.0, 1.0));
            painter.text(pos2(right, top), Align2::RIGHT_CENTER, format!("{bpm:.1} BPM"), mono.clone(), ink)
        });
        let room_left = tempo.map_or(right, |r| r.left() - 10.0 * z) - left;
        kit::fit_text(painter, pos2(left, top), Align2::LEFT_CENTER, deck.source.to_uppercase(), mono.clone(), t.fg_muted, room_left);
    }
    let meter = Rect::from_min_max(pos2(left, room.top() + 27.0 * z), pos2(right, room.top() + 55.0 * z));
    let (n, gap) = (28, 2.5 * z);
    let w = ((meter.width() - gap * (n - 1) as f32) / n as f32).max(1.0);
    let lit_to = meter.left() + meter.width() * level;
    if level > 0.02 {
        painter.rect_filled(Rect::from_min_max(meter.left_top() - vec2(3.0, 3.0) * z, pos2(lit_to + 3.0 * z, meter.bottom() + 3.0 * z)), CornerRadius::same((3.0 * z) as u8), t.cat_signal.gamma_multiply(0.12));
    }
    let peak = ((deck.peak.clamp(0.0, 1.0) * n as f32).ceil() as usize).saturating_sub(1);
    for s in 0..n {
        let x = meter.left() + s as f32 * (w + gap);
        // bars rising to the right, as a level does
        let h = meter.height() * (0.45 + 0.55 * s as f32 / (n - 1) as f32);
        let bar = Rect::from_min_max(pos2(x, meter.bottom() - h), pos2(x + w, meter.bottom()));
        let lit = (s as f32 + 0.5) / n as f32 <= level;
        let color = if lit {
            Theme::mix(t.cat_signal, t.fg_text, 0.35 * s as f32 / n as f32)
        } else if s == peak && deck.peak > 0.02 {
            t.fg_text
        } else {
            t.line.gamma_multiply(0.8)
        };
        painter.rect_filled(bar, CornerRadius::same((w / 2.0).min(2.0 * z) as u8), color);
    }
    if zoom >= TEXT_ZOOM {
        let foot = room.top() + 68.0 * z;
        painter.text(pos2(left, foot), Align2::LEFT_CENTER, "LEVEL", mono.clone(), t.fg_muted);
        painter.text(pos2(right, foot), Align2::RIGHT_CENTER, format!("{:>3.0}%", level * 100.0), mono, Theme::mix(t.fg_muted, t.fg_text, level));
    }
}

/// A signal plugged into a card, as its jack shows it.
struct Jack {
    /// Where it's from and what it moves.
    label: String,
    /// What comes through now (0..1).
    v: f32,
    /// Its cable pointed at or chosen, eased (0..1).
    hot: f32,
    /// Just plugged in: how far its ring has spread (0..1), 1 for long ago.
    ring: f32,
}

/// The jacks in a card's foot, a line each for a signal plugged in: its socket on the card's
/// left edge where the cable goes in (an input port's, with a hole and a pin lit by what comes
/// through, larger while its cable is pointed at), where it's from and what it moves. `slot`
/// (0..1): a new line sliding out for a signal dragged onto the card, its socket open.
fn paint_jacks(painter: &Painter, t: &Theme, strip: Rect, zoom: f32, lines: &[Jack], slot: f32) {
    let socket_at = |k: usize, hot: f32| (jack_socket(strip, zoom, k), (4.5 * zoom).max(2.0) + 1.5 * hot);
    for (k, jack) in lines.iter().enumerate() {
        let ((socket, r), v) = (socket_at(k, jack.hot), jack.v.clamp(0.0, 1.0));
        if k > 0 {
            painter.hline(strip.x_range(), strip.top() + k as f32 * JACK_H * zoom, Stroke::new(1.0, t.line.gamma_multiply(0.6)));
        }
        painter.circle_filled(socket, r, Theme::mix(Theme::mix(t.line, t.cat_signal, 0.75), t.fg_text, 0.4 * jack.hot));
        painter.circle_filled(socket, r * 0.5, t.bg_base);
        painter.circle_filled(socket, r * 0.22, Theme::mix(t.fg_muted, t.cat_signal, 0.3 + 0.7 * v));
        painter.circle_stroke(socket, r, Stroke::new(1.5, Theme::mix(t.cat_signal, t.fg_text, 0.5 * jack.hot)));
        if jack.ring < 1.0 {
            painter.circle_stroke(socket, r + 14.0 * zoom * jack.ring, Stroke::new(1.5, t.cat_signal.gamma_multiply(1.0 - jack.ring)));
        }
        if zoom >= TEXT_ZOOM {
            let left = socket.x + 12.0 * zoom;
            let ink = Theme::mix(Theme::mix(t.fg_muted, t.fg_text, 0.3 + 0.7 * v), t.fg_text, jack.hot);
            kit::fit_text(painter, pos2(left, socket.y), Align2::LEFT_CENTER, &jack.label, FontId::proportional(10.5 * zoom), ink, strip.right() - 8.0 * zoom - left);
        }
    }
    // the open slot: a ring waiting for the plug, a line saying what it does
    if slot > 0.01 {
        let (socket, r) = socket_at(lines.len(), slot);
        if !lines.is_empty() {
            painter.hline(strip.x_range(), strip.top() + lines.len() as f32 * JACK_H * zoom, Stroke::new(1.0, t.line.gamma_multiply(0.6 * slot)));
        }
        painter.circle_filled(socket, r, t.bg_base.gamma_multiply(slot));
        painter.circle_stroke(socket, r, Stroke::new(1.5, t.cat_signal.gamma_multiply(slot)));
        if zoom >= TEXT_ZOOM {
            let left = socket.x + 12.0 * zoom;
            kit::fit_text(painter, pos2(left, socket.y), Align2::LEFT_CENTER, tr!("Plug in to move one of its values"), FontId::proportional(10.5 * zoom), t.fg_muted.gamma_multiply(slot), strip.right() - 8.0 * zoom - left);
        }
    }
}

/// A signal node's inputs (muted) and outputs as bars, side by side.
fn paint_meters(painter: &Painter, t: &Theme, r: Rect, zoom: f32, values: &[f32], inputs: usize) {
    let viewer = viewer_rect(r, zoom);
    painter.rect_filled(viewer, 0, t.bg_field);
    let area = viewer.shrink(8.0 * zoom);
    let (gap, n) = (4.0 * zoom, values.len() as f32);
    let w = (area.width() - gap * (n - 1.0)) / n;
    for (k, v) in values.iter().enumerate() {
        let x = area.left() + k as f32 * (w + gap);
        let bar = Rect::from_min_max(pos2(x, area.bottom() - v.clamp(0.0, 1.0) * area.height()), pos2(x + w, area.bottom()));
        painter.rect_filled(Rect::from_min_max(pos2(x, area.top()), bar.max), CornerRadius::same((2.0 * zoom) as u8), t.line.gamma_multiply(0.5));
        let color = if k < inputs { t.fg_muted.gamma_multiply(0.5) } else { t.cat_signal };
        painter.rect_filled(bar, CornerRadius::same((2.0 * zoom) as u8), color);
    }
}

/// A card's viewer, all of its content: its picture (16:9, cropped to fill a taller card), or its
/// tint (and "coming soon" on a node that doesn't render).
fn viewer_paint(ui: &Ui, painter: &Painter, t: &Theme, viewer: Rect, zoom: f32, look: &Look) {
    match look.picture {
        _ if look.off => {
            painter.rect_filled(viewer, 0, t.bg_field);
        }
        Some((id, uv)) => {
            let k = (viewer.width() / viewer.height()) / (16.0 / 9.0);
            let uv = if k < 1.0 { Rect::from_center_size(uv.center(), vec2(uv.width() * k, uv.height())) } else { Rect::from_center_size(uv.center(), vec2(uv.width(), uv.height() / k)) };
            egui::Image::new((id, viewer.size())).uv(uv).paint_at(ui, viewer);
        }
        None => wash(painter, viewer, 0.0, widgets::wash(look.hue)),
    }
    // asleep: its last picture, dimmed, saying why it doesn't move
    if look.asleep {
        painter.rect_filled(viewer, 0, t.bg_base.gamma_multiply(0.6));
        if zoom >= TEXT_ZOOM {
            painter.text(viewer.center(), Align2::CENTER_CENTER, tr!("Not shown now"), font(t.text_sm * zoom), t.fg_muted);
        }
    }
    if look.soon && !look.off && zoom >= TEXT_ZOOM {
        let text = painter.layout_no_wrap(tr!("Coming soon").to_string(), font(t.text_sm * zoom), t.fg_text);
        let badge = Rect::from_center_size(viewer.center(), text.size() + vec2(12.0, 6.0) * zoom);
        painter.rect_filled(badge, CornerRadius::same((badge.height() / 2.0) as u8), t.bg_card.gamma_multiply(0.9));
        painter.galley(badge.center() - text.size() / 2.0, text, t.fg_text);
    }
}

fn paint_node(ui: &Ui, painter: &Painter, t: &Theme, r: Rect, zoom: f32, look: Look) {
    let Look { name, kind, .. } = look;
    let radius = (f32::from(t.radius_md) * zoom).clamp(2.0, 255.0) as u8;
    let corner = CornerRadius::same(radius);
    let cat = kind.map_or(t.fg_muted, |k| t.category(k.category));
    // moving with a signal: a glow as strong as what moves it
    let (glow, shell) = (look.glow.clamp(0.0, 1.0), look.shell);
    let jacks = shell.bottom() > r.bottom() + 0.5;
    if glow > 0.02 {
        painter.rect_stroke(shell.expand(1.0), CornerRadius::same(radius.saturating_add(1)), Stroke::new((2.0 + 7.0 * glow) * zoom.sqrt(), t.cat_signal.gamma_multiply(0.22 * glow)), StrokeKind::Outside);
    }
    painter.rect_filled(shell, corner, t.bg_card);
    // header, content, footer (and the jacks under it): the header tinted with the category, a
    // divider under each part
    let header = Rect::from_min_size(r.min, vec2(r.width(), HEADER_H * zoom));
    let footer = Rect::from_min_max(pos2(r.left(), r.bottom() - FOOTER_H * zoom), r.max);
    let foot = if jacks { 0 } else { radius };
    painter.rect_filled(header, CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 }, Theme::mix(t.bg_card, cat, 0.07));
    painter.rect_filled(footer, CornerRadius { nw: 0, ne: 0, sw: foot, se: foot }, Theme::mix(t.bg_card, t.bg_field, 0.5));
    painter.hline(r.x_range(), header.bottom(), Stroke::new(1.0, Theme::mix(t.line, cat, 0.3)));
    painter.hline(r.x_range(), footer.top(), Stroke::new(1.0, t.line));
    if jacks {
        painter.hline(r.x_range(), footer.bottom(), Stroke::new(1.0, t.line));
    }
    painter.rect_stroke(shell, corner, Stroke::new(1.0, t.line), StrokeKind::Inside);
    kit::spotlight(ui, t, shell, radius);
    // the header: its emoji (or its category's dot) and its name
    let mark = pos2(header.left() + 13.0 * zoom, header.center().y);
    let emoji = !look.icon.is_empty() && zoom >= TEXT_ZOOM;
    if emoji {
        painter.text(mark, Align2::CENTER_CENTER, look.icon, FontId::proportional(13.0 * zoom), t.fg_text);
    } else {
        painter.circle_filled(mark, 3.5 * zoom, cat);
    }
    if zoom >= TEXT_ZOOM {
        let left = mark.x + if emoji { 12.0 } else { 9.0 } * zoom;
        let right = if kind.is_some_and(|k| k.category == velo_core::Category::Output) { header.right() - 10.0 * zoom } else { power_button(r, zoom).left() - 4.0 * zoom };
        kit::fit_text(painter, pos2(left, header.center().y), Align2::LEFT_CENTER, name, font(t.text_md * zoom), t.fg_text, right - left);
        // the footer: the kind, what moves it, the parameters' chevron
        let mut right = footer.right() - 8.0 * zoom;
        if let Some(open) = look.props {
            let b = props_button(r, zoom);
            let angle = if open { std::f32::consts::PI } else { 0.0 };
            kit::icon_turned(painter, b.center(), icons::CARET_DOWN, 13.0 * zoom, if open { t.accent } else { t.fg_muted }, angle);
            right = b.left() - 2.0 * zoom;
        }
        if look.maps > 0 {
            let text = format!("{} {}", icons::WAVE_SINE, look.maps);
            let at = painter.text(pos2(right, footer.center().y), Align2::RIGHT_CENTER, text, FontId::proportional(10.5 * zoom), Theme::mix(t.fg_muted, t.cat_signal, 0.5 + 0.5 * glow));
            right = at.left() - 6.0 * zoom;
        }
        // what it costs the GPU: quiet while it's light, amber past 1 ms, red past 4
        if let Some(ms) = look.cost.filter(|ms| *ms >= 0.05) {
            let color = if ms >= 4.0 {
                t.error
            } else if ms >= 1.0 {
                t.warn
            } else {
                t.fg_muted.gamma_multiply(0.7)
            };
            let at = painter.text(pos2(right, footer.center().y), Align2::RIGHT_CENTER, format!("{ms:.1} ms"), FontId::monospace(t.text_xs * zoom), color);
            right = at.left() - 6.0 * zoom;
        }
        let kind = kind.map_or("?", |k| if name == k.label || name == i18n::t(k.label) { i18n::t(crate::role_of(k)) } else { i18n::t(k.label) }).to_uppercase();
        let left = footer.left() + 10.0 * zoom;
        kit::fit_text(painter, pos2(left, footer.center().y), Align2::LEFT_CENTER, &kind, FontId::monospace(t.text_xs * zoom), t.fg_muted, right - left);
    }
    if !look.rows {
        viewer_paint(ui, painter, t, viewer_rect(r, zoom), zoom, &look);
    }
    if look.selected {
        painter.rect_stroke(shell, corner, Stroke::new(1.5, t.accent), StrokeKind::Outside);
        painter.rect_stroke(shell.expand(1.5), CornerRadius::same(radius.saturating_add(2)), Stroke::new(3.0, t.accent.gamma_multiply(0.15)), StrokeKind::Outside);
    }
}

/// A port: filled with its wire's color when wired, a hollow ring when not. Under the pointer
/// (`hot`, eased 0..1) it grows and lights up.
fn paint_port(painter: &Painter, t: &Theme, p: Pos2, zoom: f32, wired: Option<egui::Color32>, hot: f32) {
    let r = (4.0 * zoom).max(1.5 + 1.5 * hot) + 1.5 * hot;
    let ring = Theme::mix(wired.unwrap_or_else(|| Theme::mix(t.line, t.fg_muted, 0.6)), t.accent, hot);
    let fill = wired.map_or(t.bg_card, |c| Theme::mix(t.line, c, 0.85));
    painter.circle_filled(p, r, Theme::mix(fill, t.accent, 0.3 * hot));
    painter.circle_stroke(p, r, Stroke::new(1.5, ring));
}

impl App {
    /// Whether the canvas gets the keyboard: it has the focus, or nothing does and the
    /// pointer is over it.
    pub fn canvas_keys(&self, ctx: &egui::Context) -> bool {
        self.session.active().is_some()
            && match ctx.memory(|m| m.focused()) {
                Some(focused) => focused == id(),
                None => self.canvas.hovered,
            }
    }

    /// The active scene's graph, filling `ui`: the whole window under the bars. The panels
    /// float over it (Figma UI3); `self.canvas.view` is the part they leave uncovered.
    pub fn node_canvas(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        let keys = self.canvas_keys(ui.ctx()) && !self.modal() && !ui.ctx().text_edit_focused();
        let rect = ui.max_rect();
        // a child clipped to the canvas: spotlights and labels never spill onto the bars
        let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));
        ui.set_clip_rect(rect);
        let bg = ui.interact(rect, id(), Sense::click_and_drag());
        let Some(scene) = self.session.active() else { return };
        let picture = self.live.as_ref().and_then(crate::live::Live::picture);
        let thumbs: Vec<Option<(egui::TextureId, Rect)>> = scene.nodes.iter().map(|n| self.live.as_ref().and_then(|l| l.thumb(&n.id))).collect();
        // the picture behind the canvas: no dots over it
        let backdrop = self.settings.preview_background && picture.is_some();
        let st = &mut self.canvas;
        // the uncovered part is known from the second frame: until then, look but don't settle
        let settled = st.view.is_some();
        if !settled {
            ui.ctx().request_repaint();
        }
        let view = st.view.filter(|v| v.width() > 100.0 && v.height() > 100.0).unwrap_or(rect);
        let (now, dt) = ui.input(|i| (i.time, i.stable_dt.min(0.1)));
        let (pointer, shift) = ui.input(|i| (i.pointer.hover_pos(), i.modifiers.shift));
        let space = ui.input(|i| i.key_down(Key::Space)) && !ui.ctx().text_edit_focused();
        // the camera hangs off the whole canvas' center: panels opening and closing over it
        // leave the graph where it is
        let center = rect.center();
        if st.scene.as_deref() != Some(scene.id.as_str()) {
            // another scene: the selection was the old one's (the very first frame keeps what
            // was set up before it, the demo's)
            if st.scene.is_some() {
                (st.selected, st.wire, st.drag, st.menu, st.anim, st.goal) = (Vec::new(), None, None, None, None, None);
            }
            st.scene = Some(scene.id.clone());
        }
        // undo can take nodes and wires away under the selection
        st.selected.retain(|n| scene.node(n).is_some());
        if st.wire.as_ref().is_some_and(|(n, p)| scene.wire_into(n, *p).is_none()) {
            st.wire = None;
        }
        if st.map.as_ref().is_some_and(|(n, s)| !scene.node(n).is_some_and(|n| n.mappings.iter().any(|m| &m.source == s))) {
            st.map = None;
        }
        // not through a panel, the toolbar or the zoom menu over it
        st.hovered = bg.hovered() || bg.dragged();

        // ---- the view ----
        let mut cam = st.cameras.get(&scene.id).copied().unwrap_or_else(|| fit(bounds(scene, None), center, view, 1.0));
        let min = min_zoom(scene, view);
        if let Some(to) = st.zoom_to.take() {
            // keys and the zoom menu ease there; the pointer if it's on the canvas, else the view's center
            let at = pointer.filter(|_| st.hovered).unwrap_or(view.center());
            let target = match to {
                ZoomTo::Fit => fit(bounds(scene, None), center, view, 1.0),
                ZoomTo::Selection => fit(bounds(scene, Some(&st.selected)).or(bounds(scene, None)), center, view, 2.0),
                ZoomTo::Level(z) => cam.zoomed(center, view.center(), z.clamp(min, MAX_ZOOM)),
                ZoomTo::In => cam.zoomed(center, at, step(cam.zoom, true).clamp(min, MAX_ZOOM)),
                ZoomTo::Out => cam.zoomed(center, at, step(cam.zoom, false).clamp(min, MAX_ZOOM)),
            };
            (st.anim, st.goal) = (Some((cam, target, now)), None);
        }
        if let Some(id) = st.reveal.take()
            && let Some(n) = scene.node(&id)
        {
            let r = node_rect(&cam, center, n.pos);
            if !rect.shrink(t.pad * 4.0).contains_rect(r) {
                st.anim = Some((cam, Camera { pan: cam.pan + (center - r.center()), ..cam }, now));
            }
        }
        if let Some((from, to, at)) = st.anim {
            let k = if t.motion > 0.0 { ((now - at) as f32 / (t.motion * 2.0)).min(1.0) } else { 1.0 };
            cam = Camera::lerp(from, to, kit::ease(k));
            if k >= 1.0 {
                st.anim = None;
            } else {
                ui.ctx().request_repaint();
            }
        }
        if st.hovered {
            // pinch and Cmd/Ctrl+scroll (egui's zoom) zoom about the pointer; plain scroll, and
            // two fingers on a touch screen, pan (FigCraft's reading of the input)
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta + i.multi_touch().map_or(Vec2::ZERO, |m| m.translation_delta), i.zoom_delta()));
            if let Some(p) = pointer
                && (zoom - 1.0).abs() > 1e-4
            {
                let goal = st.goal.map_or(cam.zoom, |g| g.0) * zoom;
                (st.goal, st.anim) = (Some((goal.clamp(min, MAX_ZOOM), p)), None);
            } else if scroll != Vec2::ZERO {
                cam.pan += scroll;
                st.anim = None;
            }
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
        }
        // the zoom eases toward where the wheel or the pinch sent it
        if let Some((goal, at)) = st.goal {
            let k = if t.motion > 0.0 { 1.0 - (-dt / ZOOM_EASE).exp() } else { 1.0 };
            let z = cam.zoom + (goal - cam.zoom) * k;
            let done = (goal - z).abs() <= goal * 0.002;
            cam = cam.zoomed(center, at, if done { goal } else { z });
            if done {
                st.goal = None;
            } else {
                ui.ctx().request_repaint();
            }
        }

        // the nodes hidden: the picture behind them alone, the view kept for when they're back
        if st.hidden {
            if settled {
                st.cameras.insert(scene.id.clone(), cam);
            }
            return;
        }

        // ---- where everything is on screen ----
        let nodes = &scene.nodes;
        let kinds: Vec<Option<&NodeKind>> = nodes.iter().map(|n| node_kind(&n.kind)).collect();
        let index: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
        // a card listing its outputs is as tall as its rows; one with many inputs, as tall as
        // they need
        let rows: Vec<Option<Vec<u8>>> = nodes.iter().map(|n| rows(scene, n)).collect();
        let shown: Vec<usize> = nodes.iter().map(|n| velo_core::inputs_shown(scene, n)).collect();
        // nodes just arranged slide from where they were
        let slid = |n: &velo_core::Node| match st.glides.get(&n.id) {
            Some((from, at)) if t.motion > 0.0 => {
                let k = kit::ease(((now - at) as f32 / (t.motion * 3.0)).min(1.0));
                [from[0] + (n.pos[0] - from[0]) * k, from[1] + (n.pos[1] - from[1]) * k]
            }
            _ => n.pos,
        };
        let rects: Vec<Rect> = nodes
            .iter()
            .map(|n| {
                let r = plane_rect(scene, n, slid(n));
                Rect::from_min_size(cam.screen(center, r.min), r.size() * cam.zoom)
            })
            .collect();
        st.glides.retain(|_, (_, at)| now - *at < f64::from(t.motion) * 3.0);
        if !st.glides.is_empty() {
            ui.ctx().request_repaint();
        }
        // a signal's wire dragged over a card, or its value there being picked: a new jack slides
        // out of the card's foot for it (the jack plugged in takes its place at once)
        let signal = |i: usize| kinds[i].is_some_and(|k| k.category == velo_core::Category::Signal);
        let signal_drag = matches!(&st.drag, Some(Drag::Wire { from, .. } | Drag::Signal { from, .. }) if index.get(from.0.as_str()).is_some_and(|&a| signal(a)));
        let opening = match (&st.pick, pointer.filter(|_| signal_drag)) {
            (Some((_, node, _)), _) => index.get(node.as_str()).copied(),
            (None, Some(p)) => (0..nodes.len()).rev().find(|&i| !signal(i) && Rect::from_min_max(rects[i].min, rects[i].max + vec2(0.0, (jacks(&nodes[i]).len() + 1) as f32 * JACK_H * cam.zoom)).contains(p)),
            _ => None,
        };
        let step = if t.motion > 0.0 { dt / t.motion } else { 1.0 };
        let slots: Vec<f32> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let plugged = st.plug_ins.iter().any(|((node, _), at)| node == &n.id && at.is_nan());
                let open = st.slots.entry(n.id.clone()).or_insert(0.0);
                *open = if plugged { 0.0 } else if opening == Some(i) { (*open + step).min(1.0) } else { (*open - step).max(0.0) };
                kit::ease(*open)
            })
            .collect();
        st.slots.retain(|_, open| *open > 0.0);
        if slots.iter().any(|&s| s > 0.0 && s < 1.0) {
            ui.ctx().request_repaint();
        }
        // the jacks in each card's foot, the open slot under them
        let strips: Vec<Rect> = nodes
            .iter()
            .zip(&rects)
            .zip(&slots)
            .map(|((n, r), slot)| Rect::from_min_max(r.left_bottom(), pos2(r.right(), r.bottom() + (jacks(n).len() as f32 + slot) * JACK_H * cam.zoom)))
            .collect();
        let count = |i: usize, output: bool| kinds[i].map_or(0, |k| if output { k.outputs.len() } else { shown[i] });
        // a listed output's port sits on its row; a hidden one isn't there
        let listed = |i: usize, p: u8, output: bool| !output || rows[i].as_ref().is_none_or(|l| l.contains(&p));
        let port = |i: usize, p: u8, output: bool| match (&rows[i], output) {
            (Some(list), true) => pos2(rects[i].right(), row_y(rects[i], cam.zoom, deck_h(&nodes[i].kind), list.iter().position(|&q| q == p).unwrap_or(0))),
            _ => port_pos(rects[i], cam.zoom, count(i, output), p, output),
        };
        // every cable's ends: a wire's by its input, a signal's line by the card it moves and its
        // source (as ties name them)
        let mut ends: HashMap<String, (Pos2, Pos2)> = HashMap::new();
        for e in &scene.edges {
            if let (Some(&a), Some(&b)) = (index.get(e.from.0.as_str()), index.get(e.to.0.as_str())) {
                ends.insert(velo_core::Tie::wire(&e.to), (port(a, e.from.1, true), port(b, e.to.1, false)));
            }
        }
        for (j, n) in nodes.iter().enumerate() {
            for (k, source) in jacks(n).into_iter().enumerate() {
                if let Some(&i) = velo_core::signal_output(source).and_then(|(id, _)| index.get(id)) {
                    let out = velo_core::signal_output(source).map_or(0, |(_, out)| out);
                    ends.insert(velo_core::Tie::line(&n.id, source), (port(i, out, true), jack_socket(strips[j], cam.zoom, k)));
                }
            }
        }
        // the ties on screen, each with its cables still there, in the order they come from (a
        // tie just made starts loose and cinches in)
        let cinching = (t.motion * 2.5).max(1e-3);
        st.cinch.retain(|(_, at)| ((now - at) as f32) < cinching);
        if !st.cinch.is_empty() {
            ui.ctx().request_repaint();
        }
        let ties: Vec<(Pos2, Vec<String>, f32)> = scene
            .ties
            .iter()
            .map(|tie| {
                let mut held: Vec<String> = tie.cables.iter().filter(|c| ends.contains_key(*c)).cloned().collect();
                held.sort_by(|a, b| ends[a].0.y.total_cmp(&ends[b].0.y));
                let loose = st.cinch.iter().find(|(at, _)| *at == tie.pos).map_or(0.0, |(_, at)| 1.0 - kit::ease(((now - at) as f32 / cinching).min(1.0)));
                (cam.screen(center, pos2(tie.pos[0], tie.pos[1])), held, TIE_GAP * cam.zoom * (1.0 + 5.0 * loose))
            })
            .collect();
        // a cable's line, through the ties holding it
        let route = |key: &str, a: Pos2, b: Pos2| {
            let mut through: Vec<Pos2> =
                ties.iter().filter_map(|(at, held, gap)| held.iter().position(|c| c == key).map(|k| *at + vec2(0.0, (k as f32 - (held.len() - 1) as f32 / 2.0) * gap))).collect();
            through.sort_by(|p, q| p.x.total_cmp(&q.x));
            routed(a, b, &through, cam.zoom)
        };
        // every wire on screen as a polyline (off-screen ones: None)
        let wires: Vec<Option<(usize, Vec<Pos2>)>> = scene
            .edges
            .iter()
            .map(|e| {
                let a = *index.get(e.from.0.as_str())?;
                let key = velo_core::Tie::wire(&e.to);
                let (p0, p3) = *ends.get(&key)?;
                let pts = route(&key, p0, p3);
                Rect::from_points(&pts).expand(40.0).intersects(rect).then_some((a, pts))
            })
            .collect();
        // a card with parameters has a chevron in its header: they drop down under it
        let chevron = |i: usize| (cam.zoom >= TEXT_ZOOM && crate::live::shader(&nodes[i].kind, nodes[i].code.as_deref()).is_some()).then(|| props_button(rects[i], cam.zoom));
        let power = |i: usize| (cam.zoom >= TEXT_ZOOM && !kinds[i].is_some_and(|k| k.category == velo_core::Category::Output)).then(|| power_button(rects[i], cam.zoom));
        // every signal's line on screen to a card it moves, one however many of its values it
        // moves: the card, the signal node, its output, the source, the line
        let mut maps: Vec<(usize, usize, u8, String, Vec<Pos2>)> = Vec::new();
        for (j, n) in nodes.iter().enumerate() {
            for map in &n.mappings {
                if maps.iter().any(|g| g.0 == j && g.3 == map.source) {
                    continue;
                }
                let Some((id, out)) = velo_core::signal_output(&map.source) else { continue };
                let Some(&i) = index.get(id) else { continue };
                // into its jack in the card's foot, through the ties holding it
                let key = velo_core::Tie::line(&n.id, &map.source);
                let Some(&(p0, p3)) = ends.get(&key) else { continue };
                let line = route(&key, p0, p3);
                if Rect::from_points(&line).expand(40.0).intersects(rect) {
                    maps.push((j, i, out, map.source.clone(), line));
                }
            }
        }
        // the cables a line from `a` to `b` crosses, and where (as ties name them)
        let cut = |a: Pos2, b: Pos2| {
            let at = |pts: &[Pos2]| pts.windows(2).find(|s| crosses(a, b, s[0], s[1])).map(|s| s[0].lerp(s[1], 0.5));
            let mut crossed: Vec<(String, Pos2)> = wires.iter().enumerate().filter_map(|(e, w)| Some((velo_core::Tie::wire(&scene.edges[e].to), at(&w.as_ref()?.1)?))).collect();
            crossed.extend(maps.iter().filter_map(|m| Some((velo_core::Tie::line(&nodes[m.0].id, &m.3), at(&m.4)?))));
            crossed
        };
        let reach = (PORT_REACH * cam.zoom).max(6.0);
        let hit = |p: Pos2| {
            // ports first (they straddle the card's edge), the topmost node first
            for i in (0..nodes.len()).rev() {
                if strips[i].contains(p) {
                    return Hit::Node(i);
                }
                if !rects[i].expand(reach).contains(p) {
                    continue;
                }
                for output in [true, false] {
                    if let Some(j) = (0..count(i, output) as u8).find(|&j| listed(i, j, output) && port(i, j, output).distance(p) <= reach) {
                        return Hit::Port(i, j, output);
                    }
                }
                if rects[i].contains(p) {
                    return Hit::Node(i);
                }
            }
            if let Some(k) = ties.iter().position(|(at, held, gap)| !held.is_empty() && untie_badge(tie_rect(*at, held.len(), *gap, cam.zoom)).contains(p)) {
                return Hit::Untie(k);
            }
            if let Some(k) = ties.iter().position(|(at, held, gap)| !held.is_empty() && tie_rect(*at, held.len(), *gap, cam.zoom).expand(3.0).contains(p)) {
                return Hit::Tie(k);
            }
            let wire = wires.iter().enumerate().find(|(_, w)| w.as_ref().is_some_and(|(_, pts)| near_polyline(p, pts, WIRE_REACH)));
            if let Some((e, _)) = wire {
                return Hit::Wire(e);
            }
            maps.iter().position(|m| near_polyline(p, &m.4, WIRE_REACH)).map_or(Hit::Empty, Hit::Map)
        };

        // ---- input ----
        let mut ops: Vec<(&'static str, Value, Option<&'static str>)> = Vec::new();
        let mut close_merge = false;
        let mut rename = None;
        let mut menu = None;
        // arranged: every node to its place in one undo step, sliding there, the view following
        if std::mem::take(&mut st.arrange) {
            let moves: HashMap<String, [f32; 2]> = arrange(scene).into_iter().collect();
            for (node, to) in &moves {
                if let Some(n) = scene.node(node) {
                    st.glides.insert(node.clone(), (n.pos, now));
                }
                ops.push(("node.move", json!({ "node": node, "pos": to }), Some("arrange")));
            }
            // each tie goes as far as the nodes its cables join, on average
            let moved = |id: &str| scene.node(id).map_or(Vec2::ZERO, |n| moves.get(id).map_or(Vec2::ZERO, |to| vec2(to[0] - n.pos[0], to[1] - n.pos[1])));
            for tie in &scene.ties {
                let ends: Vec<&str> = tie.cables.iter().flat_map(|key| cable_nodes(scene, key)).collect();
                if !ends.is_empty() {
                    let by = ends.iter().map(|id| moved(id)).fold(Vec2::ZERO, |a, b| a + b) / ends.len() as f32;
                    ops.push(("tie.move", json!({ "tie": tie.id, "pos": [tie.pos[0] + by.x, tie.pos[1] + by.y] }), Some("arrange")));
                }
            }
            // and bundles where they help: an output's cables just past it, a node's just
            // before it (none a tie already holds all of)
            for (at, cables) in bundles(scene, &|n| moves.get(&n.id).copied().unwrap_or(n.pos)) {
                ops.push(("tie.add", json!({ "pos": [at.x, at.y], "cables": cables }), Some("arrange")));
                st.cinch.push(([at.x, at.y], now));
            }
            close_merge = true;
            st.zoom_to = Some(ZoomTo::Fit);
        }
        // a mapping's settings changed on its line's popover (merge keys of their own)
        let mut map_ops: Vec<(&'static str, Value, Option<String>)> = Vec::new();
        let alt = ui.input(|i| i.modifiers.alt);
        let tool = st.tie_tool;
        let hover = if st.drag.is_none() && st.hovered { pointer.map(hit) } else { None };
        if bg.drag_started() {
            bg.request_focus();
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(center);
            st.anim = None;
            st.drag = if bg.drag_started_by(PointerButton::Middle) || (space && bg.drag_started_by(PointerButton::Primary)) {
                Some(Drag::Pan { origin, pan: cam.pan })
            } else if bg.drag_started_by(PointerButton::Primary) {
                match hit(origin) {
                    Hit::Port(i, p, true) => Some(Drag::Wire { from: (nodes[i].id.clone(), p), lifted: None }),
                    // pulling on a wired input lifts its wire off; on a free one, draws a wire back
                    Hit::Port(i, p, false) => Some(match scene.wire_into(&nodes[i].id, p) {
                        Some(e) => Drag::Wire { from: e.from.clone(), lifted: Some(e.to.clone()) },
                        None => Drag::Back { into: (nodes[i].id.clone(), p) },
                    }),
                    Hit::Node(i) => {
                        let n = &nodes[i];
                        if !st.selected.contains(&n.id) {
                            if !shift {
                                st.selected.clear();
                            }
                            st.selected.push(n.id.clone());
                        }
                        st.wire = None;
                        Some(Drag::Nodes { anchor: (n.id.clone(), n.pos), origin: cam.graph(center, origin) })
                    }
                    Hit::Tie(k) => {
                        let tie = &scene.ties[k];
                        Some(Drag::Tie { id: tie.id.clone(), grab: cam.graph(center, origin) - pos2(tie.pos[0], tie.pos[1]) })
                    }
                    Hit::Untie(_) => None,
                    // with Alt or the tie tool, a line across cables: let go, a tie gathers them
                    Hit::Empty | Hit::Wire(_) | Hit::Map(..) if alt || tool => Some(Drag::Cut { origin }),
                    // a signal's cable comes out of its jack, to go into another card or nowhere
                    Hit::Map(g) => {
                        let (j, i, out, source) = (maps[g].0, maps[g].1, maps[g].2, maps[g].3.clone());
                        Some(Drag::Signal { from: (nodes[i].id.clone(), out), node: nodes[j].id.clone(), source })
                    }
                    // a wire grabbed anywhere comes off its input, to go into another or nowhere
                    Hit::Wire(e) => {
                        st.wire = Some(scene.edges[e].to.clone());
                        Some(Drag::Wire { from: scene.edges[e].from.clone(), lifted: Some(scene.edges[e].to.clone()) })
                    }
                    Hit::Empty => Some(Drag::Band { origin, base: if shift { st.selected.clone() } else { Vec::new() } }),
                }
            } else {
                None
            };
        }
        // a dragged wire's end looks for an input (one from an output) or an output (one drawn
        // back from an input): the nearest it may go into, or a card's first free one
        let fits = |drag: &Drag, i: usize, j: u8| match drag {
            Drag::Wire { from, lifted } => lifted.as_ref().is_some_and(|l| l.0 == nodes[i].id && l.1 == j) || scene.can_connect((&from.0, from.1), (&nodes[i].id, j)).is_ok(),
            Drag::Back { into } => scene.can_connect((&nodes[i].id, j), (&into.0, into.1)).is_ok(),
            _ => false,
        };
        let seek = match &st.drag {
            Some(Drag::Wire { .. } | Drag::Signal { .. }) => Some(false),
            Some(Drag::Back { .. }) => Some(true),
            _ => None,
        };
        let bubble = SNAP * cam.zoom.sqrt().clamp(0.7, 1.5);
        let snap = match (&st.drag, seek, pointer) {
            (Some(drag), Some(output), Some(p)) => {
                let mut best: Option<(f32, usize, u8)> = None;
                for i in (0..nodes.len()).filter(|&i| rects[i].expand(PULL * bubble).contains(p)) {
                    for j in (0..count(i, output) as u8).filter(|&j| listed(i, j, output)) {
                        let d = port(i, j, output).distance(p);
                        if d <= PULL * bubble && best.is_none_or(|b| d < b.0) && fits(drag, i, j) {
                            best = Some((d, i, j));
                        }
                    }
                }
                if best.is_none()
                    && let Hit::Node(i) = hit(p)
                {
                    let free = (0..count(i, output) as u8).find(|&j| listed(i, j, output) && (output || scene.wire_into(&nodes[i].id, j).is_none()) && fits(drag, i, j));
                    best = free.map(|j| (0.0, i, j));
                }
                best.map(|(_, i, j)| (i, j, output))
            }
            _ => None,
        };
        let cat_of = |i: usize| kinds[i].map_or(t.fg_muted, |k| t.category(k.category));
        // a dragged wire has its source's color; one drawn back, the color of the output it'd take
        let drag_color = match (&st.drag, snap) {
            (Some(Drag::Wire { from, .. } | Drag::Signal { from, .. }), _) => index.get(from.0.as_str()).map_or(t.accent, |&a| cat_of(a)),
            (Some(Drag::Back { .. }), Some((i, ..))) => cat_of(i),
            _ => t.accent,
        };
        // the nodes it can't go into fade while it's dragged (only those on screen are asked); a
        // signal's wire also goes onto any other card, to move one of its values
        if let (Some(drag), Some(output)) = (&st.drag, seek) {
            let signal = |i: usize| kinds[i].is_some_and(|k| k.category == velo_core::Category::Signal);
            let mapping = matches!(drag, Drag::Wire { from, .. } | Drag::Signal { from, .. } if index.get(from.0.as_str()).is_some_and(|&a| signal(a)));
            let takes = |i: usize| (mapping && !signal(i)) || (0..count(i, output) as u8).any(|j| listed(i, j, output) && fits(drag, i, j));
            st.dim = (0..nodes.len()).filter(|&i| rects[i].intersects(rect) && !takes(i)).map(|i| nodes[i].id.clone()).collect();
        }
        let dim = ui.ctx().animate_bool_with_time(id().with("dim"), seek.is_some(), t.motion * 1.5);
        if dim == 0.0 {
            st.dim.clear();
        }
        // where a wire let go now was let go, on the plane
        let drop = pointer.map(|p| cam.graph(center, p));
        // a lone node with nothing wired, dragged onto a wire, goes into it
        let splice = match (&st.drag, pointer) {
            (Some(Drag::Nodes { anchor: (id, _), .. }), Some(p)) if st.selected.len() == 1 => index.get(id.as_str()).and_then(|&i| {
                let free = count(i, false) > 0 && count(i, true) > 0 && !scene.edges.iter().any(|e| &e.from.0 == id || &e.to.0 == id);
                let e = wires.iter().position(|w| free && w.as_ref().is_some_and(|(_, pts)| near_polyline(p, pts, WIRE_REACH * 3.0)))?;
                let edge = &scene.edges[e];
                (scene.can_connect((&edge.from.0, edge.from.1), (id, 0)).is_ok() && scene.can_connect((id, 0), (&edge.to.0, edge.to.1)).is_ok()).then_some(e)
            }),
            _ => None,
        };
        match &st.drag {
            Some(Drag::Pan { origin, pan }) => {
                if let Some(p) = pointer {
                    cam.pan = *pan + (p - *origin);
                }
            }
            Some(Drag::Nodes { anchor: (id, start), origin }) => {
                // the grabbed node follows the pointer exactly; the rest of the selection moves with it
                if let (Some(p), Some(n)) = (pointer, scene.node(id)) {
                    let d = cam.graph(center, p) - *origin;
                    let by = [start[0] + d.x - n.pos[0], start[1] + d.y - n.pos[1]];
                    if by != [0.0, 0.0] {
                        ops.push(("node.move", json!({ "nodes": st.selected, "by": by }), Some("node.move")));
                    }
                }
            }
            Some(Drag::Band { origin, base }) => {
                if let Some(p) = pointer {
                    let band = Rect::from_two_pos(*origin, p);
                    let inside = nodes.iter().zip(&rects).filter(|(n, r)| band.intersects(**r) && !base.contains(&n.id));
                    st.selected = base.iter().cloned().chain(inside.map(|(n, _)| n.id.clone())).collect();
                }
            }
            Some(Drag::Tie { id, grab }) => {
                if let Some(p) = pointer {
                    let to = cam.graph(center, p) - *grab;
                    ops.push(("tie.move", json!({ "tie": id, "pos": [to.x, to.y] }), Some("tie.move")));
                }
            }
            _ => {}
        }
        if bg.drag_stopped() || (st.drag.is_some() && !bg.dragged()) {
            match st.drag.take() {
                Some(Drag::Nodes { anchor: (id, _), .. }) => {
                    if let Some(e) = splice {
                        let edge = &scene.edges[e];
                        ops.push(("edge.connect", json!({ "from": [edge.from.0, edge.from.1], "to": [id, 0] }), Some("node.move")));
                        ops.push(("edge.connect", json!({ "from": [id, 0], "to": [edge.to.0, edge.to.1] }), Some("node.move")));
                        st.pulses.extend([((id.clone(), 0), now), (edge.to.clone(), now)]);
                    }
                    close_merge = true;
                }
                Some(Drag::Wire { from, lifted }) => {
                    let to = snap.map(|(i, j, _)| (nodes[i].id.clone(), j));
                    // plugged in (or back where it was): its end slides home; let go of nowhere: it
                    // springs back into its output (no motion: neither)
                    let land = |to: &(String, u8)| drop.filter(|_| t.motion > 0.0).map(|drop| Landing { to: to.clone(), output: false, drop, color: drag_color, at: now });
                    let loose = drop.filter(|_| t.motion > 0.0).map(|drop| Loose { port: (from.0.clone(), from.1, true), drop, color: drag_color, at: now });
                    match (to, lifted) {
                        (Some(to), Some(old)) if to == old => {
                            st.wire = None;
                            st.landings.extend(land(&to));
                        }
                        (Some(to), lifted) => {
                            st.pulses.push((to.clone(), now));
                            st.landings.extend(land(&to));
                            st.wire = None;
                            ops.push(("edge.connect", json!({ "from": [from.0, from.1], "to": [to.0, to.1] }), Some("wire")));
                            if let Some(old) = lifted {
                                ops.push(("edge.disconnect", json!({ "to": [old.0, old.1] }), Some("wire")));
                            }
                            close_merge = true;
                        }
                        (None, Some(old)) => {
                            st.wire = None;
                            st.loose = loose;
                            ops.push(("edge.disconnect", json!({ "to": [old.0, old.1] }), None));
                        }
                        // a new wire dropped on empty canvas: pick a node for it to go into; a
                        // signal dropped on a card: pick the parameter it moves
                        (None, None) => {
                            let signal = index.get(from.0.as_str()).and_then(|&i| kinds[i]).is_some_and(|k| k.category == velo_core::Category::Signal);
                            match pointer.map(|p| (p, hit(p))) {
                                Some((p, Hit::Node(i) | Hit::Port(i, ..))) if signal && kinds[i].is_some_and(|k| k.category != velo_core::Category::Signal) => {
                                    st.pick = Some((format!("{}/{}", from.0, from.1), nodes[i].id.clone(), p));
                                }
                                Some((p, Hit::Empty | Hit::Wire(_) | Hit::Map(..))) => menu = Some((p, Some(from))),
                                _ => st.loose = loose,
                            }
                        }
                    }
                }
                Some(Drag::Back { into }) => match snap {
                    Some((i, j, _)) => {
                        st.pulses.push((into.clone(), now));
                        st.landings.extend(drop.filter(|_| t.motion > 0.0).map(|drop| Landing { to: into.clone(), output: true, drop, color: drag_color, at: now }));
                        ops.push(("edge.connect", json!({ "from": [nodes[i].id, j], "to": [into.0, into.1] }), Some("wire")));
                        close_merge = true;
                    }
                    None => st.loose = drop.filter(|_| t.motion > 0.0).map(|drop| Loose { port: (into.0, into.1, false), drop, color: drag_color, at: now }),
                },
                Some(Drag::Tie { .. }) => close_merge = true,
                // a signal's cable let go: back where it was, onto another card (its values to
                // pick from, the old card losing it once one is picked), or onto nothing (gone,
                // springing back into its output)
                Some(Drag::Signal { from, node, source }) => match pointer.map(hit) {
                    Some(Hit::Node(i) | Hit::Port(i, ..)) if !signal(i) => {
                        if nodes[i].id != node {
                            st.pick = pointer.map(|p| (source.clone(), nodes[i].id.clone(), p));
                            st.repick = Some((node, source));
                        }
                    }
                    _ => {
                        for m in scene.node(&node).into_iter().flat_map(|n| &n.mappings).filter(|m| m.source == source) {
                            ops.push(("mapping.remove", json!({ "node": node, "param": m.param, "source": source }), Some("unmap")));
                        }
                        close_merge = true;
                        st.map = None;
                        st.loose = drop.filter(|_| t.motion > 0.0).map(|drop| Loose { port: (from.0, from.1, true), drop, color: drag_color, at: now });
                    }
                },
                // the cables the line crossed, tied where they crossed it; they cinch in
                Some(Drag::Cut { origin }) => {
                    let crossed = pointer.filter(|p| p.distance(origin) > 6.0).map(|p| cut(origin, p)).unwrap_or_default();
                    if !crossed.is_empty() {
                        let mid = crossed.iter().fold(Vec2::ZERO, |sum, (_, at)| sum + at.to_vec2()) / crossed.len() as f32;
                        let at = cam.graph(center, mid.to_pos2());
                        let cables: Vec<&String> = crossed.iter().map(|(key, _)| key).collect();
                        ops.push(("tie.add", json!({ "pos": [at.x, at.y], "cables": cables }), None));
                        st.cinch.push(([at.x, at.y], now));
                    }
                }
                _ => {}
            }
        }
        if bg.clicked() {
            bg.request_focus();
            let on_chevron = bg.interact_pointer_pos().and_then(|p| (0..nodes.len()).rev().find(|&i| chevron(i).is_some_and(|b| b.contains(p))));
            let on_power = bg.interact_pointer_pos().and_then(|p| match hit(p) {
                Hit::Node(i) if power(i).is_some_and(|b| b.contains(p)) => Some(i),
                _ => None,
            });
            if let Some(i) = on_power {
                ops.push(("node.toggle", json!({ "nodes": [nodes[i].id] }), None));
            } else if let Some(i) = on_chevron {
                let id = nodes[i].id.clone();
                st.props = (st.props.as_ref() != Some(&id)).then(|| id.clone());
                (st.selected, st.wire) = (vec![id], None);
            } else if let Some(p) = bg.interact_pointer_pos() {
                let target = hit(p);
                if !matches!(target, Hit::Map(..)) {
                    st.map = None;
                }
                match target {
                    // Alt+click takes a wire or a mapping away
                    Hit::Wire(e) if alt => ops.push(("edge.disconnect", json!({ "to": [scene.edges[e].to.0, scene.edges[e].to.1] }), None)),
                    Hit::Map(g) if alt => {
                        let (j, source) = (maps[g].0, &maps[g].3);
                        for map in nodes[j].mappings.iter().filter(|m| &m.source == source) {
                            ops.push(("mapping.remove", json!({ "node": nodes[j].id, "param": map.param, "source": map.source }), Some("unmap")));
                        }
                        close_merge = true;
                    }
                    Hit::Map(g) => {
                        st.map = Some((nodes[maps[g].0].id.clone(), maps[g].3.clone()));
                        (st.selected, st.wire) = (Vec::new(), None);
                    }
                    Hit::Node(i) | Hit::Port(i, ..) => {
                        let id = &nodes[i].id;
                        st.wire = None;
                        match st.selected.iter().position(|s| s == id) {
                            Some(at) if shift => {
                                st.selected.remove(at);
                            }
                            _ if shift => st.selected.push(id.clone()),
                            _ => {
                                st.selected = vec![id.clone()];
                            }
                        }
                    }
                    Hit::Wire(e) => {
                        st.wire = Some(scene.edges[e].to.clone());
                        st.selected.clear();
                    }
                    // Alt+click or its × takes a tie off; a click chooses it (Delete takes it off)
                    Hit::Tie(k) if alt => ops.push(("tie.remove", json!({ "tie": scene.ties[k].id }), None)),
                    Hit::Untie(k) => {
                        ops.push(("tie.remove", json!({ "tie": scene.ties[k].id }), None));
                        st.tie = None;
                    }
                    Hit::Tie(k) => {
                        st.tie = Some(scene.ties[k].id.clone());
                        (st.selected, st.wire, st.map) = (Vec::new(), None, None);
                    }
                    Hit::Empty => {
                        st.tie = None;
                        if !shift {
                            st.selected.clear();
                            st.wire = None;
                        }
                    }
                }
            }
        }
        // a quick third click is a triple to egui: here it still means what a double does (as in FigCraft)
        if (bg.double_clicked() || bg.triple_clicked())
            && let Some(p) = bg.interact_pointer_pos()
        {
            match hit(p) {
                Hit::Node(i) if !power(i).is_some_and(|b| b.contains(p)) => rename = Some((nodes[i].id.clone(), nodes[i].name.clone())),
                Hit::Empty => menu = Some((p, None)),
                // a tie double-clicked comes off: its cables go their own ways
                Hit::Tie(k) => ops.push(("tie.remove", json!({ "tie": scene.ties[k].id }), None)),
                _ => {}
            }
        }
        if keys && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            st.selected.clear();
            (st.wire, st.map) = (None, None);
        }
        if bg.has_focus() {
            // Tab is the node menu here and Esc deselects: neither moves the focus away
            ui.memory_mut(|m| m.set_focus_lock_filter(id(), EventFilter { tab: true, escape: true, ..Default::default() }));
        }
        if std::mem::take(&mut st.open_menu) {
            // at the pointer; from the keyboard elsewhere, beside the node it would append to
            let beside = match st.selected.as_slice() {
                [one] => index.get(one.as_str()).map(|&i| rects[i].right_top() + vec2(t.pad * 2.0, 0.0)),
                _ => None,
            };
            let at = pointer.filter(|_| st.hovered).or(beside).unwrap_or(view.center());
            menu = Some((at, None));
        }
        if let Some((at, from)) = menu {
            st.menu = Some(Menu::new(cam.graph(center, at), from, None, None));
        }

        // ---- paint ----
        let painter = ui.painter_at(rect);
        if !backdrop {
            grid(&painter, &t, rect, center + cam.pan, cam.zoom);
        }
        let selected: HashSet<&str> = st.selected.iter().map(String::as_str).collect();
        let lifted = match &st.drag {
            Some(Drag::Wire { lifted: Some(l), .. }) => Some(l),
            _ => None,
        };
        let width = (1.5 * cam.zoom).clamp(1.0, 2.5);
        // what a signal node's output reads now: its meters, its wires' glow
        let read = |id: &str, port: u8| scene.node(id).filter(|n| !n.off).and_then(|_| self.live.as_ref()?.signal(id, port)).unwrap_or(0.0);
        for (e, w) in wires.iter().enumerate() {
            let (Some((a, pts)), edge) = (w, &scene.edges[e]) else { continue };
            if lifted == Some(&edge.to) {
                continue;
            }
            // plugged in by hand: its end slides home from where it was let go, as the bubble
            // goes in, and it settles from bright to its rest color
            let landing = st.landings.iter().find(|l| l.to == edge.to).map(|l| (l, (now - l.at) as f32 / t.motion));
            let slid;
            let pts = match landing {
                Some((l, k)) if k < 1.2 => {
                    let home = if l.output { pts[0] } else { pts[pts.len() - 1] };
                    let end = cam.screen(center, l.drop).lerp(home, kit::ease(k / 1.2));
                    slid = if l.output { bezier(end, pts[pts.len() - 1], cam.zoom) } else { bezier(pts[0], end, cam.zoom) };
                    &slid
                }
                _ => pts,
            };
            let cat = kinds[*a].map_or(t.fg_muted, |k| t.category(k.category));
            let (color, w) = if st.wire.as_ref() == Some(&edge.to) {
                (t.accent, width + 1.0)
            } else if hover == Some(Hit::Wire(e)) {
                (Theme::mix(cat, t.fg_text, 0.4), width + 0.5)
            } else if selected.contains(edge.from.0.as_str()) || selected.contains(edge.to.0.as_str()) {
                (Theme::mix(t.line, cat, 0.85), width)
            } else {
                (Theme::mix(t.line, cat, 0.5), width)
            };
            let (color, w) = landing.map_or((color, w), |(l, k)| {
                let k = kit::ease(k / 3.6);
                (Theme::mix(l.color, color, k), w + 1.0 - k)
            });
            let fade = if nodes[*a].off { 0.45 } else { 1.0 };
            let chosen = st.wire.as_ref() == Some(&edge.to);
            // grabbed attention: a glow under the hovered, the chosen and the one a node would go into
            if chosen || hover == Some(Hit::Wire(e)) || splice == Some(e) {
                let glow = if splice == Some(e) { t.accent } else { color };
                painter.add(Shape::line(pts.clone(), Stroke::new(w * 5.0, glow.gamma_multiply(0.14))));
            }
            // what goes through it, as lights running to the input
            if chosen || hover == Some(Hit::Wire(e)) {
                for k in 0..3 {
                    let at = crawl(pts, now, 40.0 * cam.zoom, k, 3);
                    painter.circle_filled(at, 2.0 * cam.zoom.sqrt(), color);
                }
                ui.ctx().request_repaint();
            }
            // a wire just made: one light along it
            if let Some(&(_, at)) = st.pulses.iter().find(|(to, _)| to == &edge.to) {
                let k = ((now - at) / 0.45) as f32;
                if k < 1.0 {
                    let p = along(pts, kit::ease(k));
                    painter.circle_filled(p, 7.0 * cam.zoom.sqrt(), t.accent.gamma_multiply(0.25 * (1.0 - k)));
                    painter.circle_filled(p, 3.0 * cam.zoom.sqrt(), t.accent);
                    ui.ctx().request_repaint();
                }
            }
            // numbers between signal nodes: dashed, like the wires into parameters, brighter and
            // wider with what they carry
            if kinds[*a].is_some_and(|k| k.category == velo_core::Category::Signal) {
                let v = read(&edge.from.0, edge.from.1).clamp(0.0, 1.0);
                if v > 0.05 {
                    painter.add(Shape::line(pts.clone(), Stroke::new(w * (2.0 + 4.0 * v), t.cat_signal.gamma_multiply(0.12 * v))));
                }
                painter.extend(Shape::dashed_line(pts, Stroke::new(w * (1.0 + v), Theme::mix(Theme::mix(t.line, t.cat_signal, 0.8), t.fg_text, 0.5 * v).gamma_multiply(fade)), 6.0 * cam.zoom, 4.0 * cam.zoom));
            } else {
                painter.add(Shape::line(pts.clone(), Stroke::new(w, color.gamma_multiply(fade))));
            }
        }
        for (_, at) in st.pulses.iter_mut().filter(|(_, at)| at.is_nan()) {
            *at = now;
        }
        st.pulses.retain(|(_, at)| now - at < 0.45);
        let hues = hues(scene, &index);
        // a wired input takes its wire's color: the color of what feeds it
        let color_of = |id: &str| index.get(id).and_then(|&i| kinds[i]).map_or(t.fg_muted, |k| t.category(k.category));
        let wired_in: HashMap<(&str, u8), egui::Color32> = scene.edges.iter().filter(|e| lifted != Some(&e.to)).map(|e| ((e.to.0.as_str(), e.to.1), color_of(&e.from.0))).collect();
        let wired_out: HashSet<(&str, u8)> = scene.edges.iter().map(|e| (e.from.0.as_str(), e.from.1)).collect();
        let hot_port = match (hover, snap) {
            (_, Some(s)) => Some(s),
            (Some(Hit::Port(i, j, out)), _) => Some((i, j, out)),
            // a signal's cable pointed at: the output it leaves from, as its jack
            (Some(Hit::Map(g)), _) => maps.get(g).map(|m| (m.1, m.2, true)),
            _ => None,
        };
        // signal wires: from a signal node's output to each node a mapping of it moves, dashed
        let beats = self.live.as_ref().map_or(0.0, crate::live::Live::beats);
        // the drum model decides the hits, and the beat while the tempo follows the music
        let model = self.live.as_ref().is_some_and(|l| l.audio.settings.mode == velo_audio::Mode::Model);
        let following = self.settings.auto_bpm;
        let meters: Vec<(Vec<f32>, usize)> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                if n.off || n.kind == velo_core::LFO || !kinds[i].is_some_and(|k| k.category == velo_core::Category::Signal) {
                    return (Vec::new(), 0);
                }
                // a Probe's input is a picture: only what it gives is metered
                let ins = if n.kind == velo_core::PROBE { 0 } else { count(i, false) };
                let inputs = (0..ins as u8).map(|p| scene.wire_into(&n.id, p).map_or(0.0, |e| read(&e.from.0, e.from.1)));
                let values: Vec<f32> = inputs.chain((0..count(i, true) as u8).map(|p| read(&n.id, p))).collect();
                (values, ins)
            })
            .collect();
        st.trails.retain(|(id, _), _| scene.node(id).is_some_and(|n| !n.off));
        // the rows' curves: a sample every 1/30 s
        if now - st.trail_at >= 1.0 / 30.0 {
            st.trail_at = now;
            for (i, n) in nodes.iter().enumerate() {
                for &p in rows[i].iter().flatten() {
                    let trail = st.trails.entry((n.id.clone(), p)).or_default();
                    trail.push_back(meters[i].0.get(meters[i].1 + usize::from(p)).copied().unwrap_or(0.0));
                    if trail.len() > TRAIL {
                        trail.pop_front();
                    }
                }
            }
        }
        // the Output going out, as the engine picks it: the last one something's wired into
        let outputs: Vec<usize> = (0..nodes.len()).filter(|&i| kinds[i].is_some_and(|k| k.category == velo_core::Category::Output)).collect();
        let going_out = outputs.iter().rev().copied().find(|&i| scene.edges.iter().any(|e| e.to.0 == nodes[i].id)).or(outputs.first().copied());
        let (mut chosen_map, mut map_label) = (None, None);
        // a value set from a link's popover: its node, its key, the value
        let mut values: Vec<(String, String, f32)> = Vec::new();
        for (_, at) in st.plug_ins.iter_mut().filter(|(_, at)| at.is_nan()) {
            *at = now;
        }
        st.plug_ins.retain(|(_, at)| now - at < 0.6);
        if !st.plug_ins.is_empty() {
            ui.ctx().request_repaint();
        }
        // the Audio In's reels turn faster as it's louder
        let level = nodes.iter().find(|n| n.kind == velo_core::AUDIO && !n.off).and_then(|n| Some(read(&n.id, node_kind(&n.kind)?.outputs.iter().position(|o| *o == "level")? as u8))).unwrap_or(0.0);
        st.reel = (st.reel + dt * (0.4 + 6.0 * level.clamp(0.0, 1.0))) % std::f32::consts::TAU;
        st.peak = level.max(st.peak - dt * 0.35).max(0.0);
        for (g, (j, i, out, source, curve)) in maps.iter().enumerate() {
            let n = &nodes[*j];
            // taken out of its jack: drawn from the pointer instead
            if matches!(&st.drag, Some(Drag::Signal { node, source: s, .. }) if *node == n.id && s == source) {
                continue;
            }
            let moved: Vec<&velo_core::Mapping> = n.mappings.iter().filter(|m| &m.source == source).collect();
            let v = read(&nodes[*i].id, *out).clamp(0.0, 1.0);
            let hot = hover == Some(Hit::Map(g));
            let chosen = st.map.as_ref().is_some_and(|(node, s)| node == &n.id && s == source);
            if v > 0.05 || hot || chosen {
                let glow = if hot || chosen { 0.2 } else { 0.1 * v };
                painter.add(Shape::line(curve.clone(), Stroke::new((3.0 + 6.0 * v) * cam.zoom.sqrt(), t.cat_signal.gamma_multiply(glow))));
            }
            let fade = if nodes[*i].off { 0.45 } else { 1.0 };
            let lit = if hot || chosen { 0.7 } else { 0.4 * v };
            let color = Theme::mix(t.cat_signal.gamma_multiply(0.8), t.fg_text, lit).gamma_multiply(fade);
            let stroke = Stroke::new((1.5 + 1.0 * v + f32::from(u8::from(hot || chosen))) * cam.zoom.sqrt(), color);
            painter.extend(Shape::dashed_line(curve, stroke, 6.0 * cam.zoom, 4.0 * cam.zoom));
            // what goes through it: lights crawling to the card, brighter as it's louder
            if v > 0.05 || hot || chosen {
                for k in 0..4 {
                    let at = crawl(curve, now, 16.0 * cam.zoom, k, 4);
                    painter.circle_filled(at, (1.5 + 2.5 * v) * cam.zoom.sqrt(), Theme::mix(t.cat_signal, t.fg_text, 0.5 * v).gamma_multiply((0.3 + 0.7 * v).max(if hot || chosen { 0.6 } else { 0.0 }) * fade));
                }
                ui.ctx().request_repaint();
            }
            // it lands in the card's jack (drawn with the card); how many of its values, by the middle
            if moved.len() > 1 && cam.zoom >= PORT_ZOOM {
                let mid = along(curve, 0.5);
                painter.circle(mid, 8.0, t.bg_card, Stroke::new(1.0, color));
                painter.text(mid, Align2::CENTER_CENTER, moved.len().to_string(), Theme::mono(t.text_xs), t.fg_text);
            }
            if chosen {
                chosen_map = Some((*j, source.clone(), along(curve, 0.5)));
            } else if hot {
                // what it is: the source, what it moves and how far (over the cards, at the pointer)
                let what: Vec<String> = moved.iter().map(|m| format!("{} {:+.0}%", crate::param_name(n, &m.param), m.amount * 100.0)).collect();
                let text = format!("{} → {}", crate::mapping_source(scene, source), what.join(" · "));
                map_label = Some((pointer.unwrap_or_else(|| along(curve, 0.5)), text));
            }
        }
        // the ties: a capsule standing across each bundle, lit while pointed at or slid
        for (k, (at, held, gap)) in ties.iter().enumerate().filter(|(_, (_, held, _))| !held.is_empty()) {
            let tie = &scene.ties[k];
            let chosen = st.tie.as_ref() == Some(&tie.id);
            let held_now = chosen || matches!(hover, Some(Hit::Tie(j) | Hit::Untie(j)) if j == k) || matches!(&st.drag, Some(Drag::Tie { id, .. }) if *id == tie.id);
            let hot = ui.ctx().animate_bool_with_time(id().with(("tie", tie.id.as_str())), held_now, t.motion);
            let r = tie_rect(*at, held.len(), *gap, cam.zoom);
            let round = CornerRadius::same((r.width() / 2.0) as u8);
            if hot > 0.0 {
                painter.rect_filled(r.expand(4.0), CornerRadius::same((r.width() / 2.0 + 4.0) as u8), (if chosen { t.accent } else { t.fg_text }).gamma_multiply(0.1 * hot));
            }
            painter.rect_filled(r, round, t.bg_card);
            let ring = if chosen { t.accent } else { Theme::mix(Theme::mix(t.line, t.fg_text, 0.45), t.fg_text, hot) };
            painter.rect_stroke(r, round, Stroke::new(1.5, ring), StrokeKind::Inside);
            // near it or chosen: the × that takes it off
            let near = pointer.is_some_and(|p| r.expand(22.0).contains(p)) || chosen;
            let show = ui.ctx().animate_bool_with_time(id().with(("untie", tie.id.as_str())), near && st.drag.is_none(), t.motion);
            if show > 0.0 {
                let b = untie_badge(r);
                let over = hover == Some(Hit::Untie(k));
                painter.circle_filled(b.center(), 7.0 * show, Theme::mix(t.bg_card, t.error, if over { 0.6 } else { 0.0 }));
                painter.circle_stroke(b.center(), 7.0 * show, Stroke::new(1.0, Theme::mix(t.line, t.fg_text, 0.4).gamma_multiply(show)));
                kit::icon_at(&painter, b.center(), icons::X, 9.0 * show, t.fg_text.gamma_multiply(show));
            }
        }
        for (i, n) in nodes.iter().enumerate() {
            let r = rects[i];
            if !Rect::from_min_max(r.min, pos2(r.right(), strips[i].bottom())).expand(12.0).intersects(rect) {
                continue;
            }
            let output = kinds[i].is_some_and(|k| k.category == velo_core::Category::Output);
            let signal = kinds[i].is_some_and(|k| k.category == velo_core::Category::Signal);
            let soon = kinds[i].is_some_and(|k| !output && !signal && !velo_core::is_shader(k.id) && velo_engine::builtin(k.id).is_none());
            let props = chevron(i).map(|_| st.props.as_deref() == Some(n.id.as_str()));
            // (an Audio In's whole card glows with what it hears)
            let heard = if n.kind == velo_core::AUDIO { level } else { 0.0 };
            let glow = if n.off { 0.0 } else { n.mappings.iter().filter_map(|m| velo_core::signal_output(&m.source)).map(|(id, p)| read(id, p)).fold(heard, f32::max) };
            // what it draws: the output's the preview, any other node's its own small picture
            let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let shows = if n.off {
                None
            } else if output {
                // of several Outputs, only the one going out shows the picture
                picture.filter(|_| Some(i) == going_out).map(|p| (p, full))
            } else {
                thumbs.get(i).copied().flatten()
            };
            let cost = self.live.as_ref().and_then(|l| l.cost(&n.id)).filter(|_| !n.off);
            let asleep = !n.off && self.live.as_ref().is_some_and(|l| l.asleep(&n.id));
            let shell = Rect::from_min_max(r.min, pos2(r.right(), strips[i].bottom()));
            let look = Look { name: &n.name, kind: kinds[i], selected: selected.contains(n.id.as_str()), off: n.off, hue: hues[i], picture: shows, soon, props, rows: rows[i].is_some(), icon: &n.icon, maps: n.mappings.len(), glow, cost, asleep, shell };
            paint_node(ui, &painter, &t, r, cam.zoom, look);
            if !n.off && n.kind == velo_core::LFO {
                paint_lfo(&painter, &t, r, cam.zoom, &velo_engine::signal::Lfo::from_params(|p| n.params.get(p).map(|v| v[0])), beats);
            } else if let (Some(list), Some(k)) = (&rows[i], kinds[i]) {
                let listed: Vec<Row> = list
                    .iter()
                    .map(|&p| {
                        let id = k.outputs[usize::from(p)];
                        let now = meters[i].0.get(meters[i].1 + usize::from(p)).copied().unwrap_or(0.0);
                        let model = model && (matches!(id, "kick" | "snare" | "hat") || (n.kind == velo_core::CLOCK && following));
                        Row { name: crate::source_label(id), hit: matches!(id, "kick" | "snare" | "hat" | "beat"), model, now, trail: if n.off { None } else { st.trails.get(&(n.id.clone(), p)) } }
                    })
                    .collect();
                let deck = deck_h(&n.kind);
                let badges = paint_rows(&painter, &t, r, cam.zoom, deck, &listed);
                if ui.ctx().pointer_hover_pos().is_some_and(|p| badges.iter().any(|b| b.contains(p))) {
                    bg.clone().on_hover_text(widgets::model_tip());
                }
                if deck > 0.0 {
                    let at = |id: &str| k.outputs.iter().position(|o| *o == id).map_or(0.0, |p| read(&n.id, p as u8));
                    let room = Rect::from_min_max(pos2(r.left() + 1.0, r.top() + HEADER_H * cam.zoom + 0.5), pos2(r.right() - 1.0, r.top() + (HEADER_H + deck) * cam.zoom));
                    let level = if n.off { 0.0 } else { at("level") };
                    // (the test loop's name carries its tempo: the deck shows that apart)
                    let source = self.live.as_ref().and_then(|l| l.audio.state()).map(|s| s.name.split(" · ").next().unwrap_or_default().to_string()).unwrap_or_default();
                    let bpm = self.live.as_ref().map(|l| l.clock()).map(|c| (c[0], c[1]));
                    let deck = Deck { reel: st.reel, tape: ((now / 90.0).fract() as f32 * 2.0 - 1.0).abs(), level, peak: st.peak, source: &source, bpm };
                    paint_deck(&painter, &t, room, cam.zoom, &deck);
                }
            } else if !meters[i].0.is_empty() {
                paint_meters(&painter, &t, r, cam.zoom, &meters[i].0, meters[i].1);
            }
            // the signals plugged in, a jack each in the card's foot, larger while its cable is
            // pointed at or chosen, a ring leaving one just plugged in
            let plugged = jacks(n);
            if !plugged.is_empty() || slots[i] > 0.0 {
                let lines: Vec<Jack> = plugged
                    .iter()
                    .map(|s| {
                        let moves: Vec<String> = n.mappings.iter().filter(|m| &m.source == s).map(|m| crate::param_name(n, &m.param).to_string()).collect();
                        let v = if n.off { 0.0 } else { velo_core::signal_output(s).map_or(0.0, |(id, p)| read(id, p)) };
                        let pointed = maps.iter().position(|m| m.0 == i && &m.3 == s).is_some_and(|g| hover == Some(Hit::Map(g)));
                        let chosen = st.map.as_ref().is_some_and(|(node, source)| node == &n.id && source == s);
                        let hot = ui.ctx().animate_bool_with_time(id().with(("jack", n.id.as_str(), *s)), pointed || chosen, t.motion);
                        let ring = st.plug_ins.iter().find(|((node, source), _)| node == &n.id && source == s).map_or(1.0, |(_, at)| (((now - at) / 0.5) as f32).clamp(0.0, 1.0));
                        Jack { label: format!("{} → {}", crate::mapping_source(scene, s), moves.join(", ")), v, hot, ring }
                    })
                    .collect();
                paint_jacks(&painter, &t, strips[i], cam.zoom, &lines, slots[i]);
            }
            if n.off {
                let radius = (f32::from(t.radius_md) * cam.zoom).clamp(2.0, 255.0) as u8;
                painter.rect_filled(shell, CornerRadius::same(radius), t.bg_base.gamma_multiply(0.55));
            }
            let cat = kinds[i].map_or(t.fg_muted, |k| t.category(k.category));
            if let Some(b) = power(i) {
                painter.text(b.center(), Align2::CENTER_CENTER, icons::POWER, FontId::proportional(13.0 * cam.zoom), if n.off { t.fg_muted } else { cat });
            }
            for output in [false, true] {
                for j in (0..count(i, output) as u8).filter(|&j| listed(i, j, output)) {
                    if cam.zoom < PORT_ZOOM && hot_port != Some((i, j, output)) {
                        continue;
                    }
                    let fed = if output { wired_out.contains(&(n.id.as_str(), j)).then_some(cat) } else { wired_in.get(&(n.id.as_str(), j)).copied() };
                    // while a wire is dragged, every port it may go into shows it
                    if let Some(drag) = &st.drag
                        && seek == Some(output)
                        && fits(drag, i, j)
                    {
                        painter.circle_stroke(port(i, j, output), 7.0 * cam.zoom.sqrt(), Stroke::new(1.5, drag_color.gamma_multiply(0.5)));
                    }
                    let hot = ui.ctx().animate_bool_with_time(id().with(("port", n.id.as_str(), j, output)), hot_port == Some((i, j, output)), t.motion);
                    paint_port(&painter, &t, port(i, j, output), cam.zoom, fed, hot);
                }
            }
            if dim > 0.0 && st.dim.contains(&n.id) {
                let radius = (f32::from(t.radius_md) * cam.zoom).clamp(2.0, 255.0) as u8;
                painter.rect_filled(shell.expand(6.0 * cam.zoom.max(0.5)), CornerRadius::same(radius.saturating_add(6)), t.bg_base.gamma_multiply(0.6 * dim));
            }
        }
        // ---- a dragged wire's snap: a bubble grows round the port it'd go into, its end melts into
        // it through a neck that stretches and snaps as it's pulled away; let go, it goes in ----
        let target = snap.map(|(i, j, output)| (nodes[i].id.clone(), j, output));
        for b in &mut st.bubbles {
            let to = if Some(&b.port) == target.as_ref() { 1.0 } else { 0.0 };
            if b.to != to {
                (b.from, b.to, b.at) = (b.size(now, t.motion), to, now);
            }
            if seek.is_some() {
                b.color = drag_color;
            }
        }
        if let Some(port) = target.filter(|p| !st.bubbles.iter().any(|b| &b.port == p)) {
            st.bubbles.push(Bubble { port, color: drag_color, from: 0.0, to: 1.0, at: now });
        }
        st.bubbles.retain(|b| b.to > 0.0 || b.size(now, t.motion) > 0.0);
        let zs = cam.zoom.sqrt().clamp(0.7, 1.5);
        let tip = pointer.filter(|_| seek.is_some()).map(|p| (p, TIP * zs));
        for b in &st.bubbles {
            if let Some(&i) = index.get(b.port.0.as_str()) {
                goo(&painter, (port(i, b.port.1, b.port.2), bubble * b.size(now, t.motion).max(0.0)), tip, b.color);
            }
            if b.moving(now, t.motion) {
                ui.ctx().request_repaint();
            }
        }
        // just plugged in: a ring spreads from the port and two lights run round the card, one
        // each way, meeting on the far side
        for l in &st.landings {
            let k = ((now - l.at) as f32 / t.motion - 0.6) / 3.0;
            let Some(e) = scene.edges.iter().find(|e| e.to == l.to).filter(|_| (0.0..1.0).contains(&k)) else { continue };
            let (id, j) = if l.output { (&e.from.0, e.from.1) } else { (&e.to.0, e.to.1) };
            let Some(&i) = index.get(id.as_str()) else { continue };
            let (at, fade) = (port(i, j, l.output), 1.0 - k);
            let ring = bubble * (0.5 + 0.7 * kit::ease(k * 1.2));
            painter.circle_stroke(at, ring, Stroke::new(7.0 * zs, l.color.gamma_multiply(0.08 * fade)));
            painter.circle_stroke(at, ring, Stroke::new((1.0 + 2.0 * fade) * zs, l.color.gamma_multiply(0.6 * fade)));
            let round = outline(rects[i], (f32::from(t.radius_md) * cam.zoom).clamp(2.0, 255.0), at);
            let head = 0.5 * kit::ease(k);
            for way in [1.0, -1.0] {
                let trail: Vec<Pos2> = (0..12).map(|s| along(&round, 0.5 - way * (0.5 - head + s as f32 * 0.01))).collect();
                for (s, seg) in trail.windows(2).enumerate() {
                    painter.line_segment([seg[0], seg[1]], Stroke::new(2.5 * zs, l.color.gamma_multiply(fade * (1.0 - s as f32 / 11.0))));
                }
                painter.circle_filled(trail[0], 6.0 * zs, l.color.gamma_multiply(0.25 * fade));
            }
        }
        st.landings.retain(|l| ((now - l.at) as f32) < t.motion * 3.6);
        if !st.landings.is_empty() {
            ui.ctx().request_repaint();
        }
        // a wire let go of nowhere springs back into its port, fading
        if let Some(Loose { port: (id, j, output), drop, color, at }) = &st.loose {
            let k = (now - at) as f32 / (t.motion * 1.5);
            if let Some(&i) = index.get(id.as_str()).filter(|_| k < 1.0) {
                let home = port(i, *j, *output);
                let end = cam.screen(center, *drop).lerp(home, kit::ease(k));
                let line = if *output { bezier(home, end, cam.zoom) } else { bezier(end, home, cam.zoom) };
                painter.add(Shape::line(line, Stroke::new(width + 1.0, color.gamma_multiply(1.0 - k))));
                painter.circle_filled(end, 3.5 * zs * (1.0 - k), color.gamma_multiply(1.0 - k));
                ui.ctx().request_repaint();
            } else {
                st.loose = None;
            }
        }
        // a signal's wire over a card: drawn to the slot coming out of its foot, its end melting
        // into the open socket as it gets there
        let slot_socket = opening.filter(|_| signal_drag && snap.is_none()).map(|i| (jack_socket(strips[i], cam.zoom, jacks(&nodes[i]).len()), slots[i]));
        if let Some((socket, k)) = slot_socket {
            goo(&painter, (socket, bubble * k), pointer.map(|p| (p, TIP * zs)), drag_color);
        }
        // the wire being dragged: its end at the pointer, the bubble showing where it'd go
        if let (Some(drag), Some(p)) = (&st.drag, pointer) {
            let p = slot_socket.map_or(p, |(socket, k)| p.lerp(socket, 0.6 * k));
            let line = match drag {
                Drag::Wire { from, .. } | Drag::Signal { from, .. } => index.get(from.0.as_str()).map(|&a| bezier(port(a, from.1, true), p, cam.zoom)),
                Drag::Back { into } => index.get(into.0.as_str()).map(|&b| bezier(p, port(b, into.1, false), cam.zoom)),
                _ => None,
            };
            if let Some(line) = line {
                painter.add(Shape::line(line, Stroke::new(width + 1.0, drag_color)));
                painter.circle_filled(p, 3.5 * zs, drag_color);
            }
        }
        // a line drawn across cables with Alt: dashed, the cables it crosses counted at its end
        if let (Some(Drag::Cut { origin }), Some(p)) = (&st.drag, pointer) {
            let crossed = cut(*origin, p).len();
            painter.extend(Shape::dashed_line(&[*origin, p], Stroke::new(1.5, t.fg_text.gamma_multiply(0.75)), 5.0, 4.0));
            for (_, at) in cut(*origin, p) {
                painter.circle_filled(at, 3.0, t.fg_text);
            }
            let text = if crossed == 1 { tr!("1 cable").to_string() } else { tr!("{} cables", crossed) };
            painter.text(p + vec2(12.0, -10.0), Align2::LEFT_BOTTOM, text, FontId::monospace(t.text_xs), t.fg_text);
        }
        if let (Some(Drag::Band { origin, .. }), Some(p)) = (&st.drag, pointer) {
            let band = Rect::from_two_pos(*origin, p);
            painter.rect_filled(band, CornerRadius::same(2), t.select.gamma_multiply(0.06));
            painter.rect_stroke(band, CornerRadius::same(2), Stroke::new(1.0, t.select.gamma_multiply(0.5)), StrokeKind::Inside);
        }
        if let Some((i, j, output)) = hot_port
            && let Some(k) = kinds[i]
        {
            let names = if output { k.outputs } else { k.inputs };
            port_label(&painter, &t, port(i, j, output), names[usize::from(j)], output);
        }
        if nodes.is_empty() {
            empty_state(&painter, &t, view);
        }
        if let Some((at, text)) = map_label {
            let galley = painter.layout_no_wrap(text, t.small(), t.fg_text);
            let pill = Rect::from_min_size(at + vec2(12.0, -28.0), galley.size() + vec2(2.0 * t.pad, t.gap * 2.0));
            painter.rect(pill, CornerRadius::same(t.radius_sm), t.bg_card, Stroke::new(1.0, t.cat_signal.gamma_multiply(0.6)), StrokeKind::Inside);
            painter.galley(pill.center() - galley.size() / 2.0, galley, t.fg_text);
        }
        // a chosen link, a wire or a signal's line, opens the same popover by its middle: what it
        // joins, everything about it to change, what to do with it
        let port_name = |i: usize, p: u8, output: bool| kinds[i].and_then(|k| if output { k.outputs } else { k.inputs }.get(usize::from(p)).copied()).unwrap_or("");
        if st.drag.is_none()
            && let Some(e) = st.wire.as_ref().and_then(|to| scene.edges.iter().position(|e| &e.to == to))
            && let Some((_, pts)) = &wires[e]
            && let (Some(&a), Some(&b)) = (index.get(scene.edges[e].from.0.as_str()), index.get(scene.edges[e].to.0.as_str()))
        {
            let edge = scene.edges[e].clone();
            let title = format!("{} · {}  →  {} · {}", nodes[a].name, port_name(a, edge.from.1, true), nodes[b].name, port_name(b, edge.to.1, false));
            // where it may come from: any output that can feed this input; where it may go: the card's inputs
            let mut froms: Vec<((String, u8), String)> = Vec::new();
            for (i, n) in nodes.iter().enumerate() {
                for p in 0..count(i, true) as u8 {
                    let from = (n.id.clone(), p);
                    if from == edge.from || scene.can_connect((&n.id, p), (&edge.to.0, edge.to.1)).is_ok() {
                        froms.push((from, format!("{} · {}", n.name, port_name(i, p, true))));
                    }
                }
            }
            // an input in use says by what: picking it takes that wire's place
            let intos: Vec<String> = (0..count(b, false) as u8)
                .map(|j| match scene.wire_into(&edge.to.0, j).filter(|_| j != edge.to.1).and_then(|w| index.get(w.from.0.as_str())) {
                    Some(&i) => format!("{} · {}", port_name(b, j, false), nodes[i].name),
                    None => port_name(b, j, false).to_string(),
                })
                .collect();
            let at = along(pts, 0.5) + vec2(12.0, 12.0);
            let mut close = false;
            egui::Area::new(id().with(("wire popover", &edge.to))).order(egui::Order::Foreground).default_pos(at).movable(true).constrain(true).show(ui.ctx(), |ui| {
                kit::popover_frame(ui.ctx(), &t).inner_margin(0).show(ui, |ui| {
                    ui.set_width(LINK_W);
                    close = link_header(ui, &t, &title);
                    egui::Frame::new().inner_margin(t.pad).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(t.gap, t.gap);
                        ui.label(RichText::new(tr!("From")).font(t.body()).color(t.fg_muted));
                        let now = froms.iter().position(|f| f.0 == edge.from);
                        let r = kit::select(ui, &t, None, now.map_or("", |i| froms[i].1.as_str()));
                        kit::menu(&t, &r).show(|ui| {
                            let items: Vec<(&str, &str)> = froms.iter().map(|f| ("", f.1.as_str())).collect();
                            if let Some(i) = kit::choose(ui, &t, &items, now).filter(|&i| Some(i) != now) {
                                map_ops.push(("edge.connect", json!({ "from": [froms[i].0.0, froms[i].0.1], "to": [edge.to.0, edge.to.1] }), None));
                            }
                        });
                        ui.add_space(t.gap);
                        ui.label(RichText::new(tr!("Into")).font(t.body()).color(t.fg_muted));
                        let r = kit::select(ui, &t, None, &intos[usize::from(edge.to.1).min(intos.len().saturating_sub(1))]);
                        kit::menu(&t, &r).show(|ui| {
                            let items: Vec<(&str, &str)> = intos.iter().map(|n| ("", n.as_str())).collect();
                            if let Some(j) = kit::choose(ui, &t, &items, Some(usize::from(edge.to.1))).filter(|&j| j != usize::from(edge.to.1)) {
                                let key = Some("wire".to_string());
                                map_ops.push(("edge.connect", json!({ "from": [edge.from.0, edge.from.1], "to": [edge.to.0, j] }), key.clone()));
                                map_ops.push(("edge.disconnect", json!({ "to": [edge.to.0, edge.to.1] }), key));
                                close_merge = true;
                            }
                        });
                    });
                    kit::menu_line(ui, &t);
                    egui::Frame::new().inner_margin(t.pad).show(ui, |ui| {
                        let shown = kit::actions(ui, &t, &[(kit::Variant::Outline, Some(icons::PLUS), tr!("Insert a node")), (kit::Variant::Outline, Some(icons::TRASH), tr!("Remove"))]);
                        if shown[0].clicked() {
                            let mid = along(pts, 0.5);
                            st.menu = Some(Menu::new(cam.graph(center, mid), Some(edge.from.clone()), Some(edge.to.clone()), None));
                        }
                        if shown[1].clicked() {
                            ops.push(("edge.disconnect", json!({ "to": [edge.to.0, edge.to.1] }), None));
                        }
                    });
                });
            });
            if close {
                st.wire = None;
            }
        }
        if let Some((j, source, at)) = chosen_map {
            let n = &nodes[j];
            let moved: Vec<&velo_core::Mapping> = n.mappings.iter().filter(|m| m.source == source).collect();
            let title = format!("{}  →  {}", crate::mapping_source(scene, &source), n.name);
            // the values it doesn't move yet, to move too, or instead of one it moves
            let all = mappable(n);
            let more: Vec<&(String, String)> = all.iter().filter(|(key, _)| !moved.iter().any(|m| &m.param == key)).collect();
            // what the source reads now, for the sliders' modulation
            let label = crate::mapping_source(scene, &source);
            let level = match velo_core::signal_output(&source) {
                Some((id, port)) => self.live.as_ref().and_then(|l| l.signal(id, port)).unwrap_or(0.0),
                None => velo_core::SOURCES.iter().position(|s| *s == source).map_or(0.0, |i| self.live.as_ref().map(crate::live::Live::sources).unwrap_or_default()[i]),
            };
            let mut close = false;
            egui::Area::new(id().with(("mapping", &n.id, &source))).order(egui::Order::Foreground).default_pos(at + vec2(12.0, 12.0)).movable(true).constrain(true).show(ui.ctx(), |ui| {
                kit::popover_frame(ui.ctx(), &t).inner_margin(0).show(ui, |ui| {
                    ui.set_width(LINK_W);
                    close = link_header(ui, &t, &title);
                    egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
                        for (k, m) in moved.iter().enumerate() {
                            if k > 0 {
                                kit::menu_line(ui, &t);
                            }
                            egui::Frame::new().inner_margin(t.pad).show(ui, |ui| {
                                ui.spacing_mut().item_spacing = vec2(t.gap * 2.0, t.gap * 2.0);
                                let mut edit = widgets::MapEdit::default();
                                ui.horizontal(|ui| {
                                    // what it moves: another value picked here is moved instead, the
                                    // same way
                                    ui.allocate_ui(vec2(ui.available_width() - t.control_h - t.gap, t.control_h + 4.0), |ui| {
                                        let r = kit::select(ui, &t, None, &crate::param_name(n, &m.param));
                                        kit::menu(&t, &r).show(|ui| {
                                            let options: Vec<&(String, String)> = all.iter().filter(|(key, _)| key == &m.param || more.iter().any(|o| &o.0 == key)).collect();
                                            let items: Vec<(&str, &str)> = options.iter().map(|(_, label)| ("", label.as_str())).collect();
                                            let now = options.iter().position(|(key, _)| key == &m.param);
                                            if let Some(i) = kit::choose(ui, &t, &items, now).filter(|&i| Some(i) != now) {
                                                let (to, key) = (&options[i].0, Some("retarget".to_string()));
                                                map_ops.push(("mapping.add", json!({ "node": n.id, "param": to, "source": m.source }), key.clone()));
                                                let how = json!({ "node": n.id, "param": to, "source": m.source, "amount": m.amount, "attack_ms": m.attack_ms, "release_ms": m.release_ms, "invert": m.invert });
                                                map_ops.push(("mapping.set", how, key.clone()));
                                                map_ops.push(("mapping.remove", json!({ "node": n.id, "param": m.param, "source": m.source }), key));
                                                close_merge = true;
                                            }
                                        });
                                    });
                                    kit::right(ui, |ui| edit.remove = widgets::remove_button(ui, &t, tr!("Remove mapping")).clicked());
                                });
                                // the value itself, set here as on its card; its chip opens the
                                // mapping's popover, as it does in the inspector
                                if let Some((info, value)) = param_info(n, &m.param) {
                                    let mods = [widgets::ModView { source: &label, amount: m.amount, level }];
                                    let out = widgets::slider(ui, &t, info, value, &widgets::Marks { mods: &mods, ..Default::default() });
                                    if let Some(v) = out.value {
                                        values.push((n.id.clone(), m.param.clone(), v));
                                    }
                                    close_merge |= out.response.drag_stopped() || out.response.lost_focus();
                                    // Alt-drag on it sets the amount, as on its card
                                    edit.amount = out.amount;
                                    if let Some(chip) = out.badges.first()
                                        && let Some(set) = widgets::mapping_popup(&t, chip, &format!("{label} → {}", crate::param_name(n, &m.param)), m)
                                    {
                                        crate::map_ops(&set, &n.id, m, &mut map_ops);
                                        close_merge |= set.done;
                                    }
                                }
                                let fields = widgets::mapping_fields(ui, &t, m);
                                edit = widgets::MapEdit { remove: edit.remove, amount: fields.amount.or(edit.amount), ..fields };
                                crate::map_ops(&edit, &n.id, m, &mut map_ops);
                                close_merge |= edit.done;
                            });
                        }
                    });
                    kit::menu_line(ui, &t);
                    egui::Frame::new().inner_margin(t.pad).show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(t.gap, t.gap);
                        // the same link moves another value: picked from what's left
                        ui.add_enabled_ui(!more.is_empty(), |ui| {
                            let r = kit::select(ui, &t, Some(icons::PLUS), tr!("Move another value too"));
                            kit::menu(&t, &r).show(|ui| {
                                let items: Vec<(&str, &str)> = more.iter().map(|(_, label)| ("", label.as_str())).collect();
                                if let Some(i) = kit::choose(ui, &t, &items, None) {
                                    map_ops.push(("mapping.add", json!({ "node": n.id, "param": more[i].0, "source": source }), None));
                                }
                            });
                        });
                        if kit::actions(ui, &t, &[(kit::Variant::Outline, Some(icons::TRASH), tr!("Remove all"))])[0].clicked() {
                            for m in &moved {
                                map_ops.push(("mapping.remove", json!({ "node": n.id, "param": m.param, "source": m.source }), Some("unmap".to_string())));
                            }
                            close_merge = true;
                            close = true;
                        }
                    });
                });
            });
            if close {
                st.map = None;
            }
        }

        // ---- feedback ----
        let space_pan = space && st.hovered;
        let cursor = match (&st.drag, hover) {
            (Some(Drag::Pan { .. } | Drag::Nodes { .. } | Drag::Tie { .. } | Drag::Signal { .. }), _) => Some(CursorIcon::Grabbing),
            (Some(Drag::Cut { .. }), _) => Some(CursorIcon::Crosshair),
            (None, _) if space_pan => Some(CursorIcon::Grab),
            (None, Some(Hit::Tie(_))) => Some(CursorIcon::Grab),
            (None, Some(Hit::Untie(_))) => Some(CursorIcon::PointingHand),
            (None, Some(Hit::Empty | Hit::Wire(_) | Hit::Map(..))) if tool => Some(CursorIcon::Crosshair),
            (None, Some(Hit::Node(i))) if pointer.is_some_and(|p| power(i).is_some_and(|b| b.contains(p))) => Some(CursorIcon::PointingHand),
            (None, Some(Hit::Port(..))) => Some(CursorIcon::Crosshair),
            (None, Some(Hit::Map(..))) => Some(CursorIcon::PointingHand),
            (None, Some(Hit::Wire(_))) => Some(CursorIcon::Grab),
            _ => None,
        };
        if let Some(c) = cursor {
            ui.ctx().set_cursor_icon(c);
        }
        let hint = match hover {
            Some(Hit::Empty | Hit::Wire(_) | Hit::Map(..)) if tool => Some(tr!("Drag a line across cables to tie them")),
            Some(Hit::Port(_, _, true)) => Some(tr!("Drag to an input to wire it · drop on empty canvas to pick a node")),
            Some(Hit::Port(i, j, false)) if wired_in.contains_key(&(nodes[i].id.as_str(), j)) => Some(tr!("Drag to move this wire · drop it anywhere else to unplug it")),
            Some(Hit::Port(_, _, false)) => Some(tr!("Drag to an output to wire it")),
            Some(Hit::Node(i)) if pointer.is_some_and(|p| power(i).is_some_and(|b| b.contains(p))) => Some(if nodes[i].off { tr!("Turn on") } else { tr!("Turn off") }),
            Some(Hit::Node(_)) => Some(tr!("Drag to move · Shift+click adds to the selection · double-click to rename")),
            Some(Hit::Wire(_)) => Some(tr!("Drag the wire into another input or off · click for all about it · Alt+click removes it")),
            Some(Hit::Map(..)) => Some(tr!("A signal moving values of this node: click to set them or move another · Alt+click removes it")),
            Some(Hit::Tie(_)) => Some(tr!("A cable tie: drag it along its cables · click it, then Delete, or its × takes it off")),
            Some(Hit::Untie(_)) => Some(tr!("Take the tie off")),
            Some(Hit::Empty) if cfg!(target_os = "macos") => Some(tr!("Tab adds a node · drag to select · Alt+drag across cables ties them · scroll to pan · Cmd+scroll zooms · H frames all")),
            Some(Hit::Empty) => Some(tr!("Tab adds a node · drag to select · Alt+drag across cables ties them · scroll to pan · Ctrl+scroll zooms · H frames all")),
            _ => None,
        };
        if settled {
            st.cameras.insert(scene.id.clone(), cam);
        }
        let scene_id = scene.id.clone();
        // Figma's zoom readout in the corner, with its menu
        let map = &self.settings.keymap;
        let keys_of = |id: &str| map.keys(id).first().map(keymap::text).unwrap_or_default();
        // the right button: a node's own menu, or the add menu on the empty canvas
        if bg.secondary_clicked()
            && let Some(p) = bg.interact_pointer_pos()
        {
            st.context = match hit(p) {
                Hit::Node(i) | Hit::Port(i, ..) => {
                    let id = nodes[i].id.clone();
                    if !st.selected.contains(&id) {
                        st.selected = vec![id.clone()];
                    }
                    Some((Some(id), p))
                }
                _ => Some((None, p)),
            };
        }
        let (target, mut act) = (st.context.clone(), None);
        kit::context_menu(&t, &bg).show(|ui| match &target {
            Some((Some(id), _)) => {
                let node = index.get(id.as_str()).map(|&i| &nodes[i]);
                let has_props = node.is_some_and(|n| crate::live::shader(&n.kind, n.code.as_deref()).is_some());
                if ui.add_enabled(has_props, egui::Button::new(format!("{}  {}", icons::SLIDERS_HORIZONTAL, tr!("Parameters")))).clicked() {
                    act = Some(Act::Props(id.clone()));
                }
                if ui.button(format!("{}  {}", icons::PENCIL_SIMPLE, tr!("Rename…"))).clicked() {
                    act = node.map(|n| Act::Rename(n.id.clone(), n.name.clone()));
                }
                if ui.add(egui::Button::new(format!("{}  {}", icons::COPY, tr!("Duplicate"))).shortcut_text(keys_of("canvas.duplicate"))).clicked() {
                    act = Some(Act::Duplicate);
                }
                if let Some(node) = node.filter(|n| !node_kind(&n.kind).is_some_and(|k| k.category == velo_core::Category::Output))
                    && ui.add(egui::Button::new(format!("{}  {}", icons::POWER, if node.off { tr!("Turn on") } else { tr!("Turn off") })).shortcut_text(keys_of("canvas.toggle"))).clicked()
                {
                    act = Some(Act::Toggle(node.off));
                }
                if ui.button(format!("{}  {}", icons::ARROWS_LEFT_RIGHT, tr!("Replace with…"))).clicked() {
                    act = target.as_ref().map(|(_, p)| Act::Replace(id.clone(), *p));
                }
                let wired = scene.edges.iter().any(|e| &e.from.0 == id || &e.to.0 == id);
                if ui.add_enabled(wired, egui::Button::new(format!("{}  {}", icons::LINK_BREAK, tr!("Disconnect")))).clicked() {
                    act = Some(Act::Disconnect(id.clone()));
                }
                ui.separator();
                if ui.add(egui::Button::new(RichText::new(format!("{}  {}", icons::TRASH, tr!("Delete"))).color(t.error)).shortcut_text(keys_of("canvas.delete"))).clicked() {
                    act = Some(Act::Delete);
                }
            }
            Some((None, p)) => {
                if ui.add(egui::Button::new(format!("{}  {}", icons::PLUS, tr!("Add a node…"))).shortcut_text(keys_of("canvas.add"))).clicked() {
                    act = Some(Act::Add(*p));
                }
                if ui.add(egui::Button::new(format!("{}  {}", icons::CROSSHAIR, tr!("Zoom to fit"))).shortcut_text(keys_of("canvas.home"))).clicked() {
                    act = Some(Act::Fit);
                }
                if ui.button(format!("{}  {}", icons::TREE_STRUCTURE, tr!("Arrange the nodes"))).clicked() {
                    act = Some(Act::Arrange);
                }
            }
            None => {}
        });
        let (duplicate, delete) = (matches!(act, Some(Act::Duplicate)), matches!(act, Some(Act::Delete)));
        match act {
            Some(Act::Props(id)) => (st.props, st.selected) = (Some(id.clone()), vec![id]),
            Some(Act::Rename(id, name)) => rename = Some((id, name)),
            Some(Act::Toggle(on)) => ops.push(("node.toggle", json!({ "nodes": st.selected, "on": on }), None)),
            Some(Act::Disconnect(id)) => {
                for e in scene.edges.iter().filter(|e| e.from.0 == id || e.to.0 == id) {
                    ops.push(("edge.disconnect", json!({ "to": [e.to.0, e.to.1] }), Some("disconnect")));
                }
                close_merge = true;
            }
            Some(Act::Add(p)) => st.menu = Some(Menu::new(cam.graph(center, p), None, None, None)),
            Some(Act::Replace(id, p)) => st.menu = Some(Menu::new(cam.graph(center, p), None, None, Some(id))),
            Some(Act::Fit) => st.zoom_to = Some(ZoomTo::Fit),
            Some(Act::Arrange) => st.arrange = true,
            _ => {}
        }
        let corner = view.right_bottom() - vec2(t.pad, t.pad);
        kit::pill(ui.ctx(), &t, "zoom", corner, Align2::RIGHT_BOTTOM, |ui| {
            let r = kit::action(ui, &t, kit::Variant::Ghost, None, &format!("{:.0}%  {}", cam.zoom * 100.0, icons::CARET_DOWN)).on_hover_text(tr!("Zoom"));
            kit::menu(&t, &r).show(|ui| {
                let items = [
                    (tr!("Zoom in"), "canvas.zoom_in", ZoomTo::In),
                    (tr!("Zoom out"), "canvas.zoom_out", ZoomTo::Out),
                    (tr!("Zoom to fit"), "canvas.home", ZoomTo::Fit),
                    (tr!("Zoom to selection"), "canvas.zoom_selection", ZoomTo::Selection),
                    ("50%", "", ZoomTo::Level(0.5)),
                    ("100%", "canvas.zoom_100", ZoomTo::Level(1.0)),
                    ("200%", "", ZoomTo::Level(2.0)),
                ];
                for (i, (label, command, to)) in items.into_iter().enumerate() {
                    if i == 2 || i == 4 {
                        ui.separator();
                    }
                    let enabled = to != ZoomTo::Selection || !st.selected.is_empty();
                    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys_of(command))).clicked() {
                        st.zoom_to = Some(to);
                        ui.close();
                    }
                }
            });
        });

        // the dropdown stays while its node is the one selected
        if st.props.as_ref().is_some_and(|p| st.selected.as_slice() != std::slice::from_ref(p)) {
            st.props = None;
        }
        let dropdown = st.props.as_ref().and_then(|p| index.get(p.as_str()).map(|&i| (p.clone(), rects[i])));
        for (op, props, key) in ops {
            match key {
                Some(key) => self.run_merge(op, props, key),
                None => self.run(op, props),
            }
        }
        for (op, props, key) in map_ops {
            match key {
                Some(key) => self.run_merge(op, props, &key),
                None => self.run(op, props),
            }
        }
        for (node, key, v) in values {
            self.set_param(&node, &key, v, "link value");
        }
        if close_merge {
            self.session.close_merge();
        }
        if let Some((node, name)) = rename {
            self.rename = Some(shell::Renaming::node(&scene_id, &node, &name));
        }
        if let Some(hint) = hint {
            self.info_next = hint.into();
        }
        if let Some((node, card)) = dropdown {
            self.props_dropdown(ui.ctx(), &t, &node, card);
        }
        self.param_picker(ui.ctx(), &t);
        if duplicate {
            self.duplicate_nodes();
        }
        if delete {
            self.delete_selection();
        }
    }

    /// A signal dropped on a card: the menu of that node's parameters it can move (numbers, a
    /// vec2 by axis; a color by its hue); picking one maps it.
    fn param_picker(&mut self, ctx: &egui::Context, t: &Theme) {
        let Some((source, node, at)) = self.canvas.pick.clone() else { return };
        let target = self.session.active().and_then(|s| s.node(&node)).cloned();
        let Some(keys) = target.as_ref().filter(|n| crate::live::shader(&n.kind, n.code.as_deref()).is_some()).map(mappable) else {
            self.canvas.pick = None;
            return;
        };
        let (mut picked, mut close) = (None, false);
        // a menu like every other: its lines, and a search once the node has many parameters
        let area = egui::Area::new(egui::Id::unique("signal-pick")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            kit::menu_rows(t).apply(ui.style_mut());
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(240.0);
                kit::menu_title(ui, t, tr!("MOVE WITH THIS SIGNAL"));
                let items: Vec<(&str, &str)> = keys.iter().map(|(_, label)| ("", label.as_str())).collect();
                picked = kit::choose(ui, t, &items, None).map(|i| keys[i].0.clone());
                if keys.is_empty() {
                    kit::menu_title(ui, t, tr!("Nothing here moves with a signal"));
                }
            });
        });
        if area.response.clicked_elsewhere() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        if let Some(param) = picked {
            self.run_merge("mapping.add", json!({ "node": node, "param": param, "source": source }), "remap");
            self.canvas.plug_ins.push(((node.clone(), source.clone()), f64::NAN));
            // a cable moved here from another card: that card lets go of it, one undo step
            if let Some((old, from)) = self.canvas.repick.take().filter(|(_, s)| *s == source) {
                let params: Vec<String> = self.session.active().and_then(|s| s.node(&old)).map(|n| n.mappings.iter().filter(|m| m.source == from).map(|m| m.param.clone()).collect()).unwrap_or_default();
                for p in params {
                    self.run_merge("mapping.remove", json!({ "node": old, "param": p, "source": from }), "remap");
                }
            }
            self.session.close_merge();
            close = true;
        }
        if close {
            (self.canvas.pick, self.canvas.repick) = (None, None);
        }
    }

    /// A node's parameters dropped down under its card, as the inspector shows them: a header
    /// with its name and a close button, then its sections, scrolling past a screenful.
    fn props_dropdown(&mut self, ctx: &egui::Context, t: &Theme, node: &str, card: Rect) {
        let Some(n) = self.session.active().and_then(|s| s.node(node)).cloned() else { return };
        let kind = node_kind(&n.kind);
        let width = card.width().max(280.0);
        let mut close = false;
        // as tall as the window leaves under the card, at most a screenful of controls
        let at = card.left_bottom() + vec2(0.0, 6.0);
        let room = (ctx.content_rect().bottom() - at.y - t.bar_h - 16.0).clamp(120.0, 440.0);
        // on the menus' layer, not over it: its own menus and popovers open on top of it
        egui::Area::new(egui::Id::unique(("props", node))).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            let frame = Frame::new().fill(t.bg_panel).stroke(Stroke::new(1.0, t.line)).corner_radius(t.radius_md).shadow(ctx.global_style().visuals.popup_shadow);
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                kit::bar(ui, t, |ui| {
                    ui.add_space(t.pad);
                    let dot = kind.map_or(t.fg_muted, |k| t.category(k.category));
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, t.control_h), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.0, dot);
                    ui.label(RichText::new(&n.name).strong().color(t.fg_text));
                    // the kind, when the name doesn't say it
                    if let Some(k) = kind.filter(|k| !n.name.starts_with(i18n::t(k.label))) {
                        ui.label(RichText::new(i18n::t(k.label)).small().color(t.fg_muted));
                    }
                    kit::right(ui, |ui| {
                        ui.add_space(t.gap);
                        close = kit::icon_button(ui, t, icons::X, tr!("Close"), false).clicked();
                    });
                });
                let line = ui.available_rect_before_wrap();
                ui.painter().hline(line.x_range(), line.top(), Stroke::new(1.0, t.line));
                // the scroll bar sits inside the card's edge, not on it
                Frame::new().inner_margin(Margin { left: 0, right: 4, top: 1, bottom: 4 }).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                        Frame::new().inner_margin(Margin { left: t.pad as i8, right: (t.pad - 4.0) as i8, top: t.gap as i8, bottom: t.pad as i8 }).show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(t.gap, t.gap);
                            let media = velo_core::shows_media(&n.kind);
                            if media {
                                self.media_section(ui, t, node, true);
                            }
                            if n.kind == "velo.grid" {
                                self.grid_section(ui, t, node, true);
                            } else {
                                self.node_params(ui, t, node, !media);
                            }
                        });
                    });
                });
            });
        });
        if close {
            self.canvas.props = None;
        }
    }

    /// TouchDesigner's OP Create dialog as a modal: type to filter, a tab per group (All first;
    /// the ones a search leaves empty are dimmed), ↑ ↓ and Enter, or click.
    pub fn node_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = &mut self.canvas.menu else { return };
        let t = self.theme.clone();
        // VELO_OPEN_NODE_MENU=<words>: opened with those words searched (screenshots)
        static DEMO: std::sync::Once = std::sync::Once::new();
        DEMO.call_once(|| {
            if let Some(q) = std::env::var("VELO_OPEN_NODE_MENU").ok().filter(|q| q != "1") {
                menu.query = q;
            }
        });
        let words: Vec<String> = menu.query.to_lowercase().split_whitespace().map(String::from).collect();
        let wiring = menu.from.is_some();
        let into = menu.into.is_some();
        let fits = |k: &&NodeKind| {
            let text = format!("{} {} {} {} {}", k.label, i18n::t(k.label), i18n::t(k.category.label()), i18n::t(k.group()), k.id).to_lowercase();
            // a dropped wire needs something with an input; one put into a wire, an output too
            (!wiring || !k.inputs.is_empty()) && (!into || !k.outputs.is_empty()) && words.iter().all(|w| text.contains(w.as_str()))
        };
        let found: Vec<&'static NodeKind> = NODE_KINDS.iter().filter(fits).collect();
        // the tabs: All, then the groups in the menu's order; one with nothing found is off
        let mut tabs: Vec<Option<&'static str>> = vec![None];
        for k in NODE_KINDS {
            if !tabs.contains(&Some(k.group())) {
                tabs.push(Some(k.group()));
            }
        }
        let has = |g: Option<&str>| g.is_none_or(|g| found.iter().any(|k| k.group() == g));
        // Ctrl+Tab (Shift: back) walks the tabs that have something
        let (back, next) = ctx.input_mut(|i| (i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab), i.consume_key(Modifiers::CTRL, Key::Tab)));
        let mut tab = menu.tab.filter(|&g| has(Some(g)));
        if back || next {
            let step = if next { 1 } else { tabs.len() - 1 };
            let mut i = tabs.iter().position(|&g| g == tab).unwrap_or(0);
            i = (i + step) % tabs.len();
            while !has(tabs[i]) {
                i = (i + step) % tabs.len();
            }
            (menu.tab, tab, menu.selected, menu.top) = (tabs[i], tabs[i], 0, true);
        }
        let matches: Vec<&'static NodeKind> = found.iter().copied().filter(|k| tab.is_none_or(|g| k.group() == g)).collect();
        // what the new node will be wired after, for the footer
        let after = append_source(&self.session, &self.canvas.selected, menu.from.as_ref()).and_then(|(id, _)| self.session.active()?.node(&id).map(|n| n.name.clone()));
        let mut chosen = None;
        let title = if menu.replace.is_some() { tr!("Replace a node") } else { tr!("Add a node") };
        let (tab_key, close) = kit::modal_flush(ctx, &t, "node-menu", title, MENU_W, |ui| {
            let field = Margin { left: t.pad as i8, right: t.pad as i8, top: t.pad as i8, bottom: t.gap as i8 };
            let r = egui::Frame::new().inner_margin(field).show(ui, |ui| ui.add(kit::field(&mut menu.query).hint_text(format!("{}  {}", icons::MAGNIFYING_GLASS, tr!("Search nodes…"))).desired_width(f32::INFINITY))).inner;
            r.request_focus();
            if r.changed() {
                (menu.selected, menu.top) = (0, true);
            }
            let items: Vec<(Option<&'static str>, &str, &str)> = tabs.iter().map(|&g| (g, group_icon(g), g.map_or(tr!("All"), i18n::t))).collect();
            egui::Frame::new().inner_margin(Margin { left: t.pad as i8, right: t.pad as i8, top: 0, bottom: t.pad as i8 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if let Some(g) = kit::tabs(ui, &t, Id::unique("node-menu-tabs"), tab, &items, false, |g| !has(g)) {
                        (menu.tab, menu.selected, menu.top) = (g, 0, true);
                    }
                });
            });
            let line = ui.available_rect_before_wrap();
            ui.painter().hline(line.x_range(), line.top() - 0.5, Stroke::new(1.0, t.line));
            let (up, down, enter, tab_key) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                    i.consume_key(Modifiers::NONE, Key::Tab),
                )
            });
            let last = matches.len().saturating_sub(1);
            menu.selected = if down {
                (menu.selected + 1).min(last)
            } else if up {
                menu.selected.saturating_sub(1)
            } else {
                menu.selected.min(last)
            };
            // as tall whatever it shows, so the modal holds still while you type
            let list = egui::ScrollArea::vertical().max_height(LIST_H).auto_shrink([false, false]);
            // egui keeps scroll offsets (even across runs): each opening, search and tab starts at the top
            let list = if std::mem::take(&mut menu.top) { list.vertical_scroll_offset(0.0) } else { list };
            list.show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut group = None;
                for (i, k) in matches.iter().enumerate() {
                    // the groups' names in All; a tab says it already
                    if tab.is_none() && group != Some(k.group()) {
                        group = Some(k.group());
                        let top = if i == 0 { t.gap * 1.5 } else { t.gap * 3.0 };
                        egui::Frame::new().inner_margin(Margin { left: t.pad as i8, right: t.pad as i8, top: top as i8, bottom: t.gap as i8 }).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            kit::caps(ui, &t, &i18n::t(k.group()).to_uppercase());
                        });
                    }
                    let r = kit::row_sized(ui, &t, Id::unique(("node-menu-row", k.id)), i == menu.selected, MENU_ROW_H);
                    let mut painter = ui.painter_at(r.rect);
                    painter.multiply_opacity(kit::row_alpha(ui, &r));
                    // its icon on a tile of its category's color, its name over what it does
                    let color = t.category(k.category);
                    let tile = Rect::from_center_size(pos2(r.rect.left() + t.pad + 15.0, r.rect.center().y), vec2(30.0, 30.0));
                    painter.rect_filled(tile, t.radius_sm, color.gamma_multiply(0.16));
                    kit::icon_at(&painter, tile.center(), node_icon(k.id), 17.0, color);
                    let x = tile.right() + t.pad + 2.0;
                    painter.text(pos2(x, r.rect.center().y), Align2::LEFT_BOTTOM, i18n::t(k.label), t.body(), t.fg_text);
                    if let Some(about) = node_blurb(k.id) {
                        kit::fit_text(&painter, pos2(x, r.rect.center().y + 2.0), Align2::LEFT_TOP, about, t.small(), t.fg_muted, r.rect.right() - t.pad - x);
                    }
                    if r.clicked() {
                        chosen = Some(*k);
                    }
                    if i == menu.selected && (up || down) {
                        r.scroll_to_me(None);
                    }
                }
                if matches.is_empty() {
                    egui::Frame::new().inner_margin(t.pad).show(ui, |ui| ui.label(egui::RichText::new(tr!("Nothing found")).color(t.fg_muted)));
                }
            });
            let foot = ui.available_rect_before_wrap();
            ui.painter().hline(foot.x_range(), foot.top() - 0.5, Stroke::new(1.0, t.line));
            egui::Frame::new().inner_margin(Margin::symmetric(t.pad as i8, (t.gap * 1.5) as i8)).show(ui, |ui| {
                let keys = match &after {
                    Some(name) => tr!("↑ ↓ choose · Ctrl+Tab group · Enter adds it after {} · Esc closes", name),
                    None => tr!("↑ ↓ choose · Ctrl+Tab group · Enter adds · Esc closes").to_string(),
                };
                let text = match matches.len() {
                    0 => keys,
                    1 => format!("{} · {keys}", tr!("1 node")),
                    n => format!("{} · {keys}", tr!("{} nodes", n)),
                };
                ui.label(egui::RichText::new(text).small().color(t.fg_muted));
            });
            if enter {
                chosen = matches.get(menu.selected).copied();
            }
            tab_key
        });
        if let Some(kind) = chosen {
            let (graph, from, into, replace) = (menu.graph, menu.from.take(), menu.into.take(), menu.replace.take());
            self.canvas.menu = None;
            match replace {
                Some(node) => self.replace_node(&node, kind),
                None => self.add_node(kind, graph, from, into),
            }
        } else if close || tab_key {
            self.canvas.menu = None;
        }
        if self.canvas.menu.is_none() {
            ctx.memory_mut(|m| m.request_focus(id()));
        }
    }

    /// Turns a node into another kind where it stands: the values and mappings of parameters
    /// the new kind has too come along (a Blur's mix into a Kaleido's mix), the wires that fit.
    fn replace_node(&mut self, node: &str, kind: &'static NodeKind) {
        let Some(n) = self.session.active().and_then(|s| s.node(node)).cloned() else { return };
        let code = velo_engine::template(kind.id);
        let has = |param: &str| crate::live::shader(kind.id, code).is_some_and(|s| s.controls.iter().any(|(p, _)| p.id == param.split('.').next().unwrap_or(param)));
        let params: BTreeMap<&String, &[f32; 4]> = n.params.iter().filter(|(p, _)| has(p)).collect();
        let mappings: Vec<&velo_core::Mapping> = n.mappings.iter().filter(|m| has(&m.param)).collect();
        // a Shader node starts from the template
        self.run("node.replace", json!({ "node": node, "kind": kind.id, "params": params, "mappings": mappings, "code": code }));
    }

    /// Adds a node from the menu. With a dropped wire, or one node selected, it's wired from
    /// that output and placed after it (TouchDesigner's append); otherwise it goes at `at`.
    /// `into`: the input it then feeds (a node put into a wire).
    fn add_node(&mut self, kind: &'static NodeKind, at: Pos2, from: Option<(String, u8)>, into: Option<(String, u8)>) {
        let dropped = from.is_some();
        let source = append_source(&self.session, &self.canvas.selected, from.as_ref()).filter(|_| !kind.inputs.is_empty());
        let Some(scene) = self.session.active() else { return };
        let mut pos = match (&source, dropped) {
            // its input where the wire was let go
            (Some(_), true) => at - vec2(0.0, HEADER_H + VIEWER_H / 2.0),
            (Some((id, _)), false) => scene.node(id).map_or(at, |n| pos2(n.pos[0] + NODE_W + SPACING, n.pos[1])),
            (None, _) => at - vec2(NODE_W / 2.0, HEADER_H / 2.0),
        };
        let size = vec2(NODE_W, NODE_H);
        let taken = |p: Pos2| scene.nodes.iter().any(|n| plane_room(scene, n).expand(8.0).intersects(Rect::from_min_size(p, size)));
        for _ in 0..50 {
            if !taken(pos) {
                break;
            }
            pos.y += NODE_H + SPACING / 2.0;
        }
        let before = scene.nodes.len();
        // node and wire are one undo step
        // a Shader node starts from its first preset, named after it
        let code = velo_engine::template(kind.id);
        let name = code.and_then(|c| velo_engine::ShaderNode::parse(c).ok()).map_or_else(|| i18n::t(kind.label).to_string(), |n| i18n::t(&n.name).to_string());
        self.run_merge("node.add", json!({ "kind": kind.id, "pos": [pos.x, pos.y], "name": name, "code": code }), "node.add");
        let added = self.session.active().filter(|s| s.nodes.len() > before).and_then(|s| s.nodes.last()).map(|n| n.id.clone());
        if let (Some(new), Some((src, port))) = (&added, source) {
            self.run_merge("edge.connect", json!({ "from": [src, port], "to": [new, 0] }), "node.add");
        }
        if let (Some(new), Some(to)) = (&added, into) {
            self.run_merge("edge.connect", json!({ "from": [new, 0], "to": [to.0, to.1] }), "node.add");
            // its light starts when the canvas next draws (it knows the time)
            self.canvas.pulses.push((to, f64::NAN));
        }
        self.session.close_merge();
        if let Some(new) = added {
            self.canvas.selected = vec![new.clone()];
            self.canvas.wire = None;
            self.canvas.reveal = Some(new);
        }
    }

    /// Imports an ISF shader file (`.fs`) as a Shader node, right of the others, selected; it
    /// says what it found, or why it won't run.
    pub fn import_isf(&mut self, path: &std::path::Path) {
        let code = match std::fs::metadata(path).map(|m| m.len()) {
            Ok(n) if n <= 1 << 20 => std::fs::read_to_string(path).map_err(|e| e.to_string()),
            Ok(_) => Err(tr!("too big for a shader").to_string()),
            Err(e) => Err(e.to_string()),
        };
        let code = match code {
            Ok(c) if velo_engine::isf::is_isf(&c) => c,
            Ok(_) => return self.say(Tone::Error, tr!("{} isn't an ISF shader (a GLSL file under a /*{{ \"INPUTS\" … }}*/ header)", path.display())),
            Err(e) => return self.say(Tone::Error, tr!("Can't read {}: {}", path.display(), e)),
        };
        if self.session.active().is_none() {
            self.run("scene.add", json!({}));
        }
        let Some(scene) = self.session.active() else { return };
        let right = scene.nodes.iter().map(|n| n.pos[0]).fold(f32::MIN, f32::max);
        let pos = if right == f32::MIN { [0.0, 0.0] } else { [right + NODE_W + SPACING, 0.0] };
        let name = path.file_stem().map_or_else(|| "ISF".to_string(), |s| s.to_string_lossy().into_owned());
        // an image input makes it an effect
        let kind = if velo_engine::own(&code).is_ok_and(|n| !n.inputs.is_empty()) { velo_core::SHADER_FX } else { velo_core::SHADER };
        self.run("node.add", json!({ "kind": kind, "pos": pos, "name": name, "code": code }));
        let Some(new) = self.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) else { return };
        self.canvas.selected = vec![new.clone()];
        self.canvas.reveal = Some(new);
        match velo_engine::own(&code).and_then(|n| n.validate().map(|_| n)) {
            Ok(n) => self.say(Tone::Done, tr!("Imported {}: {} parameters{}", name, n.params.len(), if n.inputs.is_empty() { "" } else { ", one image in" })),
            Err(e) => self.say(Tone::Error, tr!("Imported {}, but it doesn't run here yet: {}", name, e)),
        }
    }

    /// A picture, a GIF or a folder of frames as a new Image node, a movie as a Video node, right of the scene's nodes
    /// (a new scene if there's none). It shows once the file's decoded.
    pub fn import_media(&mut self, path: &std::path::Path) {
        if self.session.active().is_none() {
            self.run("scene.add", json!({}));
        }
        let Some(scene) = self.session.active() else { return };
        let right = scene.nodes.iter().map(|n| n.pos[0]).fold(f32::MIN, f32::max);
        let pos = if right == f32::MIN { [0.0, 0.0] } else { [right + NODE_W + SPACING, 0.0] };
        let name = path.file_stem().map_or_else(|| "Image".to_string(), |s| s.to_string_lossy().into_owned());
        // a movie makes a Video node, anything else an Image node
        let movie = path.extension().is_some_and(|e| velo_engine::media::MOVIES.iter().any(|m| e.eq_ignore_ascii_case(m)));
        let kind = if movie { velo_core::VIDEO } else { velo_core::IMAGE };
        self.run("node.add", json!({ "kind": kind, "pos": pos, "name": name, "media": path.to_string_lossy() }));
        let Some(new) = self.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) else { return };
        self.canvas.selected = vec![new.clone()];
        self.canvas.reveal = Some(new);
    }

    /// Several pictures or movies dropped at once: a Media List of them (eight at most), right
    /// of the scene's nodes, into an Output with nothing in yet, selected; one undo step.
    pub fn import_media_set(&mut self, paths: &[std::path::PathBuf]) {
        const KEY: &str = "media set";
        if self.session.active().is_none() {
            self.run("scene.add", json!({}));
        }
        let Some(scene) = self.session.active() else { return };
        let right = scene.nodes.iter().map(|n| n.pos[0]).fold(f32::MIN, f32::max);
        let x = if right == f32::MIN { 0.0 } else { right + NODE_W + SPACING };
        let output = scene.nodes.iter().find(|n| n.kind == velo_core::OUTPUT && scene.wire_into(&n.id, 0).is_none()).map(|n| n.id.clone());
        let paths = &paths[..paths.len().min(velo_core::MEDIA_LIST_FILES)];
        let media: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        self.run_merge("node.add", json!({ "kind": velo_core::MEDIA_LIST, "pos": [x, 0.0], "media": media.join("\n") }), KEY);
        let Some(list) = self.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) else { return };
        if let Some(out) = output {
            self.run_merge("edge.connect", json!({ "from": [list, 0], "to": [out, 0] }), KEY);
        }
        self.session.close_merge();
        self.canvas.selected = vec![list.clone()];
        self.canvas.reveal = Some(list);
        self.say(Tone::Done, tr!("{} files in a Media List, a new one every 4 beats", paths.len()));
    }

    pub fn import_isf_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter(tr!("ISF shader"), &["fs", "frag", "glsl"]).pick_file() {
            self.import_isf(&path);
        }
    }

    pub fn toggle_nodes(&mut self) {
        if !self.canvas.selected.is_empty() {
            self.run("node.toggle", json!({ "nodes": self.canvas.selected }));
        }
    }

    /// Duplicates the selection, wires among it included, a little down and to the right.
    pub fn duplicate_nodes(&mut self) {
        let Some(scene) = self.session.active() else { return };
        let picked: Vec<_> = scene.nodes.iter().filter(|n| self.canvas.selected.contains(&n.id)).cloned().collect();
        let inner: Vec<_> = scene.edges.iter().filter(|e| picked.iter().any(|p| p.id == e.from.0) && picked.iter().any(|p| p.id == e.to.0)).cloned().collect();
        let mut copies = HashMap::new();
        for n in picked {
            let [x, y] = n.pos;
            self.run_merge("node.add", json!({ "kind": n.kind, "pos": [x + 32.0, y + 32.0], "name": n.name, "params": n.params, "mappings": n.mappings, "code": n.code, "media": n.media, "icon": n.icon }), "duplicate");
            if let Some(copy) = self.session.active().and_then(|s| s.nodes.last()) {
                let id = copy.id.clone();
                if n.off {
                    self.run_merge("node.toggle", json!({ "nodes": [id], "on": false }), "duplicate");
                }
                copies.insert(n.id, id);
            }
        }
        for e in inner {
            if let (Some(a), Some(b)) = (copies.get(&e.from.0), copies.get(&e.to.0)) {
                self.run_merge("edge.connect", json!({ "from": [a, e.from.1], "to": [b, e.to.1] }), "duplicate");
            }
        }
        self.session.close_merge();
        self.canvas.selected = copies.into_values().collect();
    }

    /// Delete / Backspace: the selected wire, else the selected nodes (one undo step).
    pub fn delete_selection(&mut self) {
        if let Some(tie) = self.canvas.tie.take() {
            self.run("tie.remove", json!({ "tie": tie }));
            return;
        }
        if let Some((node, source)) = self.canvas.map.take() {
            let params: Vec<String> = self.session.active().and_then(|s| s.node(&node)).map(|n| n.mappings.iter().filter(|m| m.source == source).map(|m| m.param.clone()).collect()).unwrap_or_default();
            for param in params {
                self.run_merge("mapping.remove", json!({ "node": node, "param": param, "source": source }), "unmap");
            }
            self.session.close_merge();
            return;
        }
        if let Some(to) = self.canvas.wire.take() {
            self.run("edge.disconnect", json!({ "to": [to.0, to.1] }));
            return;
        }
        for node in std::mem::take(&mut self.canvas.selected) {
            self.run_merge("node.remove", json!({ "node": node }), "node.remove");
        }
        self.session.close_merge();
    }

    pub fn select_all_nodes(&mut self) {
        self.canvas.selected = self.session.active().map(|s| s.nodes.iter().map(|n| n.id.clone()).collect()).unwrap_or_default();
        self.canvas.wire = None;
    }
}

/// The output a new node gets wired from: a dropped wire's, else the one selected node's first.
fn append_source(session: &Session, selected: &[String], from: Option<&(String, u8)>) -> Option<(String, u8)> {
    if let Some(from) = from {
        return Some(from.clone());
    }
    let [one] = selected else { return None };
    let node = session.active()?.node(one)?;
    node_kind(&node.kind).filter(|k| !k.outputs.is_empty()).map(|_| (node.id.clone(), 0))
}

/// An empty scene says how to start: a key cap, painted (labels would take the canvas' clicks).
fn empty_state(painter: &Painter, t: &Theme, rect: Rect) {
    let c = rect.center() - vec2(0.0, 24.0);
    // the V of light: the brand pack's icon, not an icon-font glyph
    let mark = Rect::from_center_size(c - vec2(0.0, 72.0), vec2(72.0, 72.0));
    painter.image(widgets::brand(painter.ctx(), widgets::ICON_256), mark, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Theme::NO_TINT);
    painter.text(c - vec2(0.0, 10.0), Align2::CENTER_CENTER, tr!("No nodes yet"), t.body(), t.fg_text);
    painter.text(c + vec2(0.0, 10.0), Align2::CENTER_CENTER, tr!("Press Tab to add one, or double-click anywhere."), t.body(), t.fg_muted);
    let galley = painter.layout_no_wrap("Tab".into(), Theme::mono(t.text_md), t.fg_text);
    let cap = Rect::from_center_size(c + vec2(0.0, 44.0), galley.size() + vec2(20.0, 10.0));
    painter.rect_filled(cap, CornerRadius::same(t.radius_sm), t.bg_control);
    painter.hline(cap.x_range(), cap.bottom() - 0.5, Stroke::new(1.0, t.bg_field));
    painter.galley(cap.center() - galley.size() / 2.0, galley, t.fg_text);
    // the motto (not translated: it's the name's companion)
    painter.text(cap.center() + vec2(0.0, 48.0), Align2::CENTER_CENTER, MOTTO, t.small(), t.fg_muted.gamma_multiply(0.7));
}

/// Velo's motto (README.md, docs/product.md §1).
pub const MOTTO: &str = "Sound shapes light.";

/// `VELO_DEMO_GRAPH`: fills the active scene, if empty, with a small believable patch (1) or a
/// stress graph of about `n` nodes in rows of ten (n > 1). Built with the operators, like a
/// user would; returns a node to select.
pub fn demo(session: &mut Session, n: usize) -> Option<String> {
    if session.active().is_none() {
        session.run("scene.add", json!({})).ok()?;
    }
    if !session.active()?.nodes.is_empty() {
        return None;
    }
    let (dx, dy) = (NODE_W + SPACING, NODE_H + 40.0);
    let mut ids = Vec::new();
    let mut add = |s: &mut Session, kind: &str, x: f32, y: f32| {
        let _ = s.run("node.add", json!({ "kind": kind, "pos": [x * dx, y * dy] }));
        ids.push(s.active().and_then(|c| c.nodes.last()).map(|n| n.id.clone()).unwrap_or_default());
    };
    let mut wires = Vec::new();
    if n <= 1 {
        let plan = [
            ("velo.noise", 0.0, 0.0),
            ("velo.kaleido", 1.0, 0.0),
            ("velo.color", 2.0, 0.0),
            ("velo.gradient", 0.0, 1.0),
            ("velo.shapes", 0.0, 2.0),
            ("velo.transform", 1.0, 2.0),
            ("velo.displace", 2.0, 1.5),
            ("velo.blend", 3.0, 0.75),
            ("velo.feedback", 4.0, 0.75),
            ("velo.output", 5.0, 0.75),
        ];
        for (kind, x, y) in plan {
            add(session, kind, x, y);
        }
        wires = vec![(0, 1, 0), (1, 2, 0), (2, 7, 0), (3, 6, 0), (4, 5, 0), (5, 6, 1), (6, 7, 1), (7, 8, 0), (8, 9, 0)];
    } else {
        // rows of ten: a generator, effects and blends, a layer stack or an output; extra
        // inputs come from the row above, so the graph stays a DAG with cross-links
        const ROW: [&str; 10] = ["", "velo.transform", "velo.blend", "velo.kaleido", "velo.displace", "velo.color", "velo.blend", "velo.blur", "velo.feedback", ""];
        const GENERATORS: [&str; 5] = ["velo.noise", "velo.gradient", "velo.shapes", "velo.shader", "velo.image"];
        for r in 0..n.div_ceil(10) {
            for (c, effect) in ROW.into_iter().enumerate() {
                let kind = match c {
                    0 => GENERATORS[r % 5],
                    9 if r % 2 == 0 => "velo.layers",
                    9 => "velo.output",
                    _ => effect,
                };
                add(session, kind, c as f32, r as f32);
                let i = r * 10 + c;
                if c > 0 {
                    wires.push((i - 1, i, 0));
                }
                if r > 0 {
                    match c {
                        2 | 4 | 6 => wires.push((i - 11, i, 1)),
                        9 if r % 2 == 0 => wires.extend([(i - 11, i, 1), (i - 13, i, 2), (i - 15, i, 3)]),
                        _ => {}
                    }
                }
            }
        }
    }
    for (a, b, port) in wires {
        let _ = session.run("edge.connect", json!({ "from": [ids[a], 0], "to": [ids[b], port] }));
    }
    ids.get(2).cloned()
}

/// `VELO_DEMO_SHADER=1|broken`: a scene of its own, "Shader": a Shader node on the template
/// (or with a typo on line 19) through Kaleido to the Output. Returns the Shader node.
pub fn demo_shader(session: &mut Session, broken: bool) -> Option<String> {
    session.run("scene.add", json!({ "name": "Shader" })).ok()?;
    let code = if broken { velo_engine::TEMPLATE.replace("g.time * speed", "g.tim * speed") } else { velo_engine::TEMPLATE.to_string() };
    let (dx, dy) = (NODE_W + SPACING, NODE_H + 40.0);
    let mut ids = Vec::new();
    for (i, kind) in [velo_core::SHADER, "velo.kaleido", "velo.output"].into_iter().enumerate() {
        let code = (i == 0).then_some(code.as_str());
        session.run("node.add", json!({ "kind": kind, "pos": [i as f32 * dx, dy * 0.5], "code": code })).ok()?;
        ids.push(session.active()?.nodes.last()?.id.clone());
    }
    for pair in ids.windows(2) {
        session.run("edge.connect", json!({ "from": [pair[0], 0], "to": [pair[1], 0] })).ok()?;
    }
    ids.into_iter().next()
}

/// `VELO_DEMO_SCENES`: a second scene, "Drop" (Shapes through Kaleido and Feedback, its zoom on
/// the kick), launched on 4 bars with an 8-beat crossfade. Built with the operators; returns its id.
pub fn demo_drop(session: &mut Session) -> Option<String> {
    session.run("scene.add", json!({ "name": "Drop" })).ok()?;
    // VELO_DEMO_TRANSITION=Wipe|Zoom|Glitch|Flash: launched with that instead (screenshots)
    let transition = std::env::var("VELO_DEMO_TRANSITION").unwrap_or_else(|_| "Crossfade".into());
    session.run("scene.set_launch", json!({ "quantize": "Bars4", "duration": 8, "transition": transition })).ok()?;
    let (dx, dy) = (NODE_W + SPACING, NODE_H + 40.0);
    let mut ids = Vec::new();
    for (i, kind) in ["velo.shapes", "velo.kaleido", "velo.feedback", "velo.color", "velo.output"].into_iter().enumerate() {
        session.run("node.add", json!({ "kind": kind, "pos": [i as f32 * dx, (i % 2) as f32 * dy * 0.5] })).ok()?;
        ids.push(session.active()?.nodes.last()?.id.clone());
    }
    for pair in ids.windows(2) {
        session.run("edge.connect", json!({ "from": [pair[0], 0], "to": [pair[1], 0] })).ok()?;
    }
    session.run("node.set_param", json!({ "node": ids[1], "param": "segments", "value": 8 })).ok()?;
    session.run("node.set_param", json!({ "node": ids[3], "param": "hue", "value": 0.35 })).ok()?;
    session.run("mapping.add", json!({ "node": ids[1], "param": "zoom", "source": "kick" })).ok()?;
    session.active_scene.clone()
}

/// `VELO_DEMO_MACROS`, on the demo patch: four named macros driving Kaleido, Color and Feedback,
/// three variations, a few controller bindings. Built with the operators; returns Kaleido's id.
pub fn demo_macros(session: &mut Session) -> Option<String> {
    let scene = session.active()?;
    let id = |kind: &str| scene.nodes.iter().find(|n| n.kind == kind).map(|n| n.id.clone());
    let (kaleido, color, feedback, noise) = (id("velo.kaleido")?, id("velo.color")?, id("velo.feedback")?, id("velo.noise")?);
    let targets = [(0, "Zoom", &kaleido, "zoom", 0.8, 2.6), (0, "Zoom", &noise, "scale", 2.0, 8.0), (1, "Hue", &color, "hue", -0.4, 0.4), (2, "Spin", &kaleido, "spin", -0.3, 0.3), (3, "Trails", &feedback, "decay", 0.8, 0.98)];
    for (index, name, node, param, min, max) in targets {
        session.run("macro.rename", json!({ "index": index, "name": name })).ok()?;
        session.run("macro.assign", json!({ "index": index, "node": node, "param": param, "min": min, "max": max })).ok()?;
    }
    for values in [[0.2, 0.5, 0.5, 0.3], [0.7, 0.2, 0.6, 0.8], [0.45, 0.85, 0.35, 0.6]] {
        for (index, value) in values.into_iter().enumerate() {
            session.run("macro.set", json!({ "index": index, "value": value })).ok()?;
        }
        session.run("variation.store", json!({})).ok()?;
    }
    session.run("variation.recall", json!({ "index": 1 })).ok()?;
    let midi = |msg: Value| json!({ "Midi": { "port": "Launch Control XL", "channel": 0, "msg": msg } });
    for (msg, target, behavior) in [
        (json!({ "Cc": 21 }), json!({ "Macro": 0 }), json!({ "Absolute": { "takeover": "Pickup" } })),
        (json!({ "Cc": 22 }), json!({ "Macro": 1 }), json!({ "Absolute": { "takeover": "Pickup" } })),
        (json!({ "Note": 36 }), json!({ "LaunchScene": 0 }), json!("Trigger")),
        (json!({ "Note": 41 }), json!({ "Variation": 0 }), json!("Trigger")),
        (json!({ "Note": 44 }), json!("Blackout"), json!("Toggle")),
    ] {
        session.run("binding.add", json!({ "control": midi(msg), "target": target, "behavior": behavior })).ok()?;
    }
    Some(kaleido)
}

/// What a built-in node does, in a line, for the node menu (none for a kind it doesn't know).
/// A node kind's icon in the node menu.
fn node_icon(id: &str) -> &'static str {
    use velo_core as v;
    match id {
        "velo.noise" => icons::CLOUD,
        "velo.gradient" => icons::GRADIENT,
        "velo.shapes" => icons::SHAPES,
        "velo.waveform" => icons::WAVEFORM,
        v::SHADER | v::SHADER_FX => icons::CODE,
        v::IMAGE => icons::IMAGE,
        v::VIDEO => icons::FILM_STRIP,
        v::MEDIA_LIST => icons::IMAGES,
        v::TEXT => icons::TEXT_T,
        v::TEXT_LIST => icons::LIST_BULLETS,
        v::PARTICLES => icons::CONFETTI,
        "velo.transform" => icons::ARROWS_OUT_CARDINAL,
        "velo.kaleido" => icons::FLOWER,
        "velo.color" => icons::PALETTE,
        "velo.blur" => icons::DROP,
        v::FEEDBACK => icons::ARROWS_CLOCKWISE,
        "velo.displace" => icons::WAVES,
        "velo.opacity" => icons::CIRCLE_HALF,
        "velo.levels" => icons::CHART_BAR,
        "velo.threshold" => icons::CIRCLE_HALF_TILT,
        "velo.edges" => icons::POLYGON,
        "velo.channels" => icons::SWATCHES,
        "velo.crop" => icons::CROP,
        "velo.polar" => icons::TARGET,
        "velo.uvmap" => icons::MAP_TRIFOLD,
        "velo.lookup" => icons::EYEDROPPER,
        "velo.freeze" => icons::SNOWFLAKE,
        "velo.timemachine" => icons::HOURGLASS,
        "velo.key" => icons::SCISSORS,
        "velo.lumablur" => icons::DROP_HALF,
        v::BLEND => icons::CIRCLES_THREE,
        "velo.mask" => icons::SELECTION_INVERSE,
        "velo.merge" => icons::GIT_MERGE,
        v::SWITCH => icons::TOGGLE_RIGHT,
        v::GRID => icons::GRID_FOUR,
        "velo.layers" => icons::STACK,
        v::OUTPUT => icons::PROJECTOR_SCREEN,
        v::AUDIO => icons::MICROPHONE,
        v::CLOCK => icons::METRONOME,
        v::LFO => icons::WAVE_SINE,
        v::TIME => icons::CLOCK,
        v::RANDOM => icons::DICE_FIVE,
        v::PROBE => icons::CROSSHAIR,
        v::REMAP => icons::ARROWS_DOWN_UP,
        v::MATH => icons::MATH_OPERATIONS,
        v::ENVELOPE => icons::WAVE_TRIANGLE,
        v::COUNTER => icons::HASH,
        v::SEQUENCER => icons::DOTS_NINE,
        v::HOLD => icons::PUSH_PIN,
        v::TOGGLE => icons::TOGGLE_LEFT,
        v::COMPARE => icons::SCALES,
        v::TIMER => icons::TIMER,
        v::SMOOTH => icons::CHART_LINE_UP,
        v::QUANTIZE => icons::STAIRS,
        v::SPEED => icons::SPEEDOMETER,
        v::DELAY => icons::CLOCK_COUNTER_CLOCKWISE,
        v::SPRING => icons::SPIRAL,
        v::SLOPE => icons::TREND_UP,
        _ => icons::SPARKLE,
    }
}

/// A node menu tab's icon: its group's (None is All).
fn group_icon(group: Option<&str>) -> &'static str {
    match group {
        None => icons::SQUARES_FOUR,
        Some("Generators") => icons::SPARKLE,
        Some("Effects") => icons::MAGIC_WAND,
        Some("Composite") => icons::INTERSECT,
        Some("Control") => icons::ARROWS_SPLIT,
        Some("Output") => icons::MONITOR,
        Some("Signals") => icons::BROADCAST,
        _ => icons::FUNCTION,
    }
}

fn node_blurb(id: &str) -> Option<&'static str> {
    Some(match id {
        "velo.noise" => tr!("Cloudy noise that drifts: textures, smoke, fog."),
        "velo.gradient" => tr!("Two colors blended across the picture."),
        "velo.shapes" => tr!("Rings, polygons or stars, repeated and turning."),
        "velo.waveform" => tr!("The sound's wave or spectrum, drawn as a line."),
        velo_core::SHADER => tr!("A picture in WGSL: one of its presets (Plasma, Fire, Lasers…) or your own code, its knobs from its header."),
        velo_core::IMAGE => tr!("A picture, a GIF or a folder of frames from a file."),
        velo_core::VIDEO => tr!("A movie from a file, looping."),
        velo_core::MEDIA_LIST => tr!("Up to eight pictures and movies in turn: in order, at random or back and forth, on the beat."),
        velo_core::TEXT => tr!("Words set in a font, colored, sized and placed."),
        velo_core::TEXT_LIST => tr!("Lines of words one at a time: in order, at random or back and forth, on the beat."),
        velo_core::PARTICLES => tr!("Thousands of glowing points or pictures of your own, thrown, pulled and stirred: map Kick onto Emission for bursts."),
        "velo.transform" => tr!("Moves, scales and turns what comes in."),
        "velo.kaleido" => tr!("Mirrors what comes in into a kaleidoscope."),
        "velo.color" => tr!("Brightness, contrast, hue, tint and more."),
        "velo.blur" => tr!("Softens what comes in."),
        velo_core::FEEDBACK => tr!("Mixes in its own last frame: trails and echoes."),
        "velo.displace" => tr!("Pushes the picture around by a second one, its map."),
        "velo.opacity" => tr!("How much of what comes in shows: fades it out, over black or what's under it."),
        "velo.levels" => tr!("Black and white points, gamma and an S-curve, on every channel or one."),
        "velo.threshold" => tr!("Two colors, split where the picture crosses a level: masks and stencils."),
        "velo.edges" => tr!("Lines where the picture changes, alone or over it."),
        "velo.channels" => tr!("Swaps, copies or splits the red, green, blue and alpha."),
        "velo.crop" => tr!("Cuts the edges away: in place, stretched to fill or fitted."),
        "velo.polar" => tr!("Wraps the picture round its middle, or unrolls a round one."),
        "velo.uvmap" => tr!("Shows the picture where a second one, its map, says: any warp."),
        "velo.lookup" => tr!("Recolors the picture by its brightness from a second one, its palette."),
        "velo.freeze" => tr!("Holds the picture on a hit, or takes a frame every few beats."),
        "velo.timemachine" => tr!("The picture as it was a few frames ago; with a map, each part as far back as the map is bright."),
        "velo.key" => tr!("Cuts a color, the dark or the bright out of the picture, to see through it."),
        "velo.lumablur" => tr!("Blurs the picture as much as a second one, its map, is bright there."),
        velo_core::SHADER_FX => tr!("An effect in WGSL on what comes in: one of its presets (Glitch, VHS, LED Wall…) or your own code."),
        velo_core::BLEND => tr!("Up to sixteen pictures in one: a blend mode, its intensity, a key for dark backgrounds."),
        "velo.layers" => tr!("Up to eight pictures, each with its mode, opacity and order."),
        "velo.mask" => tr!("Shows the picture where the mask is bright."),
        "velo.merge" => tr!("Up to four pictures as one's red, green, blue and alpha."),
        velo_core::GRID => tr!("Up to eight pictures in a grid of cells."),
        velo_core::SWITCH => tr!("One of up to sixteen pictures at a time, picked by a value: cut, crossfaded, dissolved or wiped. Only the one it shows renders."),
        velo_core::OUTPUT => tr!("Where the scene's picture leaves: the preview, a display."),
        velo_core::AUDIO => tr!("The music as signals: its level, bands, hits and melody."),
        velo_core::CLOCK => tr!("The tempo as signals: a hit on every beat, a ramp through the bar."),
        velo_core::LFO => tr!("A shape that repeats over a number of beats."),
        velo_core::TIME => tr!("A ramp or a wave over a number of seconds."),
        velo_core::REMAP => tr!("Brings a signal into the range you want, smoothed."),
        velo_core::MATH => tr!("Two signals made one: added, multiplied, the larger…"),
        velo_core::ENVELOPE => tr!("A rise and a fall on each hit."),
        velo_core::COUNTER => tr!("Steps on each hit, up, down or at random."),
        velo_core::SEQUENCER => tr!("A value a step on each beat."),
        velo_core::HOLD => tr!("On each hit, holds what comes in, or a random value."),
        velo_core::TOGGLE => tr!("Flips between 0 and 1 on each hit."),
        velo_core::COMPARE => tr!("1 while what comes in is above (or below) a level."),
        velo_core::TIMER => tr!("A hit every so many seconds, again and again."),
        velo_core::RANDOM => tr!("A new random value every few beats or on each hit, gliding or not."),
        velo_core::SMOOTH => tr!("Follows what comes in slowly: a glide or a lag, up and down apart."),
        velo_core::QUANTIZE => tr!("Snaps what comes in to a few even steps."),
        velo_core::SPEED => tr!("Goes round as fast as what comes in says: motion that speeds up and slows down, never jumps."),
        velo_core::DELAY => tr!("What comes in, played back a few beats later."),
        velo_core::SPRING => tr!("Follows what comes in on a spring: it overshoots and swings back."),
        velo_core::SLOPE => tr!("How fast what comes in rises or falls: 1 for a sudden jump."),
        velo_core::PROBE => tr!("A picture read as a number: its brightness or a channel, over all of it, a part or a point."),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, RawInput};

    #[test]
    fn arranging_reads_left_to_right_without_overlaps() {
        let mut session = Session::default();
        // Pulse Rings: Shapes into Feedback into Output, and the kick and the snare moving them
        let rings = crate::templates::TEMPLATES.iter().find(|t| t.name == "Pulse Rings").unwrap();
        crate::templates::add(&mut session, rings, None).unwrap();
        let scene = session.active().unwrap().clone();
        let mut moved = scene.clone();
        for (id, to) in arrange(&scene) {
            moved.nodes.iter_mut().find(|n| n.id == id).unwrap().pos = to;
        }
        let x = |kind: &str| moved.nodes.iter().find(|n| n.kind == kind).map(|n| plane_rect(&moved, n, n.pos)).unwrap();
        assert!(x(velo_core::AUDIO).right() < x("velo.shapes").left(), "the signal before what it moves");
        assert!(x("velo.shapes").right() < x("velo.feedback").left() && x("velo.feedback").right() < x("velo.output").left(), "wired left to right");
        for (i, a) in moved.nodes.iter().enumerate() {
            for b in &moved.nodes[i + 1..] {
                assert!(!plane_room(&moved, a).intersects(plane_room(&moved, b)), "{} over {}", a.kind, b.kind);
            }
        }
        assert!(arrange(&moved).is_empty(), "arranged twice, nothing moves");
    }

    #[test]
    fn bundles_tie_what_meets_and_only_once() {
        let mut session = Session::default();
        let bloom = crate::templates::TEMPLATES.iter().find(|t| t.name == "Kaleido Bloom").unwrap();
        crate::templates::add(&mut session, bloom, None).unwrap();
        let scene = session.active().unwrap().clone();
        let blend = scene.nodes.iter().find(|n| n.kind == "velo.blend").unwrap();
        let found = bundles(&scene, &|n| n.pos);
        let into_blend: Vec<_> = found.iter().filter(|(_, cables)| cables.iter().all(|c| c.starts_with(&blend.id))).collect();
        assert_eq!(into_blend.len(), 1, "Blend's two inputs, bundled");
        assert!(into_blend[0].0.x < plane_rect(&scene, blend, blend.pos).left(), "just before it");
        for (at, cables) in &found {
            session.run("tie.add", json!({ "pos": [at.x, at.y], "cables": cables })).unwrap();
        }
        assert!(bundles(session.active().unwrap(), &|n| n.pos).is_empty(), "tied once");
    }

    #[test]
    fn the_camera_round_trips_and_zooms_about_the_pointer() {
        let (c, p) = (pos2(500.0, 400.0), pos2(640.0, 300.0));
        let cam = Camera { pan: vec2(30.0, -12.0), zoom: 0.8 };
        let g = cam.graph(c, p);
        assert!(cam.screen(c, g).distance(p) < 1e-3);
        let zoomed = cam.zoomed(c, p, 1.2);
        assert!((zoomed.zoom - 1.2).abs() < 1e-5);
        assert!(zoomed.screen(c, g).distance(p) < 1e-3, "the point under the pointer stays put");
        assert_eq!((step(1.0, true), step(1.0, false), step(0.7, true), step(8.0, true)), (2.0, 0.5, 1.0, MAX_ZOOM));
    }

    #[test]
    fn you_zoom_out_only_until_every_node_shows() {
        let view = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0));
        let mut s = Session::default();
        assert_eq!(min_zoom(&Scene::default(), view), ZOOM_FLOOR, "an empty scene stops at the floor");
        demo(&mut s, 1);
        assert_eq!(min_zoom(s.active().unwrap(), view), ZOOM_FLOOR, "a small patch fits well before the floor");
        let mut s = Session::default();
        demo(&mut s, 200);
        let scene = s.active().unwrap();
        let min = min_zoom(scene, view);
        assert!(min < ZOOM_FLOOR && min > MIN_ZOOM, "{min}");
        // at that zoom, the view that fits everything is on screen
        let cam = fit(bounds(scene, None), view.center(), view, 1.0);
        assert!((cam.zoom - min).abs() < 1e-4);
        let b = bounds(scene, None).unwrap();
        assert!(view.expand(1.0).contains_rect(Rect::from_two_pos(cam.screen(view.center(), b.min), cam.screen(view.center(), b.max))));
    }

    #[test]
    fn wires_are_found_near_their_curve() {
        let pts = bezier(pos2(0.0, 0.0), pos2(200.0, 100.0), 1.0);
        assert_eq!((pts[0], *pts.last().unwrap()), (pos2(0.0, 0.0), pos2(200.0, 100.0)));
        assert!(near_polyline(pos2(100.0, 50.0), &pts, WIRE_REACH), "the middle of an S-curve is its center");
        assert!(!near_polyline(pos2(100.0, 0.0), &pts, WIRE_REACH));
        // the light that runs along a wire: by length, not by point count
        let bend = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0)];
        assert_eq!(along(&bend, 0.75), pos2(10.0, 5.0));
        assert_eq!((along(&bend, -1.0), along(&bend, 2.0)), (pos2(0.0, 0.0), pos2(10.0, 10.0)));
    }

    #[test]
    fn demo_graphs_build_through_the_operators() {
        let mut s = Session::default();
        demo(&mut s, 1).unwrap();
        let scene = s.active().unwrap();
        assert_eq!((scene.nodes.len(), scene.edges.len()), (10, 9), "every planned wire is legal");
        demo_macros(&mut s).expect("the macros demo builds on it");
        assert_eq!((s.active().unwrap().variations.len(), s.project.bindings.len()), (3, 5));
        let mut s = Session::default();
        demo(&mut s, 200);
        let scene = s.active().unwrap();
        assert_eq!(scene.nodes.len(), 200);
        assert!(scene.edges.len() >= 250, "{} wires", scene.edges.len());
        assert!(demo(&mut s, 1).is_none(), "a scene with nodes is left alone");
    }

    // ---- the real UI, headless: synthetic pointer and keys through App::draw ----

    const SCREEN: Vec2 = vec2(1280.0, 800.0);
    const NONE: Modifiers = Modifiers::NONE;
    #[test]
    fn several_files_dropped_at_once_are_a_media_list() {
        let mut app = App::for_tests();
        app.run("scene.add", json!({}));
        app.run("node.add", json!({ "kind": velo_core::OUTPUT, "pos": [0.0, 0.0] }));
        let files: Vec<std::path::PathBuf> = ["a.png", "b.png", "c.mp4"].iter().map(|f| std::env::temp_dir().join(f)).collect();
        app.import_media_set(&files);
        let s = app.session.active().unwrap();
        let list = s.nodes.iter().find(|n| n.kind == velo_core::MEDIA_LIST).unwrap();
        let names: Vec<String> = velo_core::media_list(list).iter().map(|p| std::path::Path::new(p).file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["a.png", "b.png", "c.mp4"], "the files in the order dropped");
        let out = s.nodes.iter().find(|n| n.kind == velo_core::OUTPUT).unwrap();
        assert_eq!(s.wire_into(&out.id, 0).map(|e| e.from.0.as_str()), Some(list.id.as_str()));
        // one step to undo
        app.run("edit.undo", json!({}));
        assert_eq!(app.session.active().unwrap().nodes.len(), 1);
    }

    /// What a Cmd (macOS) or Ctrl chord carries from egui-winit.
    const CMD: Modifiers = if cfg!(target_os = "macos") { Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE } } else { Modifiers { ctrl: true, command: true, ..Modifiers::NONE } };

    struct Harness {
        ctx: egui::Context,
        app: App,
        time: f64,
        /// Held down, as the window reports them with every frame.
        modifiers: Modifiers,
        /// The last frame's accessibility tree, once `ctx.enable_accesskit()` is on: where
        /// widgets are, by their names.
        access: Option<egui::accesskit::TreeUpdate>,
    }

    impl Harness {
        /// A scene with Noise, Blur and Output in a row, unwired, seen at 100 % with the plane's
        /// origin at the canvas' center.
        fn new() -> Self {
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            let mut app = App::for_tests();
            app.theme.apply(&ctx);
            app.run("scene.add", json!({}));
            for (kind, x) in [("velo.noise", -340.0), ("velo.blur", -100.0), ("velo.output", 140.0)] {
                app.run("node.add", json!({ "kind": kind, "pos": [x, -100.0] }));
            }
            let mut h = Self { ctx, app, time: 0.0, modifiers: NONE, access: None };
            h.frame(Vec::new());
            h.frame(Vec::new());
            let scene = h.scene().id.clone();
            h.app.canvas.cameras.insert(scene, Camera::default());
            h.frame(Vec::new());
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let events = std::iter::once(Event::ModifiersChanged(self.modifiers)).chain(events).collect();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.time), events, ..Default::default() };
            let (ctx, app) = (&self.ctx, &mut self.app);
            // nothing renders it: the font atlas' uploads go nowhere
            let mut out = ctx.run_ui(input, |ui| app.draw(ui));
            self.access = out.platform_output.accesskit_update.take();
            out.drop_without_applying_deltas();
        }

        /// The window's close button: the commands the app sent back to the window.
        fn close_window(&mut self) -> Vec<egui::ViewportCommand> {
            self.time += 1.0 / 60.0;
            let mut input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.time), ..Default::default() };
            input.viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
            let (ctx, app) = (&self.ctx, &mut self.app);
            let mut out = ctx.run_ui(input, |ui| app.draw(ui));
            let commands = out.viewport_output.remove(&egui::ViewportId::ROOT).map(|v| v.commands).unwrap_or_default();
            out.drop_without_applying_deltas();
            commands
        }

        /// Where the widget named `label` was last frame.
        fn widget(&self, label: &str) -> Option<Rect> {
            let (_, node) = self.access.as_ref()?.nodes.iter().find(|(_, n)| n.label() == Some(label))?;
            let b = node.bounds()?;
            Some(Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32)))
        }

        fn key_at(&mut self, key: Key, physical: Key, modifiers: Modifiers) {
            self.modifiers = modifiers;
            for pressed in [true, false] {
                self.frame(vec![Event::Key { key, physical_key: Some(physical), pressed, repeat: false, modifiers }]);
            }
            self.modifiers = NONE;
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.key_at(key, key, modifiers);
        }

        fn text(&mut self, text: &str) {
            self.frame(vec![Event::Text(text.into())]);
        }

        fn move_to(&mut self, p: Pos2) {
            self.frame(vec![Event::PointerMoved(p)]);
        }

        fn button(&mut self, p: Pos2, pressed: bool, modifiers: Modifiers) {
            self.modifiers = modifiers;
            self.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers }]);
            self.modifiers = NONE;
        }

        fn click_with(&mut self, p: Pos2, modifiers: Modifiers) {
            self.move_to(p);
            self.button(p, true, modifiers);
            self.button(p, false, modifiers);
        }

        fn click(&mut self, p: Pos2) {
            self.click_with(p, NONE);
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.move_to(from);
            self.button(from, true, NONE);
            for k in 1..=8 {
                self.move_to(from + (to - from) * k as f32 / 8.0);
            }
            self.button(to, false, NONE);
            self.frame(Vec::new());
        }

        fn scene(&self) -> &Scene {
            self.app.session.active().unwrap()
        }

        fn id(&self, i: usize) -> String {
            self.scene().nodes[i].id.clone()
        }

        fn cam(&self) -> Camera {
            self.app.canvas.cameras[&self.scene().id]
        }

        /// The canvas: the window under the top bar and over the status bar.
        fn center() -> Pos2 {
            Rect::from_min_max(pos2(0.0, Theme::dark().bar_h), pos2(SCREEN.x, SCREEN.y - 24.0)).center()
        }

        fn node_at(&self, i: usize) -> Rect {
            node_rect(&self.cam(), Self::center(), self.scene().nodes[i].pos)
        }

        fn port_at(&self, i: usize, port: u8, output: bool) -> Pos2 {
            let n = &self.scene().nodes[i];
            let count = if output { node_kind(&n.kind).unwrap().outputs.len() } else { velo_core::inputs_shown(self.scene(), n) };
            port_pos(self.node_at(i), self.cam().zoom, count, port, output)
        }

        fn wires(&self) -> Vec<(String, String, u8)> {
            self.scene().edges.iter().map(|e| (e.from.0.clone(), e.to.0.clone(), e.to.1)).collect()
        }
    }

    #[test]
    fn a_signal_dragged_onto_a_card_opens_a_jack_and_plugs_in() {
        let mut h = Harness::new();
        h.app.run("node.add", json!({ "kind": velo_core::LFO, "pos": [-340.0, 120.0] }));
        h.frame(Vec::new());
        let (lfo, noise) = (h.scene().nodes.len() - 1, h.id(0));
        let (from, to) = (h.port_at(lfo, 0, true), h.node_at(0).center());
        h.move_to(from);
        h.button(from, true, NONE);
        for k in 1..=10 {
            h.move_to(from + (to - from) * k as f32 / 10.0);
        }
        for _ in 0..30 {
            h.move_to(to);
        }
        assert!(h.app.canvas.slots.get(&noise).is_some_and(|open| *open > 0.9), "a slot slid out of the card's foot");
        h.button(to, false, NONE);
        h.frame(Vec::new());
        assert_eq!(h.app.canvas.pick.as_ref().map(|p| p.1.as_str()), Some(noise.as_str()), "let go on it, its values to pick from");
        h.key(Key::Enter, NONE);
        h.frame(Vec::new());
        let moved = &h.scene().nodes[0].mappings;
        assert_eq!(moved.len(), 1, "the first value is moved by the LFO");
        assert_eq!(jacks(&h.scene().nodes[0]).len(), 1, "a jack for it");
        assert!(h.app.canvas.slots.get(&noise).is_none_or(|open| *open == 0.0), "the jack took the slot's place");
    }

    #[test]
    fn a_signals_cable_pulled_off_onto_nothing_unplugs() {
        let mut h = Harness::new();
        h.app.run("node.add", json!({ "kind": velo_core::LFO, "pos": [-340.0, 160.0] }));
        let (lfo, blur) = (h.scene().nodes.len() - 1, h.id(1));
        h.app.run("mapping.add", json!({ "node": blur, "param": "radius", "source": format!("{}/0", h.id(lfo)) }));
        h.frame(Vec::new());
        assert_eq!(h.scene().nodes[1].mappings.len(), 1);
        // the cable's middle (its curve's, between the LFO's output and the Blur's jack)
        let socket = jack_socket(Rect::from_min_size(h.node_at(1).left_bottom(), vec2(NODE_W, 0.0)), 1.0, 0);
        let mid = h.port_at(lfo, 0, true).lerp(socket, 0.5);
        h.drag(mid, mid + vec2(0.0, 220.0));
        assert!(h.scene().nodes[1].mappings.is_empty(), "let go of nowhere, it's gone");
        h.app.run("edit.undo", json!({}));
        assert_eq!(h.scene().nodes[1].mappings.len(), 1, "one step brings it back");
    }

    #[test]
    fn a_split_canvas_keeps_the_inspector_beside_the_nodes_or_floats_it() {
        let mut h = Harness::new();
        h.app.canvas.selected = vec![h.id(0)];
        h.ctx.enable_accesskit();
        h.app.command("app.split");
        for _ in 0..3 {
            h.frame(Vec::new());
        }
        let (center, nodes) = (h.app.center_rect.unwrap(), h.app.canvas.view.unwrap());
        let scale = h.widget("Scale").expect("the inspector, docked by the nodes");
        assert!(nodes.right() < scale.left() && scale.right() < center.right() - center.width() * 0.3, "nodes | inspector | picture: {nodes:?} {scale:?} {center:?}");
        // floating: its own window, the nodes take the inspector's room back
        h.app.command("app.float_inspector");
        for _ in 0..3 {
            h.frame(Vec::new());
        }
        assert!(h.widget("Scale").is_some() && h.app.canvas.view.unwrap().width() > nodes.width());
        // dragged by its bar (beside its title), it goes where it's dragged
        let (window, before) = (h.widget("inspector").expect("its window"), h.widget("Scale").unwrap());
        let grab = window.left_top() + vec2(120.0, 16.0);
        h.drag(grab, grab + vec2(-120.0, -60.0));
        h.frame(Vec::new());
        let after = h.widget("Scale").unwrap();
        assert!((after.min - before.min - vec2(-120.0, -60.0)).length() < 2.0, "{before:?} → {after:?}");
        h.app.command("app.split");
        h.app.command("app.split");
        assert!(h.app.settings.split == crate::Split::Off, "right, bottom, off");
    }

    #[test]
    fn a_card_lists_its_outputs_but_the_hidden_unless_in_use() {
        let mut s = Session::default();
        s.run("scene.add", json!({})).unwrap();
        s.run("node.add", json!({ "kind": "velo.kaleido" })).unwrap();
        let kaleido = s.active().unwrap().nodes[0].id.clone();
        // the kick, mapped: an Audio In added for it
        s.run("mapping.add", json!({ "node": kaleido, "param": "zoom", "source": "kick" })).unwrap();
        let audio = s.active().unwrap().nodes[1].id.clone();
        assert_eq!(rows(s.active().unwrap(), &s.active().unwrap().nodes[1]), Some(vec![0, 1, 2, 3, 4]), "at first level, bass, mid and high, and the kick in use");
        s.run("node.set_param", json!({ "node": audio, "param": "hidden", "value": 0b1111_1110 })).unwrap();
        assert_eq!(rows(s.active().unwrap(), &s.active().unwrap().nodes[1]), Some(vec![0, 4]), "level, and the kick in use");
        assert_eq!(rows(s.active().unwrap(), &s.active().unwrap().nodes[0]), None, "a picture node has a viewer");
    }

    #[test]
    fn a_double_click_on_a_slider_types_a_value() {
        for dropdown in [false, true] {
            double_click_types(dropdown);
        }
    }

    /// In the inspector, or in the node's parameters dropped down on the canvas.
    fn double_click_types(dropdown: bool) {
        let mut h = Harness::new();
        h.app.canvas.selected = vec![h.id(0)];
        if dropdown {
            h.app.settings.panels.inspector = false;
            h.app.canvas.props = Some(h.id(0));
        }
        h.ctx.enable_accesskit();
        h.frame(Vec::new());
        h.frame(Vec::new());
        let scale = h.widget("Scale").expect("the noise's Scale slider");
        h.move_to(scale.center());
        for _ in 0..2 {
            h.button(scale.center(), true, NONE);
            h.button(scale.center(), false, NONE);
        }
        h.frame(Vec::new());
        assert!(h.ctx.text_edit_focused(), "the value's field has the keyboard (dropdown: {dropdown})");
        // the caret at the end: what's typed goes after the shown value, erased first
        for _ in 0..8 {
            h.key(Key::Backspace, NONE);
        }
        h.text("7.5");
        h.key(Key::Enter, NONE);
        assert_eq!(h.scene().nodes[0].params.get("scale").map(|v| v[0]), Some(7.5), "dropdown: {dropdown}");
        assert!(!h.ctx.text_edit_focused());
    }

    #[test]
    fn a_source_dropped_on_a_slider_maps_it_and_alt_drag_sets_its_amount() {
        let mut h = Harness::new();
        h.app.canvas.selected = vec![h.id(0)];
        h.ctx.enable_accesskit();
        h.frame(Vec::new());
        let scale = h.widget("Scale").expect("the noise's Scale slider in the inspector");
        let maps = |h: &Harness| h.scene().nodes[0].mappings.iter().map(|m| (m.param.clone(), m.source.clone(), m.amount)).collect::<Vec<_>>();
        // a Kick chip released over the slider
        for _ in 0..2 {
            egui::DragAndDrop::set_payload(&h.ctx, crate::widgets::SourceDrag("kick".into(), "Kick".into()));
            h.move_to(scale.center());
            h.button(scale.center(), false, NONE);
        }
        // from the scene's Audio In, added for it
        let audio = h.scene().nodes.iter().find(|n| n.kind == velo_core::AUDIO).map(|n| n.id.clone()).expect("an Audio In");
        assert_eq!(maps(&h), [("scale".to_string(), format!("{audio}/4"), 0.5)], "dropped twice, mapped once");
        // Alt-drag right: more amount, one undo step
        h.frame(Vec::new());
        let scale = h.widget("Scale").unwrap();
        let from = pos2(scale.center().x, scale.bottom() - 30.0);
        h.modifiers = Modifiers::ALT;
        h.move_to(from);
        h.button(from, true, Modifiers::ALT);
        h.modifiers = Modifiers::ALT;
        for k in 1..=6 {
            h.move_to(from + vec2(10.0 * k as f32, 0.0));
        }
        h.button(from + vec2(60.0, 0.0), false, Modifiers::ALT);
        h.modifiers = NONE;
        let amount = maps(&h)[0].2;
        assert!(amount > 0.6, "Alt-drag raised the amount: {amount}");
        assert_eq!(h.scene().nodes[0].params.get("scale"), None, "and left the base value alone");
        h.app.run("edit.undo", json!({}));
        assert_eq!(maps(&h)[0].2, 0.5);
        h.app.run("edit.undo", json!({}));
        assert!(maps(&h).is_empty());
    }

    #[test]
    fn a_mapping_chip_opens_its_settings_in_the_inspector_and_on_its_link() {
        for on_link in [false, true] {
            let mut h = Harness::new();
            let noise = h.id(0);
            h.app.run("mapping.add", json!({ "node": noise, "param": "scale", "source": "kick" }));
            let source = h.scene().nodes[0].mappings[0].source.clone();
            if on_link {
                h.app.canvas.map = Some((noise, source.clone()));
            } else {
                h.app.canvas.selected = vec![noise];
            }
            h.ctx.enable_accesskit();
            h.frame(Vec::new());
            h.frame(Vec::new());
            // its settings, wherever they show: an Attack knob each
            let attacks = |h: &Harness| h.access.as_ref().map_or(0, |a| a.nodes.iter().filter(|(_, n)| n.label().is_some_and(|l| l.starts_with("Attack"))).count());
            let before = attacks(&h);
            let chip = h.widget(&format!("{} +50%", crate::mapping_source(h.scene(), &source))).expect("the chip under the slider");
            h.click(chip.center());
            h.frame(Vec::new());
            assert_eq!(attacks(&h), before + 1, "a click on the chip opened its settings (on its link: {on_link})");
        }
    }

    #[test]
    fn wires_are_dragged_lifted_and_moved() {
        let mut h = Harness::new();
        let (noise, blur, out) = (h.id(0), h.id(1), h.id(2));
        h.drag(h.port_at(0, 0, true), h.port_at(1, 0, false));
        assert_eq!(h.wires(), [(noise.clone(), blur.clone(), 0)], "output dragged onto an input");
        assert_eq!(h.app.session.undo_label(), Some("Connect"));
        // dropped on a card, a wire takes its first free input
        h.drag(h.port_at(1, 0, true), h.node_at(2).center());
        assert_eq!(h.wires().len(), 2);
        // a wire into its own node snaps nowhere and changes nothing
        h.drag(h.port_at(1, 0, true), h.port_at(1, 0, false) + vec2(2.0, 0.0));
        assert_eq!(h.wires().len(), 2, "no node feeds itself");
        // pulled off an input and let go on empty canvas, it's unplugged
        h.drag(h.port_at(2, 0, false), h.port_at(2, 0, false) + vec2(0.0, 160.0));
        assert_eq!(h.wires(), [(noise.clone(), blur.clone(), 0)]);
        // pulled off one input onto another, it moves in one undo step
        h.drag(h.port_at(1, 0, false), h.port_at(2, 0, false));
        assert_eq!(h.wires(), [(noise.clone(), out.clone(), 0)]);
        h.app.run("edit.undo", json!({}));
        assert_eq!(h.wires(), [(noise, blur, 0)], "the move undoes at once");
    }

    #[test]
    fn a_wire_is_drawn_back_from_a_free_input_and_snaps_from_near() {
        let mut h = Harness::new();
        let (noise, blur) = (h.id(0), h.id(1));
        // let go on the canvas beside an output it may take: the bubble pulls it in
        h.drag(h.port_at(1, 0, false), h.port_at(0, 0, true) + vec2(20.0, 24.0));
        assert_eq!(h.wires(), [(noise, blur.clone(), 0)], "drawn back from Blur's input into Noise's output");
        // onto its own node's output it goes nowhere
        h.app.run("edge.disconnect", json!({ "to": [blur, 0] }));
        h.drag(h.port_at(1, 0, false), h.port_at(1, 0, true));
        assert!(h.wires().is_empty(), "no node feeds itself");
    }

    #[test]
    fn a_wire_is_grabbed_anywhere_cut_with_alt_and_takes_a_node_dropped_on_it() {
        let mut h = Harness::new();
        let (noise, blur, out) = (h.id(0), h.id(1), h.id(2));
        // the blur out of the way, then the noise straight into the output, past it
        h.drag(h.node_at(1).center(), h.node_at(1).center() + vec2(0.0, 260.0));
        h.drag(h.port_at(0, 0, true), h.port_at(2, 0, false));
        assert_eq!(h.wires(), [(noise.clone(), out.clone(), 0)]);
        // a lone node dropped on the wire goes into it, its move and wires one undo step
        let middle = h.port_at(0, 0, true).lerp(h.port_at(2, 0, false), 0.5);
        h.drag(h.node_at(1).center(), middle);
        assert_eq!(h.wires(), [(noise.clone(), blur.clone(), 0), (blur.clone(), out.clone(), 0)]);
        h.app.run("edit.undo", json!({}));
        assert_eq!(h.wires(), [(noise.clone(), out.clone(), 0)]);
        // grabbed by its middle and let go on empty canvas, the wire comes off
        h.drag(middle, middle - vec2(0.0, 200.0));
        assert!(h.wires().is_empty());
        h.app.run("edit.undo", json!({}));
        // Alt+click takes it away at once
        h.click_with(middle, Modifiers::ALT);
        assert!(h.wires().is_empty(), "Alt+click removes the wire");
    }

    #[test]
    fn closing_with_work_unsaved_asks_first_and_saved_work_just_goes() {
        let mut h = Harness::new();
        // the scene it built is unsaved: the close waits for an answer
        assert!(h.close_window().contains(&egui::ViewportCommand::CancelClose));
        assert!(matches!(h.app.pending, Some(crate::files::Pending::Quit)));
        // Don't Save: it closes, and leaves nothing to restore next time
        h.app.pending = None;
        h.app.saved = h.app.session.project.clone();
        assert!(!h.close_window().contains(&egui::ViewportCommand::CancelClose), "saved work just closes");
        assert!(h.app.pending.is_none());
    }

    #[test]
    fn a_new_wire_dropped_on_empty_canvas_asks_what_to_feed() {
        let mut h = Harness::new();
        let noise = h.id(0);
        h.drag(h.port_at(0, 0, true), h.port_at(0, 0, true) + vec2(60.0, 180.0));
        assert!(h.app.canvas.menu.as_ref().is_some_and(|m| m.from.is_some()), "the node menu opens for the wire");
        h.text("kal");
        h.key(Key::Enter, NONE);
        let kaleido = h.scene().nodes.last().unwrap().clone();
        assert_eq!(kaleido.kind, "velo.kaleido");
        assert_eq!(h.wires(), [(noise, kaleido.id, 0)]);
    }

    #[test]
    fn power_and_e_toggle_nodes_with_undo() {
        let mut h = Harness::new();
        h.app.settings.hide_preview = true;
        for zoom in [0.5, 1.0, 2.0] {
            let scene = h.scene().id.clone();
            h.app.canvas.cameras.insert(scene, Camera { zoom, ..Camera::default() });
            h.frame(Vec::new());
            h.click(power_button(h.node_at(1), zoom).center());
            assert!(h.scene().nodes[1].off, "power at zoom {zoom}");
            assert_eq!(h.app.session.undo_label(), Some("Turn Node On or Off"));
            h.app.run("edit.undo", json!({}));
            assert!(!h.scene().nodes[1].off);
        }
        h.move_to(Harness::center());
        h.key(Key::A, CMD);
        h.key(Key::E, NONE);
        assert_eq!(h.scene().nodes.iter().map(|n| n.off).collect::<Vec<_>>(), [true, true, false]);
        h.key(Key::E, NONE);
        assert!(h.scene().nodes.iter().all(|n| !n.off));
        h.app.run("edit.undo", json!({}));
        assert_eq!(h.scene().nodes.iter().map(|n| n.off).collect::<Vec<_>>(), [true, true, false]);
    }

    #[test]
    fn nodes_select_move_and_go() {
        let mut h = Harness::new();
        let (noise, blur) = (h.id(0), h.id(1));
        // a click selects, Shift+click adds, Esc lets go
        h.click(h.node_at(0).center());
        assert_eq!(h.app.canvas.selected, std::slice::from_ref(&noise));
        h.click_with(h.node_at(1).center(), Modifiers::SHIFT);
        assert_eq!(h.app.canvas.selected, [noise.clone(), blur.clone()]);
        h.key(Key::Escape, NONE);
        assert!(h.app.canvas.selected.is_empty());
        // a drag moves the node by exactly the pointer's travel, as one undo step
        let before = h.scene().nodes[1].pos;
        h.drag(h.node_at(1).center(), h.node_at(1).center() + vec2(100.0, 50.0));
        assert_eq!(h.scene().nodes[1].pos, [before[0] + 100.0, before[1] + 50.0]);
        assert_eq!(h.app.session.undo_label(), Some("Move Node"));
        h.app.run("edit.undo", json!({}));
        assert_eq!(h.scene().nodes[1].pos, before);
        // a rectangle from empty canvas selects what it touches
        let (a, b) = (h.node_at(0).left_top() - vec2(30.0, 30.0), h.node_at(1).center());
        h.drag(a, b);
        assert_eq!(h.app.canvas.selected, [noise, blur]);
        // Delete takes them, wires and all, in one undo step
        h.app.run("edge.connect", json!({ "from": [h.id(0), 0], "to": [h.id(1), 0] }));
        h.key(Key::Delete, NONE);
        assert_eq!(h.scene().nodes.len(), 1);
        assert!(h.wires().is_empty());
        h.app.run("edit.undo", json!({}));
        assert_eq!((h.scene().nodes.len(), h.wires().len()), (3, 1));
        // a click on a wire selects it, Backspace removes it
        let mid = (h.port_at(0, 0, true).to_vec2() + h.port_at(1, 0, false).to_vec2()) / 2.0;
        h.click(mid.to_pos2());
        assert!(h.app.canvas.wire.is_some());
        h.key(Key::Backspace, NONE);
        assert!(h.wires().is_empty());
        // Cmd+A, Cmd+D: every node, doubled
        h.move_to(Harness::center());
        h.key(Key::A, CMD);
        assert_eq!(h.app.canvas.selected.len(), 3);
        h.key(Key::D, CMD);
        assert_eq!(h.scene().nodes.len(), 6);
        assert_eq!(h.app.session.undo_label(), Some("Add Node"), "a duplicate is one step");
    }

    #[test]
    fn tab_appends_and_double_click_renames() {
        let mut h = Harness::new();
        let blur = h.id(1);
        h.click(h.node_at(1).center());
        h.key(Key::Tab, NONE);
        assert!(h.app.canvas.menu.is_some(), "Tab opens the node menu");
        h.text("colo");
        h.key(Key::Enter, NONE);
        assert!(h.app.canvas.menu.is_none());
        let color = h.scene().nodes.last().unwrap().clone();
        assert_eq!(color.kind, "velo.color");
        assert_eq!(h.wires(), [(blur.clone(), color.id.clone(), 0)], "appended after the selected node");
        assert!(color.pos[0] > h.scene().nodes[1].pos[0] + NODE_W, "to its right");
        assert_eq!(h.app.canvas.selected, std::slice::from_ref(&color.id), "the new node is selected: Tab again chains");
        h.app.run("edit.undo", json!({}));
        assert!(h.wires().is_empty() && h.scene().nodes.len() == 3, "node and wire undo together");
        // Esc closes the menu; double-click on empty canvas opens it
        h.key(Key::Tab, NONE);
        h.key(Key::Escape, NONE);
        assert!(h.app.canvas.menu.is_none());
        // under the nodes, over the toolbar and the macros
        let empty = Harness::center() + vec2(0.0, 120.0);
        h.click(empty);
        h.click(empty);
        assert!(h.app.canvas.menu.is_some(), "double-click on empty canvas");
        h.key(Key::Escape, NONE);
        let noise = h.node_at(0).center();
        h.click(noise);
        h.click(noise);
        assert!(h.app.rename.as_ref().is_some_and(|r| r.node.is_some()), "double-click on a node renames it");
    }

    #[test]
    fn the_view_zooms_like_figma() {
        let mut h = Harness::new();
        h.move_to(Harness::center() + vec2(40.0, 30.0));
        h.key(Key::Equals, CMD);
        assert_eq!(h.cam().zoom, 2.0, "Cmd+= goes a Figma step in");
        h.key(Key::Minus, CMD);
        h.key(Key::Minus, CMD);
        h.key(Key::Minus, CMD);
        assert_eq!(h.cam().zoom, ZOOM_FLOOR, "a small graph zooms out to the floor and no further");
        h.key_at(Key::Equals, Key::Num0, Modifiers::SHIFT);
        assert_eq!(h.cam().zoom, 1.0, "Shift+0 (\"=\" on an Italian keyboard) is 100 %");
        h.frame(vec![Event::Zoom(1.5)]);
        h.frame(Vec::new());
        assert!((h.cam().zoom - 1.5).abs() < 1e-3, "a pinch zooms about the pointer: {}", h.cam().zoom);
        h.key_at(Key::Exclamationmark, Key::Num1, Modifiers::SHIFT);
        let fitted = h.cam();
        assert!(fitted.zoom <= 1.0 && fitted.zoom >= ZOOM_FLOOR, "Shift+1 fits everything");
        h.key(Key::H, NONE);
        assert!(h.cam() == fitted, "so does H");
        // pan: two-finger scroll, middle-drag, Space+drag; the plane has no edge
        let pan = h.cam().pan;
        for _ in 0..3 {
            h.frame(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, -40.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Trackpad, modifiers: NONE }]);
        }
        for _ in 0..30 {
            h.frame(Vec::new());
        }
        assert!(h.cam().pan.y < pan.y - 100.0 && (h.cam().pan.x - pan.x).abs() < 1e-3, "scrolling pans: {:?} → {:?}", pan, h.cam().pan);
        let (pan, at) = (h.cam().pan, Harness::center() + vec2(0.0, 150.0));
        h.move_to(at);
        h.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Middle, pressed: true, modifiers: NONE }]);
        for k in 1..=6 {
            h.move_to(at + vec2(-50.0 * k as f32, 0.0));
        }
        h.frame(vec![Event::PointerButton { pos: at + vec2(-300.0, 0.0), button: PointerButton::Middle, pressed: false, modifiers: NONE }]);
        assert!((h.cam().pan - (pan + vec2(-300.0, 0.0))).length() < 1e-3, "middle-drag pans by the pointer's travel");
        let pan = h.cam().pan;
        h.frame(vec![Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: NONE }]);
        h.drag(at, at + vec2(0.0, -120.0));
        h.frame(vec![Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: false, repeat: false, modifiers: NONE }]);
        assert!((h.cam().pan - (pan + vec2(0.0, -120.0))).length() < 1e-3, "Space+drag pans");
        assert!(h.app.canvas.selected.is_empty(), "…and selects nothing");
        // panels: Cmd+B, then focus mode hides them all and brings them back
        h.key(Key::B, CMD);
        assert!(!h.app.settings.panels.scenes);
        h.key(Key::Backslash, NONE);
        assert!(h.app.settings.panels == crate::Panels::NONE);
        // | on an Italian keyboard: Shift and the \ key
        h.key(Key::Pipe, Modifiers::SHIFT);
        assert!(h.app.settings.panels.inspector && !h.app.settings.panels.scenes, "focus mode restores what was open");
    }

    /// The scene the engine has on the output, once `ok` holds (real time: the engine runs on
    /// its own thread), within 10 s of frames.
    fn until(h: &mut Harness, what: &str, ok: impl Fn(&crate::live::Live) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ok(h.app.live.as_ref().unwrap()) {
            assert!(std::time::Instant::now() < deadline, "{what} within 10 s");
            h.frame(Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn the_ai_panel_may_change_the_live_scene_outside_agents_may_not() {
        let Ok(engine) = velo_engine::Engine::start(None) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let mut h = Harness::new();
        let first = h.scene().id.clone();
        h.app.live = Some(crate::live::Live::new(std::rc::Rc::new(engine), std::rc::Rc::default()));
        until(&mut h, "the first scene live", |l| l.live_scene() == Some(first.as_str()));
        let add = json!({ "id": "node.add", "props": { "kind": "velo.blur" } });
        let nodes = h.scene().nodes.len();
        assert!(h.app.agent_call("run_operator", &add, false).unwrap_err().contains("live"), "an outside agent is kept off it");
        assert_eq!(h.scene().nodes.len(), nodes);
        assert!(h.app.agent_call("run_operator", &add, true).is_ok(), "the panel: the user asked, in Velo");
        assert_eq!(h.scene().nodes.len(), nodes + 1);
    }

    #[test]
    fn number_keys_and_a_space_tap_launch_scenes_on_the_engine() {
        let Ok(engine) = velo_engine::Engine::start(None) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let mut h = Harness::new();
        let first = h.scene().id.clone();
        // a second scene that goes out at once, never built before its key is pressed
        h.app.run("scene.add", json!({ "name": "Drop" }));
        h.app.run("node.add", json!({ "kind": "velo.noise" }));
        h.app.run("scene.set_launch", json!({ "transition": "Cut", "quantize": "Off" }));
        let second = h.scene().id.clone();
        h.app.run("scene.select", json!({ "scene": first }));
        h.app.live = Some(crate::live::Live::new(std::rc::Rc::new(engine), std::rc::Rc::default()));
        // at start the selected scene goes live; the other isn't built (a launch builds it first)
        until(&mut h, "the first scene live", |l| l.live_scene() == Some(first.as_str()));
        h.key(Key::Num2, NONE);
        until(&mut h, "scene 2 launched by its key", |l| l.live_scene() == Some(second.as_str()));
        // the first scene, selected and not live: its preview, and Space launches it…
        h.app.run("scene.set_launch", json!({ "scene": first, "transition": "Cut", "quantize": "Off" }));
        h.frame(Vec::new());
        assert_eq!(h.app.session.active_scene.as_deref(), Some(first.as_str()));
        h.key(Key::Space, NONE);
        until(&mut h, "a Space tap launching the selected scene", |l| l.live_scene() == Some(first.as_str()));
        // …but not when it's held to pan the canvas
        h.app.run("scene.select", json!({ "scene": second }));
        let at = Harness::center();
        h.frame(vec![Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: NONE }]);
        h.drag(at, at + vec2(40.0, 0.0));
        h.frame(vec![Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: false, repeat: false, modifiers: NONE }]);
        for _ in 0..20 {
            h.frame(Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(h.app.live.as_ref().unwrap().live_scene(), Some(first.as_str()), "a pan isn't a launch");
        // 3 has no scene: nothing happens
        h.key(Key::Num3, NONE);
        h.frame(Vec::new());
        assert_eq!(h.app.live.as_ref().unwrap().live_scene(), Some(first.as_str()));
    }
}

#[cfg(test)]
mod midi_tests {
    use std::time::Instant;

    use eframe::egui::{self, Event, RawInput};
    use eframe::egui::{Key, Modifiers, PointerButton, Pos2, Rect, Vec2, pos2, vec2};
    use serde_json::json;
    use velo_core::{Behavior, Control, MidiMsg, RelMode, Takeover, Target};

    use crate::App;
    use crate::midi::Event as Midi;

    const SCREEN: Vec2 = vec2(1280.0, 800.0);

    /// The whole UI, headless, with one scene; MIDI comes in through the app's queue.
    struct Ui {
        ctx: egui::Context,
        app: App,
        time: f64,
        access: Option<egui::accesskit::TreeUpdate>,
    }

    impl Ui {
        fn new() -> Self {
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            ctx.enable_accesskit();
            let mut app = App::for_tests();
            app.theme.apply(&ctx);
            app.run("scene.add", json!({}));
            app.run("node.add", json!({ "kind": "velo.kaleido" }));
            let mut ui = Self { ctx, app, time: 0.0, access: None };
            ui.frame(Vec::new());
            ui.frame(Vec::new());
            ui
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.time), events, ..Default::default() };
            let (ctx, app) = (&self.ctx, &mut self.app);
            let mut out = ctx.run_ui(input, |ui| app.draw(ui));
            self.access = out.platform_output.accesskit_update.take();
            out.drop_without_applying_deltas();
        }

        fn widget(&self, label: &str) -> Rect {
            let (_, node) = self.access.as_ref().and_then(|a| a.nodes.iter().find(|(_, n)| n.label() == Some(label))).unwrap_or_else(|| panic!("no widget {label:?}"));
            let b = node.bounds().unwrap();
            Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32))
        }

        fn click(&mut self, p: Pos2) {
            self.frame(vec![Event::PointerMoved(p)]);
            for pressed in [true, false] {
                self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
            }
        }

        fn key(&mut self, key: Key) {
            for pressed in [true, false] {
                self.frame(vec![Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: Modifiers::NONE }]);
            }
        }

        /// A message from a controller, then a frame to take it.
        fn midi(&mut self, channel: u8, msg: MidiMsg, value: u8) {
            self.app.midi.sender().send(Midi { port: "Knobs".into(), at: Instant::now(), channel, msg, value }).unwrap();
            self.frame(Vec::new());
        }

        fn macro_value(&self, i: usize) -> f32 {
            self.app.session.active().unwrap().macros[i].value
        }
    }

    #[test]
    fn osc_moves_macros_selects_scenes_and_runs_operators() {
        use crate::osc::{Arg, Message};
        let mut h = Ui::new();
        let m = |address: &str, args: Vec<Arg>| Message { address: address.into(), args, at: Instant::now() };
        assert!(h.app.apply_osc(&m("/velo/scene/macro/1", vec![Arg::Float(0.9)]), 1.0));
        assert!((h.macro_value(0) - 0.9).abs() < 1e-6, "{}", h.macro_value(0));
        assert!(!h.app.apply_osc(&m("/velo/scene/macro/9", vec![Arg::Float(0.5)]), 1.0), "there are 8 macros");
        assert!(h.app.apply_osc(&m("/velo/op/scene.add", vec![Arg::Str(r#"{"name": "From OSC"}"#.into())]), 1.0));
        assert_eq!(h.app.session.active().map(|s| s.name.as_str()), Some("From OSC"));
        assert!(h.app.apply_osc(&m("/velo/scene/select", vec![Arg::Int(1)]), 1.0));
        assert_eq!(h.app.session.active_scene, h.app.session.project.scenes.first().map(|s| s.id.clone()));
        assert!(h.app.apply_osc(&m("/velo/scene/select", vec![Arg::Str("from osc".into())]), 1.0), "by name, any case");
        assert!(!h.app.apply_osc(&m("/velo/op/rm.rf", vec![]), 1.0), "only operators");
        assert!(!h.app.apply_osc(&m("/velo/nothing", vec![]), 1.0));
    }

    #[test]
    fn an_endless_encoder_is_learned_as_one_and_turns_by_clicks() {
        let mut h = Ui::new();
        h.key(Key::L);
        h.click(h.widget("Macro 1 0 %").center());
        // three clicks clockwise in two's complement: only an encoder sends that
        for _ in 0..3 {
            h.midi(0, MidiMsg::Cc(30), 1);
        }
        let b = h.app.session.project.bindings.first().expect("bound");
        assert_eq!(b.behavior, Behavior::Relative { mode: RelMode::TwosComplement, step: 1.0 / 128.0 });
        h.key(Key::Escape);
        assert_eq!(h.macro_value(0), 0.0, "the learning turn moved nothing");
        for _ in 0..8 {
            h.midi(0, MidiMsg::Cc(30), 2);
        }
        assert!((h.macro_value(0) - 16.0 / 128.0).abs() < 1e-6, "8 messages of 2 clicks: {}", h.macro_value(0));
        h.midi(0, MidiMsg::Cc(30), 127);
        assert!((h.macro_value(0) - 15.0 / 128.0).abs() < 1e-6, "one click back");
    }

    #[test]
    fn learn_binds_the_clicked_control_to_the_first_knob_that_moves() {
        let mut h = Ui::new();
        h.key(Key::L);
        assert!(h.app.learn.on, "L starts learn mode");
        h.click(h.widget("Macro 1 0 %").center());
        assert_eq!(h.app.learn.target, Some(Target::Macro(0)), "a click picks the control, it doesn't turn it");
        // jitter of one step doesn't count; two steps bind
        h.midi(0, MidiMsg::Cc(21), 64);
        h.midi(0, MidiMsg::Cc(21), 65);
        assert!(h.app.session.project.bindings.is_empty());
        h.midi(0, MidiMsg::Cc(21), 66);
        let b = h.app.session.project.bindings.first().expect("bound");
        assert_eq!(b.control, Control::Midi { port: Some("Knobs".into()), channel: 0, msg: MidiMsg::Cc(21) });
        assert_eq!((&b.target, b.behavior), (&Target::Macro(0), Behavior::Absolute { takeover: Takeover::Pickup }));
        assert_eq!(h.app.learn.target, None, "learn mode goes on to the next control");
        assert_eq!(h.macro_value(0), 0.0, "the learning move moved nothing");
        // a pad on a scene card: a note-on binds it
        h.click(h.widget("Scene 1").center());
        h.midi(9, MidiMsg::Note(36), 0);
        assert_eq!(h.app.session.project.bindings.len(), 1, "a note-off isn't a press");
        h.midi(9, MidiMsg::Note(36), 100);
        assert_eq!(h.app.session.project.binding(&Target::LaunchScene(0)).map(|b| b.behavior), Some(Behavior::Trigger));
        h.key(Key::Escape);
        assert!(!h.app.learn.on);
        // the knob is at 66 (52 %), the macro at 0: it picks up only once it gets there
        h.midi(0, MidiMsg::Cc(21), 70);
        assert_eq!(h.macro_value(0), 0.0, "soft takeover: not yet");
        h.midi(0, MidiMsg::Cc(21), 0);
        h.midi(0, MidiMsg::Cc(21), 50);
        assert!((h.macro_value(0) - 50.0 / 127.0).abs() < 1e-6, "picked up, then followed: {}", h.macro_value(0));
        h.midi(0, MidiMsg::Cc(21), 60);
        assert_eq!(h.app.session.undo_label(), Some("Set Macro"));
        h.app.run("edit.undo", json!({}));
        assert_eq!(h.macro_value(0), 0.0, "a sweep from a controller is one undo step");
        // a variation on a pad, bound by the operator: each press recalls it
        h.app.run("macro.set", json!({ "index": 0, "value": 0.25 }));
        h.app.run("variation.store", json!({}));
        h.app.run("macro.set", json!({ "index": 0, "value": 1.0 }));
        h.app.run("binding.add", json!({ "control": { "Midi": { "port": null, "channel": 0, "msg": { "Note": 40 } } }, "target": { "Variation": 0 }, "behavior": "Trigger" }));
        h.midi(0, MidiMsg::Note(40), 127);
        assert_eq!(h.macro_value(0), 0.25, "any port: the pad recalls A");
    }
}
