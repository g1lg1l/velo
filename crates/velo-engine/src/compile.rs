//! From a scene to a graph the render thread can run (ARCHITECTURE.md §3.2): which built-in
//! shader each node runs, what feeds it and which node goes out ([`plan`], pure data), then the
//! programs and textures, built on a worker ([`Compiler`]) so that neither the UI nor the render
//! thread ever compiles.

use std::collections::HashMap;
use std::sync::mpsc;

use std::borrow::Cow;

use velo_core::{Category, Edge, FEEDBACK, IMAGE, LIVE, LIVE_PROGRAM, Layer, MEDIA_LIST, MESH, Node, OUTPUT, PARTICLES, PROBE, SHADER, SHADER_FX, Scene, TEXT_LIST, is_shader, node_kind, shows_media};

use crate::media::{Lines, Reel, SLOTS};

use crate::gpu::{Lazy, topological};
use crate::{BUILTINS, Graph, Macros, Modulator, NodeSpec, Program, Runner, ShaderError, ShaderNode, macro_routes, routes};

/// A new Shader node's code: an audio-reactive tunnel that shows what the format offers.
pub const TEMPLATE: &str = include_str!("../shaders/templates/tunnel.wgsl");
/// What a new Shader FX node runs: its input, turned round the hue and zoomed.
pub const EFFECT_TEMPLATE: &str = include_str!("../shaders/templates/effect.wgsl");

/// The code a new node of this kind starts from: a Shader's or a Shader FX's first preset (its
/// template is the Custom one its inspector offers, code to make your own from).
pub fn template(kind: &str) -> Option<&'static str> {
    match kind {
        SHADER => Some(crate::PRESETS[0].1),
        SHADER_FX => Some(crate::EFFECT_PRESETS[0].1),
        _ => None,
    }
}
/// What a Shader node runs until its code first compiles: its input, untouched.
const BROKEN: &str = include_str!("../shaders/broken.wgsl");
/// A Probe node's picture side ([`crate::gpu::Probe`]).
const PROBE_SHADER: &str = include_str!("../shaders/probe.wgsl");
/// The Image shader for Hap Q movies: their YCoCg to RGB.
pub(crate) static IMAGE_YCOCG: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| include_str!("../shaders/image.wgsl").replace("const YCOCG: bool = false;", "const YCOCG: bool = true;"));

/// The built-in shader a node kind runs ("velo.kaleido" → "kaleido"), if it has one yet.
/// A Shader node runs its own code.
pub fn builtin(kind: &str) -> Option<&'static str> {
    let name = kind.strip_prefix("velo.")?;
    // a Video node runs the Image shader: the same picture, from a movie; a Live node too, from a feed
    let name = if name == "video" || name == "live" { "image" } else { name };
    BUILTINS.iter().find(|b| b.0 == name).map(|b| b.0)
}

/// A scene's graph as the engine runs it: on nodes that have a shader, in scene order.
/// Off nodes pass their first input through; other nodes without a shader give black.
#[derive(Debug, PartialEq)]
pub struct Plan {
    /// Each engine node: the scene node's id and what it runs.
    pub nodes: Vec<(String, Code)>,
    /// Which engine node feeds each of a node's inputs (None: black).
    pub inputs: Vec<Vec<Option<usize>>>,
    /// The engine node whose picture goes out: the Output node, grading what feeds it (the
    /// last engine node), or, with no Output node, the last node (upstream first) that nothing
    /// reads.
    pub output: Option<usize>,
    /// Each Probe node that's on, and the engine node whose picture it reads (none: nothing
    /// wired, or only off nodes).
    pub probes: Vec<(String, Option<usize>)>,
    /// The engine nodes that show what plays under the scene: the Live nodes on the program
    /// ([`LIVE_PROGRAM`]) of a layer on top. In any other scene nothing is under it to read, so
    /// there are none and such a node shows black.
    pub under: Vec<usize>,
}

/// What a node runs: a built-in shader, or a Shader node's own code.
#[derive(Clone, Debug, PartialEq)]
pub enum Code {
    Builtin(&'static str),
    Own(String),
}

/// A Shader node's code, parsed, as the node can run it (naga validates it when it's built):
/// Velo's WGSL format, or an ISF shader (GLSL, translated).
pub fn own(code: &str) -> Result<ShaderNode, ShaderError> {
    let node = if crate::isf::is_isf(code) { crate::isf::parse(code)? } else { ShaderNode::parse(code)? };
    if node.inputs.len() > 1 {
        return Err(ShaderError { line: None, message: "a Shader node has one input for now: declare at most one texture (src)".into() });
    }
    if node.particles || node.mesh {
        return Err(ShaderError { line: None, message: "particles and meshes are the Particles and Mesh nodes': a Shader node draws main_image".into() });
    }
    Ok(node)
}

/// How many inputs a node reads: its kind's, or a Media List's slots ([`expand`]).
fn ports(n: &Node) -> usize {
    if n.kind == MEDIA_LIST { SLOTS } else { node_kind(&n.kind).map_or(0, |k| k.inputs.len()) }
}

/// The scene as the engine builds it: a Media List with files gets [`SLOTS`] Image nodes of
/// its own (`<list id>#<k>`) wired into it, with no file: its [`Reel`] streams its files
/// through them.
fn expand(scene: &Scene) -> Cow<'_, Scene> {
    let lists = |n: &&Node| n.kind == MEDIA_LIST && !velo_core::media_list(n).is_empty();
    if !scene.nodes.iter().any(|n| lists(&n)) {
        return Cow::Borrowed(scene);
    }
    let mut s = scene.clone();
    for n in scene.nodes.iter().filter(lists) {
        for k in 0..SLOTS {
            let id = format!("{}#{k}", n.id);
            let file = Node { id: id.clone(), kind: IMAGE.into(), name: String::new(), off: n.off, pos: n.pos, params: Default::default(), mappings: Vec::new(), code: None, file: None, media: None, text: None, icon: String::new() };
            s.nodes.push(file);
            s.edges.push(Edge { from: (id, 0), to: (n.id.clone(), k as u8) });
        }
    }
    Cow::Owned(s)
}

pub fn plan(scene: &Scene) -> Plan {
    plan_of(&expand(scene))
}

fn plan_of(scene: &Scene) -> Plan {
    let mut all = Vec::new();
    for n in &scene.nodes {
        let code = match (builtin(&n.kind), &n.code) {
            // the Output going out comes last, below
            _ if n.kind == OUTPUT => continue,
            (Some(shader), _) => Code::Builtin(shader),
            (None, Some(code)) if is_shader(&n.kind) => Code::Own(code.clone()),
            _ => continue,
        };
        all.push((n.id.clone(), code));
    }
    let mut nodes: Vec<_> = all.iter().filter(|(id, _)| scene.node(id).is_some_and(|n| !n.off)).cloned().collect();
    let index: HashMap<_, _> = nodes.iter().enumerate().map(|(i, (id, _))| (id.as_str(), i)).collect();
    let resolve = |id: &str| {
        let mut node = scene.node(id)?;
        // a chain longer than the scene has a loop: black
        for _ in 0..scene.nodes.len() {
            if !node.off {
                return index.get(node.id.as_str()).copied();
            }
            // a generator's pins feed it, they aren't what it shows
            if node_kind(&node.kind).is_none_or(|k| k.inputs.is_empty() || k.category == Category::Generator) {
                return None;
            }
            node = scene.node(&scene.wire_into(&node.id, 0)?.from.0)?;
        }
        None
    };
    let feeds = |node: &str, port: u8| scene.wire_into(node, port).and_then(|e| resolve(&e.from.0));
    let mut inputs: Vec<Vec<Option<usize>>> = nodes
        .iter()
        .map(|(id, _)| {
            let ports = scene.node(id).map_or(0, ports);
            (0..ports as u8).map(|p| feeds(id, p)).collect()
        })
        .collect();
    let probes = scene.nodes.iter().filter(|n| n.kind == PROBE && !n.off).map(|n| (n.id.clone(), feeds(&n.id, 0))).collect();
    // the Output node grades what's wired in; with several, the last one something is wired
    // into. Fed by nothing that renders, nothing goes out.
    let outputs: Vec<&velo_core::Node> = scene.nodes.iter().filter(|n| node_kind(&n.kind).is_some_and(|k| k.category == Category::Output)).collect();
    let wired = |n: &velo_core::Node| scene.edges.iter().any(|e| e.to.0 == n.id);
    let out = outputs.iter().rev().find(|n| wired(n)).or(outputs.first()).copied();
    let output = match out {
        Some(out) => feeds(&out.id, 0).map(|from| {
            nodes.push((out.id.clone(), Code::Builtin("output")));
            inputs.push(vec![Some(from)]);
            nodes.len() - 1
        }),
        None => {
            // pick the original endpoint before bypassing it, including an off generator
            let index: HashMap<_, _> = all.iter().enumerate().map(|(i, (id, _))| (id.as_str(), i)).collect();
            let specs: Vec<NodeSpec> = all
                .iter()
                .map(|(id, _)| {
                    let ports = scene.node(id).map_or(0, ports);
                    let inputs = (0..ports as u8).map(|p| scene.wire_into(id, p).and_then(|e| index.get(e.from.0.as_str()).copied())).collect();
                    NodeSpec { program: 0, inputs }
                })
                .collect();
            let feedback: Vec<bool> = all.iter().map(|(id, _)| scene.node(id).is_some_and(|n| n.kind == FEEDBACK)).collect();
            let read = |n: usize| scene.edges.iter().any(|e| e.from.0 == all[n].0);
            topological(&specs, &feedback).ok().and_then(|order| order.into_iter().rev().find(|&n| !read(n))).and_then(|n| resolve(&all[n].0))
        }
    };
    let reads_under = |id: &str| scene.node(id).is_some_and(|n| n.kind == LIVE && n.media.as_deref() == Some(LIVE_PROGRAM));
    let under = nodes.iter().enumerate().filter(|(_, (id, _))| scene.layer == Layer::Top && reads_under(id)).map(|(n, _)| n).collect();
    Plan { nodes, inputs, output, probes, under }
}

/// A graph built for a scene, with the plan it was built from (the UI maps node ids to the
/// engine's node indices with it, for `Command::Params`), each node's shader as it runs, and
/// the Shader nodes whose code didn't compile (they run their last good code, else their input).
pub struct Built {
    pub graph: Box<Graph>,
    pub plan: Plan,
    /// The scene's signal nodes' outputs ("n_ab12/0"), in the order their values follow the
    /// shared sources.
    pub signals: Vec<String>,
    pub shaders: Vec<ShaderNode>,
    pub errors: Vec<(String, ShaderError)>,
    /// Each Image node's file (node id): built in once decoded; a load still going (or failed)
    /// showed black, and the graph is built again when it's in. The flag says whether it was in
    /// when this graph was built: a file that lands while its graph is building isn't in it, and
    /// asking the file again once the graph arrives would say it is.
    pub media: Vec<(String, crate::media::Loading, bool)>,
}

/// Builds graphs on its own thread with the engine's device. Each shader is compiled once and
/// reused by every later graph. Builds carry the caller's key (one per scene graph); when edits
/// come faster than builds, only the newest of each key is built.
pub struct Compiler {
    jobs: mpsc::Sender<Job>,
    /// Finished builds with their keys, in the order they were asked for.
    pub built: mpsc::Receiver<(u64, Result<Built, String>)>,
}

type Job = (u64, Scene, [u32; 2]);

impl Compiler {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, wave: wgpu::TextureView) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, built) = mpsc::channel();
        let worker = move || {
            // built-in programs by name; each Shader node's last code that compiled, by node id;
            // Image nodes' files
            let (mut cache, mut good, mut library) = (HashMap::new(), HashMap::new(), crate::media::Library::default());
            while let Ok(job) = rx.recv() {
                let mut todo = vec![job];
                for newer in rx.try_iter() {
                    match todo.iter_mut().find(|j| j.0 == newer.0) {
                        Some(older) => *older = newer,
                        None => todo.push(newer),
                    }
                }
                for (key, scene, size) in todo {
                    // a build that panics is that scene's error: the worker goes on with the next
                    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build((&device, &queue, &wave), &mut cache, &mut good, &mut library, &scene, size)));
                    if tx.send((key, built.unwrap_or_else(|_| Err("this scene couldn't be built".into())))).is_err() {
                        return;
                    }
                }
            }
        };
        // a failed spawn leaves `built` disconnected: the canvas just doesn't render
        let _ = std::thread::Builder::new().name("velo compile".into()).spawn(worker);
        Self { jobs, built }
    }

    /// Builds `scene` at `size` (the project's canvas) under `key`: the result comes back on
    /// [`Self::built`].
    pub fn build(&self, key: u64, scene: Scene, size: [u32; 2]) {
        let _ = self.jobs.send((key, scene, size));
    }
}

/// A built-in program, compiled once.
fn cached(device: &wgpu::Device, cache: &mut HashMap<&'static str, Program>, name: &'static str, source: &str) -> Result<Program, String> {
    if let Some(program) = cache.get(name) {
        return Ok(program.clone());
    }
    let program = ShaderNode::parse(source).and_then(|node| Program::new(device, node)).map_err(|e| format!("{name}: {e}"))?;
    cache.insert(name, program.clone());
    Ok(program)
}

fn build(
    (device, queue, wave): (&wgpu::Device, &wgpu::Queue, &wgpu::TextureView),
    cache: &mut HashMap<&'static str, Program>,
    good: &mut HashMap<String, Program>,
    library: &mut crate::media::Library,
    scene: &Scene,
    size: [u32; 2],
) -> Result<Built, String> {
    let scene = &*expand(scene);
    let plan = plan_of(scene);
    // the scene's signal nodes: their outputs come after the audio and clock sources
    let (run, signals) = crate::signal::plan(scene);
    // Image nodes' files, Text nodes' words: loading, or in
    let files: Vec<Option<crate::media::Loading>> = plan
        .nodes
        .iter()
        .map(|(id, _)| {
            let n = scene.node(id)?;
            if velo_core::sets_text(&n.kind) {
                return Some(library.text(n.text.as_deref().unwrap_or_default(), n.media.as_deref()));
            }
            // a Live node's name is a feed, found by the library on a thread of its own
            // (but the program: no feed, the render thread's own picture, below)
            if n.kind == LIVE {
                return n.media.as_deref().filter(|source| *source != LIVE_PROGRAM).map(|source| library.live(source));
            }
            // a Particles node's file is its sprite
            n.media.as_deref().filter(|_| shows_media(&n.kind) || n.kind == PARTICLES || n.kind == MESH).map(|path| library.get(path))
        })
        .collect();
    // the files in before anything is built from them
    let was_in: Vec<bool> = files.iter().map(|l| l.as_ref().is_some_and(|l| l.get().is_some())).collect();
    let file = |n: usize| files[n].as_ref().and_then(|l| l.get()).and_then(|r| r.as_ref().ok());
    let (mut programs, mut slots, mut errors) = (Vec::new(), HashMap::new(), Vec::new());
    let mut uses = Vec::with_capacity(plan.nodes.len());
    for (n, (id, code)) in plan.nodes.iter().enumerate() {
        let program = match code {
            Code::Builtin(shader) => {
                let shader = if file(n).is_some_and(|m| m.ycocg()) { "image-ycocg" } else { shader };
                if let Some(&slot) = slots.get(shader) {
                    uses.push(slot);
                    continue;
                }
                slots.insert(shader, programs.len());
                let source = if shader == "image-ycocg" { IMAGE_YCOCG.as_str() } else { BUILTINS.iter().find(|b| b.0 == shader).map(|b| b.1).ok_or_else(|| format!("no shader {shader}"))? };
                cached(device, cache, shader, source)?
            }
            // a broken Shader node never stops the scene: it keeps its last good code
            Code::Own(code) => match own(code).and_then(|node| Program::new(device, node)) {
                Ok(program) => {
                    good.insert(id.clone(), program.clone());
                    program
                }
                Err(e) => {
                    errors.push((id.clone(), e));
                    match good.get(id) {
                        Some(program) => program.clone(),
                        None => cached(device, cache, "velo-broken", BROKEN)?,
                    }
                }
            },
        };
        uses.push(programs.len());
        programs.push(program);
    }
    let specs: Vec<NodeSpec> = uses.iter().zip(&plan.inputs).map(|(&program, inputs)| NodeSpec { program, inputs: inputs.clone() }).collect();
    // Image nodes: the files in so far, each on a texture of its own (a movie needs BC
    // textures; ponytail: without them it shows black, a message if such GPUs ever turn up)
    let bc = device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC);
    let mut playing = Vec::new();
    // what the GPU refuses from here (out of video memory, a binding) is the build's error
    let (memory, valid) = (device.push_error_scope(wgpu::ErrorFilter::OutOfMemory), device.push_error_scope(wgpu::ErrorFilter::Validation));
    for n in 0..plan.nodes.len() {
        // a model has no picture to play: its points go up below
        if let Some(m) = file(n).filter(|m| m.model.is_none() && (m.movie.is_none() || bc)) {
            let speed = crate::modulate::slot(&programs[specs[n].program].node, "speed").map(|s| s.0);
            let mut p = crate::media::Playing::new(device, queue, n, speed, m.clone());
            let node = &programs[specs[n].program].node;
            p.range = crate::modulate::slot(node, "loop_start").zip(crate::modulate::slot(node, "loop_end")).map(|(a, b)| (a.0, b.0));
            playing.push(p);
        }
    }
    let models: Vec<(usize, std::sync::Arc<crate::media::Media>)> = (0..plan.nodes.len()).filter_map(|n| file(n).filter(|m| m.model.is_some()).map(|m| (n, m.clone()))).collect();
    let media = plan.nodes.iter().zip(files).zip(was_in).filter_map(|(((id, _), l), was_in)| Some((id.clone(), l?, was_in))).collect();
    let mut views: Vec<(usize, wgpu::TextureView)> = playing.iter().map(|p| (p.node, p.texture.create_view(&Default::default()))).collect();
    // what plays under the layer: one picture the render thread fills each frame, read by every
    // node that shows it as an Image node reads its file
    let under = (!plan.under.is_empty()).then(|| crate::gpu::texture_for(device, size, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST));
    views.extend(under.iter().flat_map(|view| plan.under.iter().map(move |&n| (n, view.clone()))));
    let runner = Runner::new(device, &programs, &specs, &views, size, wave);
    if let Some(e) = [pollster::block_on(valid.pop()), pollster::block_on(memory.pop())].into_iter().flatten().next() {
        return Err(crate::gpu_error(&e));
    }
    let mut runner = runner?;
    // a model's points up for a Particles node, its triangles for a Mesh node (the other does nothing)
    for (n, media) in &models {
        if let Some(model) = &media.model {
            runner.set_points(queue, *n, &model.points);
            runner.set_mesh(device, *n, &model.mesh, &model.uv);
        }
    }
    // the Probes' picture sides, each reading its node for its signal node
    let mut probes = Vec::new();
    for (id, node) in plan.probes.iter().filter_map(|(id, node)| Some((id, (*node)?))) {
        let slot = signals.iter().position(|k| *k == format!("{id}/0")).map(|k| velo_core::SOURCES.len() + k);
        if let Some(signal) = run.iter().position(|s| Some(s.slot) == slot) {
            probes.push(crate::gpu::Probe::new(device, &cached(device, cache, "velo-probe", PROBE_SHADER)?, &runner, node, signal));
        }
    }
    // the values and mappings as they were asked for: the first frame is already right, and a
    // rebuild never drops a mapping
    let mut mods = Vec::with_capacity(plan.nodes.len());
    for (n, (id, _)) in plan.nodes.iter().enumerate() {
        let (node, shader) = (scene.node(id), &programs[specs[n].program].node);
        let values = shader.values(|p| node.and_then(|node| velo_core::engine_param(scene, node, p)));
        runner.set_params(queue, &programs, n, &values);
        // shortcut: mappings of the shared signals come with the UI's next sync (it resends every
        // route once a graph is in), a frame late at most; pass the shared plan here if that shows
        mods.push(Modulator::new(shader, &values, node.map_or_else(Vec::new, |node| routes(shader, &node.mappings, &signals, &[]))));
    }
    // and the macros where they are, driving what they drive
    let nodes: Vec<(&str, &ShaderNode)> = plan.nodes.iter().zip(&specs).map(|((id, _), spec)| (id.as_str(), &programs[spec.program].node)).collect();
    let macros = Macros::new(scene.macros.each_ref().map(|m| m.value), macro_routes(&nodes, &scene.macros));
    macros.drive(&mut mods);
    // the nodes that read only some of their inputs, with where their deciding values are
    let lazy = plan
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(n, (_, code))| {
            let at = |param| crate::modulate::slot(&programs[specs[n].program].node, param).map(|s| s.0);
            match code {
                Code::Builtin("switch") => Some((n, Lazy::Switch { value: at("value")?, count: at("count")?, fade: at("fade")? })),
                Code::Builtin("blend") => Some((n, Lazy::Blend { opacity: at("opacity")? })),
                _ => None,
            }
        })
        .collect();
    // each Media List's files, streamed through its slots
    let index = |id: &str| plan.nodes.iter().position(|(n, _)| n == id);
    let lists = plan
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(n, (id, _))| {
            let list = scene.node(id).filter(|l| l.kind == MEDIA_LIST)?;
            let [a, b, c] = [0, 1, 2].map(|k| index(&format!("{id}#{k}")));
            let files = velo_core::media_list(list).into_iter().map(String::from).collect();
            Reel::new(device, queue, id.clone(), n, &programs[specs[n].program].node, [a?, b?, c?], files, bc)
        })
        .collect();
    // each Text List's items, counted: the line showing is picked for its shader every frame
    let lines = plan
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(n, (id, _))| {
            let list = scene.node(id).filter(|l| l.kind == TEXT_LIST)?;
            Lines::new(n, &programs[specs[n].program].node, velo_core::text_items(list.text.as_deref().unwrap_or_default()).len())
        })
        .collect();
    let shaders = specs.iter().map(|spec| programs[spec.program].node.clone()).collect();
    Ok(Built { graph: Box::new(Graph { programs, runner, output: plan.output, mods, macros, signals: run, probes, media: playing, lists, lines, lazy, under }), plan, signals, shaders, errors, media })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use velo_core::Session;

    use super::*;

    /// A scene built through the operators: `kinds` in order, then `wires` as (from, to, port).
    fn scene(kinds: &[&str], wires: &[(usize, usize, u8)]) -> Scene {
        let mut s = Session::default();
        s.run("scene.add", json!({})).unwrap();
        for kind in kinds {
            s.run("node.add", json!({ "kind": kind })).unwrap();
        }
        let ids: Vec<String> = s.active().unwrap().nodes.iter().map(|n| n.id.clone()).collect();
        for &(a, b, port) in wires {
            s.run("edge.connect", json!({ "from": [ids[a], 0], "to": [ids[b], port] })).unwrap();
        }
        s.active().unwrap().clone()
    }

    fn shaders(p: &Plan) -> Vec<&str> {
        p.nodes
            .iter()
            .map(|n| match &n.1 {
                Code::Builtin(shader) => shader,
                Code::Own(_) => "own",
            })
            .collect()
    }

    #[test]
    fn every_kind_with_a_shader_maps_to_its_builtin() {
        let mapped: std::collections::BTreeSet<&str> = velo_core::NODE_KINDS.iter().filter_map(|k| builtin(k.id)).collect();
        assert_eq!(mapped.len(), BUILTINS.len(), "each built-in shader has its node kind: {mapped:?}");
        assert_eq!(builtin("velo.video"), Some("image"));
        assert_eq!(builtin("velo.kaleido"), Some("kaleido"));
        assert_eq!(builtin("velo.output"), Some("output"));
        for none in ["velo.shader", "user.tunnel"] {
            assert_eq!(builtin(none), None, "{none}");
        }
    }

    #[test]
    fn a_grid_is_a_node_its_picture_goes_on() {
        let p = plan(&scene(&["velo.noise", "velo.gradient", "velo.grid", "velo.output"], &[(0, 2, 0), (1, 2, 3), (2, 3, 0)]));
        assert_eq!(shaders(&p), ["noise", "gradient", "grid", "output"]);
        assert_eq!(p.inputs[2], vec![Some(0), None, None, Some(1), None, None, None, None]);
        assert_eq!((&p.inputs[3], p.output), (&vec![Some(2)], Some(3)));
        // not an output: with no Output node it goes out as any last node would, and an effect
        // can take it on
        assert_eq!(plan(&scene(&["velo.noise", "velo.grid"], &[(0, 1, 0)])).output, Some(1));
        let on = plan(&scene(&["velo.noise", "velo.grid", "velo.kaleido", "velo.output"], &[(0, 1, 0), (1, 2, 0), (2, 3, 0)]));
        assert_eq!((shaders(&on), on.output), (vec!["noise", "grid", "kaleido", "output"], Some(3)));
    }

    #[test]
    fn noise_kaleido_output_goes_out_through_the_outputs_grade() {
        let s = scene(&["velo.noise", "velo.kaleido", "velo.output"], &[(0, 1, 0), (1, 2, 0)]);
        let p = plan(&s);
        assert_eq!(shaders(&p), ["noise", "kaleido", "output"]);
        assert_eq!(p.nodes[2].0, s.nodes[2].id);
        assert_eq!(p.inputs, vec![vec![], vec![Some(0)], vec![Some(1)]]);
        assert_eq!(p.output, Some(2));
    }

    #[test]
    fn nodes_without_a_shader_render_nothing_and_their_readers_see_black() {
        // Shader → Blend.a, Noise → Blend.b, Blend → Output
        let p = plan(&scene(&["velo.shader", "velo.noise", "velo.blend", "velo.output"], &[(0, 2, 0), (1, 2, 1), (2, 3, 0)]));
        assert_eq!(shaders(&p), ["noise", "blend", "output"]);
        assert_eq!(p.inputs[1][..2], [None, Some(0)]);
        assert_eq!(p.output, Some(2));
        // an Output fed by nothing that renders, or by nothing at all: black, and no grade to run
        let p = plan(&scene(&["velo.shader", "velo.output"], &[(0, 1, 0)]));
        assert_eq!((p.nodes.len(), p.output), (0, None));
        let p = plan(&scene(&["velo.noise", "velo.output"], &[]));
        assert_eq!((shaders(&p), p.output), (vec!["noise"], None));
    }

    #[test]
    fn a_mesh_plans_alone_as_it_did_and_with_the_picture_wired_into_its_skin() {
        // nothing wired: the mesh alone, its skin reading black, as a project from before has it
        let p = plan(&scene(&[MESH, "velo.output"], &[(0, 1, 0)]));
        assert_eq!((shaders(&p), &p.inputs, p.output), (vec!["mesh", "output"], &vec![vec![None], vec![Some(0)]], Some(1)));
        // a picture wired in is what its skin reads, and renders before it
        let mut s = scene(&["velo.noise", MESH, "velo.output"], &[(0, 1, 0), (1, 2, 0)]);
        let p = plan(&s);
        assert_eq!((shaders(&p), &p.inputs, p.output), (vec!["noise", "mesh", "output"], &vec![vec![], vec![Some(0)], vec![Some(1)]], Some(2)));
        // an off mesh shows nothing: its skin feeds it, it isn't what it shows
        s.nodes[1].off = true;
        assert_eq!(plan(&s).output, None);
    }

    #[test]
    fn off_effects_pass_their_first_input_through_to_readers_and_output() {
        let mut s = scene(&["velo.noise", "velo.blur", "velo.color", "velo.output"], &[(0, 1, 0), (1, 2, 0), (2, 3, 0)]);
        s.nodes[1].off = true;
        let p = plan(&s);
        assert_eq!(shaders(&p), ["noise", "color", "output"]);
        assert_eq!(p.inputs, vec![vec![], vec![Some(0)], vec![Some(1)]]);
        assert_eq!(p.output, Some(2));
        s.nodes[2].off = true;
        assert_eq!(plan(&s).inputs, vec![vec![], vec![Some(0)]], "a chain of off effects passes noise to output");
        s.nodes.pop();
        s.edges.pop();
        assert_eq!(plan(&s).output, Some(0), "the implicit output bypasses the off endpoint too");
        let mut s = scene(&["velo.noise", "velo.gradient", "velo.blend", "velo.output"], &[(0, 2, 0), (1, 2, 1), (2, 3, 0)]);
        s.nodes[2].off = true;
        assert_eq!(plan(&s).inputs[2], vec![Some(0)], "only the first input passes through");
    }

    #[test]
    fn off_generators_and_unwired_effects_give_black() {
        let mut s = scene(&["velo.noise", "velo.blur", "velo.output"], &[(0, 1, 0), (1, 2, 0)]);
        s.nodes[0].off = true;
        let p = plan(&s);
        assert_eq!(shaders(&p), ["blur", "output"]);
        assert_eq!(p.inputs, vec![vec![None], vec![Some(0)]]);
        s.nodes[1].off = true;
        assert_eq!(plan(&s).output, None);
        let mut s = scene(&["velo.noise", "velo.output"], &[(0, 1, 0)]);
        s.nodes[0].off = true;
        assert_eq!(plan(&s).output, None, "an off generator feeds black directly");
        s.nodes.pop();
        s.edges.clear();
        assert_eq!(plan(&s).output, None, "an off generator is also a black implicit output");
        let mut s = scene(&["velo.blur", "velo.output"], &[(0, 1, 0)]);
        s.nodes[0].off = true;
        assert_eq!(plan(&s).output, None, "an unwired off effect gives black");
    }

    #[test]
    fn loops_of_off_nodes_give_black() {
        let mut s = scene(&["velo.blur", "velo.feedback", "velo.output"], &[(0, 1, 0), (1, 0, 0), (0, 2, 0)]);
        s.nodes[0].off = true;
        s.nodes[1].off = true;
        let p = plan(&s);
        assert!(p.nodes.is_empty());
        assert_eq!(p.output, None);
    }

    #[test]
    fn of_several_outputs_the_last_one_wired_goes_out() {
        // noise into the first Output, gradient into the second, a third with nothing
        let s = scene(&["velo.noise", "velo.gradient", "velo.output", "velo.output", "velo.output"], &[(0, 2, 0), (1, 3, 0)]);
        let p = plan(&s);
        assert_eq!(shaders(&p), ["noise", "gradient", "output"], "only the Output going out grades");
        assert_eq!((&p.nodes[2].0, &p.inputs[2], p.output), (&s.nodes[3].id, &vec![Some(1)], Some(2)), "the gradient, through the last Output with a wire");
        assert_eq!(plan(&scene(&["velo.noise", "velo.output", "velo.output"], &[])).output, None, "Outputs with nothing in: nothing goes out");
    }

    #[test]
    fn a_media_list_streams_through_three_images_of_its_own() {
        let mut s = scene(&["velo.medialist", "velo.output"], &[(0, 1, 0)]);
        assert_eq!(shaders(&plan(&s)), ["medialist", "output"], "no files, no slots");
        s.nodes[0].media = Some("a.png\n\nb.mov\nc.png\nd.png\n".into());
        let p = plan(&s);
        let ids: Vec<&str> = p.nodes.iter().map(|(id, _)| id.as_str()).collect();
        let slot = |k| format!("{}#{k}", s.nodes[0].id);
        assert_eq!(ids, [s.nodes[0].id.as_str(), &slot(0), &slot(1), &slot(2), s.nodes[1].id.as_str()]);
        assert_eq!(shaders(&p), ["medialist", "image", "image", "image", "output"]);
        // four files, three slots: the list reads them, and goes out
        assert_eq!(p.inputs[0], [Some(1), Some(2), Some(3)]);
        assert_eq!((&p.inputs[4], p.output), (&vec![Some(0)], Some(4)));
        // off, its files aren't played either
        s.nodes[0].off = true;
        assert!(plan(&s).nodes.is_empty());
    }

    #[test]
    fn without_an_output_node_the_last_unread_node_goes_out() {
        // gradient → color, and noise on its own: both color and noise are unread; color ends a
        // chain, so it comes later upstream-first
        let p = plan(&scene(&["velo.color", "velo.gradient", "velo.noise"], &[(1, 0, 0)]));
        assert_eq!(p.output, Some(0));
        // upstream order, not scene order: blur reads noise, so blur is the end of the chain
        let p = plan(&scene(&["velo.blur", "velo.noise"], &[(1, 0, 0)]));
        assert_eq!(p.output, Some(0));
        assert_eq!(plan(&scene(&[], &[])), Plan { nodes: vec![], inputs: vec![], output: None, probes: vec![], under: vec![] });
    }

    #[test]
    fn a_loop_through_feedback_plans_and_sorts() {
        // noise → blend.a, blend → feedback → blend.b, feedback → output
        let s = scene(&["velo.noise", "velo.blend", "velo.feedback", "velo.output"], &[(0, 1, 0), (1, 2, 0), (2, 1, 1), (2, 3, 0)]);
        let p = plan(&s);
        assert_eq!((p.inputs[0].len(), &p.inputs[1][..2], &p.inputs[2]), (0, &[Some(0), Some(2)][..], &vec![Some(1)]));
        assert_eq!((&p.inputs[3], p.output), (&vec![Some(2)], Some(3)));
        let specs: Vec<NodeSpec> = p.inputs.iter().map(|i| NodeSpec { program: 0, inputs: i.clone() }).collect();
        assert_eq!(topological(&specs, &[false, false, true, false]).unwrap(), vec![0, 2, 1, 3]);
    }

    #[test]
    fn only_a_layer_on_top_reads_what_plays_under_it() {
        let mut s = scene(&[LIVE, "velo.kaleido", "velo.output"], &[(0, 1, 0), (1, 2, 0)]);
        s.nodes[0].media = Some(LIVE_PROGRAM.into());
        assert!(plan(&s).under.is_empty(), "a scene launched like the others has nothing under it");
        s.layer = Layer::Bottom;
        assert!(plan(&s).under.is_empty(), "nor has a layer underneath");
        s.layer = Layer::Top;
        assert_eq!(plan(&s).under, [0], "the Live node on the program");
        s.nodes[0].off = true;
        assert!(plan(&s).under.is_empty(), "switched off, it reads nothing");
        s.nodes[0].off = false;
        s.nodes[0].media = Some("ndi:Somewhere".into());
        assert!(plan(&s).under.is_empty(), "a feed from outside is no picture of ours");
    }

    #[test]
    fn the_compiler_builds_on_its_thread_with_the_values_set() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok().and_then(|a| pollster::block_on(a.request_device(&wgpu::DeviceDescriptor::default())).ok()) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let wave = crate::gpu::wave_texture(&device);
        let compiler = Compiler::new(device, queue, wave);
        let mut s = scene(&["velo.noise", "velo.kaleido", "velo.kaleido", "velo.output"], &[(0, 1, 0), (1, 2, 0), (2, 3, 0)]);
        s.nodes[0].params.insert("scale".into(), [9.0, 0.0, 0.0, 0.0]);
        compiler.build(7, s, [64, 36]);
        let (key, built) = compiler.built.recv_timeout(std::time::Duration::from_secs(20)).expect("a build");
        let built = built.expect("it compiles");
        assert_eq!(key, 7);
        assert_eq!(built.graph.programs.len(), 3, "two kaleidos share one program, then the Output's");
        assert_eq!((built.graph.runner.len(), built.graph.output), (4, Some(3)));
        assert!(built.graph.mods.iter().all(|m| !m.active()));
        // mappings come with the build
        let mut s = scene(&["velo.noise", "velo.kaleido"], &[(0, 1, 0)]);
        s.nodes[1].mappings.push(velo_core::Mapping { param: "zoom".into(), source: "kick".into(), amount: 0.5, attack_ms: 5.0, release_ms: 150.0, invert: false });
        compiler.build(8, s, [64, 36]);
        let built = compiler.built.recv_timeout(std::time::Duration::from_secs(20)).expect("a build").1.expect("it compiles");
        assert_eq!(built.graph.mods.iter().map(Modulator::active).collect::<Vec<_>>(), [false, true]);
        // so do the macros: their values, and the nodes they drive
        let mut s = scene(&["velo.noise", "velo.kaleido"], &[(0, 1, 0)]);
        s.macros[3].value = 0.5;
        s.macros[3].targets.push(velo_core::MacroTarget { node: s.nodes[0].id.clone(), param: "scale".into(), min: 1.0, max: 3.0 });
        compiler.build(9, s, [64, 36]);
        let built = compiler.built.recv_timeout(std::time::Duration::from_secs(20)).expect("a build").1.expect("it compiles");
        assert_eq!(built.graph.mods.iter().map(Modulator::active).collect::<Vec<_>>(), [true, false], "the noise is driven");
        assert_eq!(built.graph.macros.values(0.0)[3], 0.5);
        assert_eq!(built.graph.macros.routes.len(), 1);
    }

    #[test]
    fn a_mesh_node_draws_the_model_its_file_holds() {
        use crate::signal::{Kind, Probe};
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok().and_then(|a| pollster::block_on(a.request_device(&wgpu::DeviceDescriptor::default())).ok()) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let dir = std::env::temp_dir().join(format!("velo-mesh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("quad.obj");
        std::fs::write(&path, "v -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nf 1 2 3 4\n").unwrap();
        let compiler = Compiler::new(device.clone(), queue.clone(), crate::gpu::wave_texture(&device));
        let mut s = scene(&[velo_core::MESH, PROBE], &[(0, 1, 0)]);
        s.nodes[0].media = Some(path.to_string_lossy().into_owned());
        // the file loads on a thread of its own: the graph built before it's in draws nothing, the next one the model
        compiler.build(1, s.clone(), [64, 36]);
        // held, as the app holds its graph: the library drops a file nobody holds
        let _first = compiler.built.recv_timeout(std::time::Duration::from_secs(20)).expect("a build").1.expect("it compiles");
        std::thread::sleep(std::time::Duration::from_millis(500));
        compiler.build(2, s, [64, 36]);
        let mut built = compiler.built.recv_timeout(std::time::Duration::from_secs(20)).expect("a build").1.expect("it compiles");
        let g = &mut built.graph;
        let (signal, slot) = (g.probes[0].signal, g.signals[g.probes[0].signal].slot);
        let mut read = |position: [f32; 2], size: f32| {
            g.signals[signal].kind = Kind::Probe(Probe { measure: 0, position, size });
            let mut encoder = device.create_command_encoder(&Default::default());
            g.runner.render(&queue, &mut encoder, &g.programs, &[0.0, 1.0 / 60.0, 0.0, 16.0 / 9.0]);
            g.probes[0].encode(&queue, &mut encoder, &g.runner, position, size);
            queue.submit([encoder.finish()]);
            g.probes[0].read();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            g.signals[signal].feed(g.probes[0].color());
            let mut sources = [0.0f32; velo_core::SOURCES.len() + crate::signal::MAX_SIGNALS];
            crate::signal::run(&mut g.signals, &mut sources, 0.0, 1.0 / 60.0);
            sources[slot]
        };
        assert!(read([0.0, 0.0], 0.2) > 0.1, "the model lit in the middle");
        assert!(read([-0.95, 0.9], 0.05) < 0.01, "nothing in the corner");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_probe_reads_the_picture_wired_into_it_as_a_signal() {
        use crate::signal::{Kind, Probe};
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok().and_then(|a| pollster::block_on(a.request_device(&wgpu::DeviceDescriptor::default())).ok()) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let compiler = Compiler::new(device.clone(), queue.clone(), crate::gpu::wave_texture(&device));
        // white on the left half, black on the right
        let mut s = scene(&[SHADER, PROBE], &[(0, 1, 0)]);
        s.nodes[0].code = Some("/*{ \"name\": \"Half\", \"category\": \"generator\" }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(vec3f(step(uv.x, 0.5)), 1.0); }".into());
        assert_eq!(plan(&s).probes, [(s.nodes[1].id.clone(), Some(0))], "it reads the shader");
        compiler.build(1, s, [64, 36]);
        let mut built = compiler.built.recv_timeout(std::time::Duration::from_secs(20)).expect("a build").1.expect("it compiles");
        let g = &mut built.graph;
        assert_eq!(g.probes.len(), 1);
        let (signal, slot) = (g.probes[0].signal, g.signals[g.probes[0].signal].slot);
        let mut read = |measure: u8, position: [f32; 2], size: f32| {
            g.signals[signal].kind = Kind::Probe(Probe { measure, position, size });
            // a frame drawn and probed, read back after it's done; the signal pass on the next
            let mut encoder = device.create_command_encoder(&Default::default());
            g.runner.render(&queue, &mut encoder, &g.programs, &[0.0, 1.0 / 60.0, 0.0, 16.0 / 9.0]);
            g.probes[0].encode(&queue, &mut encoder, &g.runner, position, size);
            queue.submit([encoder.finish()]);
            g.probes[0].read();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            g.signals[signal].feed(g.probes[0].color());
            let mut sources = [0.0f32; velo_core::SOURCES.len() + crate::signal::MAX_SIGNALS];
            crate::signal::run(&mut g.signals, &mut sources, 0.0, 1.0 / 60.0);
            sources[slot]
        };
        let near = |a: f32, b: f32| (a - b).abs() < 0.02;
        assert!(near(read(0, [0.0, 0.0], 1.0), 0.5), "the whole picture: half white");
        assert!(near(read(0, [-0.5, 0.0], 0.4), 1.0), "the left: white");
        assert!(near(read(0, [0.5, 0.3], 0.4), 0.0), "the right: black");
        assert!(near(read(0, [-0.3, -0.6], 0.0), 1.0), "a point on the left");
        assert!(near(read(4, [0.5, 0.0], 0.4), 1.0), "opaque on the right too");
    }

    #[test]
    fn a_shader_node_runs_its_own_code_once_it_has_some() {
        let mut s = scene(&["velo.shader", "velo.kaleido"], &[(0, 1, 0)]);
        assert_eq!(shaders(&plan(&s)), ["kaleido"], "no code yet: black");
        s.nodes[0].code = Some(TEMPLATE.into());
        let p = plan(&s);
        assert_eq!(shaders(&p), ["own", "kaleido"]);
        assert_eq!(p.inputs, vec![vec![], vec![Some(0)]]);
        assert_eq!(p.output, Some(1));
    }

    #[test]
    fn the_template_and_the_fallback_compile_and_two_inputs_are_refused() {
        own(TEMPLATE).and_then(|node| node.validate().map(|_| ())).unwrap_or_else(|e| panic!("template: {e}"));
        own(BROKEN).and_then(|node| node.validate().map(|_| ())).unwrap_or_else(|e| panic!("fallback: {e}"));
        let two = TEMPLATE.replace("\"params\"", "\"inputs\": [{ \"id\": \"a\", \"type\": \"texture\" }, { \"id\": \"b\", \"type\": \"texture\" }],\n  \"params\"");
        assert!(own(&two).unwrap_err().message.contains("one input"));
    }
}
