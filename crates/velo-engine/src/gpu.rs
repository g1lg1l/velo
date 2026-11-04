//! Shader nodes on the GPU: a pipeline per shader ([`Program`]), and a graph run as a chain of
//! fullscreen passes ([`Runner`]): one texture per node, two for a feedback node (it reads the
//! one it wrote last frame), a black texture for unconnected inputs. A particle node keeps its
//! particles in a buffer of its own: two compute steps move them, then a sprite each is added up.
//! A Probe node's picture side reads a node's picture back as a color ([`Probe`]). A node with
//! a history ([`ShaderNode::history`]) keeps its first input's last [`HISTORY`] frames.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering::Relaxed};

use crate::shader::{GLOBAL_FLOATS, ShaderError, ShaderNode};

/// Every node renders in half floats: feedback trails and glow add up past 1 without banding.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The frames a node with a history keeps, at half the canvas each way: a second at 30 fps, 133 MB
/// at 1080p. ponytail: fixed; a length of its own (rebuilt when it changes) if more is wanted.
pub const HISTORY: u32 = 32;
/// Draws a picture into a history's frame, halving it.
const RECORD: &str = r#"/*{ "name": "Record", "category": "effect", "inputs": [{ "id": "src" }], "amount": "none" }*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return textureSample(src, smp, uv);
}
"#;

/// The particles a particle node's buffer holds (its `count` keeps fewer), 2 MB of them.
pub const PARTICLES: u32 = 65536;
/// Where a particle node finds its particles: past its textures (its sprite is binding 3).
const SIM_BINDING: u32 = 15;
/// Bytes a particle takes (particles.wgsl's `Particle`), after the emitter's 16.
const PARTICLE: u64 = 32;
const EMITTER: u64 = 16;

/// A shader node compiled for the GPU. Cloning shares the pipeline (wgpu handles are counted
/// references): a compiler caches one per shader and hands copies to every graph.
#[derive(Clone)]
pub struct Program {
    pub node: ShaderNode,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// A particle node's compute steps.
    sim: Option<Sim>,
}

/// Emit (one thread: how many are born this frame, and where in the ring), then step (a thread
/// a particle: born, moved, aged), over globals, parameters and the particle buffer.
#[derive(Clone)]
struct Sim {
    emit: wgpu::ComputePipeline,
    step: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
}

impl Program {
    /// Validates with naga first, so errors come back at the author's lines, then builds the pipeline.
    /// What wgpu still rejects (a texture sampled that can't be) is this shader's error too, never
    /// a panic.
    pub fn new(device: &wgpu::Device, node: ShaderNode) -> Result<Self, ShaderError> {
        node.validate()?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let program = Self::build(device, node);
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(ShaderError { line: None, message: crate::gpu_error(&e) }),
            None => Ok(program),
        }
    }

    fn build(device: &wgpu::Device, node: ShaderNode) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&node.name), source: wgpu::ShaderSource::Wgsl(node.wgsl.as_str().into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty, count: None };
        let uniform = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None };
        let texture = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let mut entries = vec![entry(0, uniform), entry(1, uniform), entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering))];
        entries.extend((0..textures(&node) as u32).map(|i| {
            let ty = if node.audio && i as usize == textures(&node) - 1 {
                wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }
            } else {
                texture
            };
            entry(3 + i, ty)
        }));
        if node.history {
            let at = 3 + textures(&node) as u32;
            entries.push(entry(at, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false }));
            entries.push(entry(at + 1, uniform));
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(&node.name), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(&node.name), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        // a particle node draws a quad a particle, read from its buffer: position and velocity, then age, life, seed, size
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let sprites = [Some(wgpu::VertexBufferLayout { array_stride: PARTICLE, step_mode: wgpu::VertexStepMode::Instance, attributes: &attributes })];
        // their light adds up; their alpha covers
        let add = |dst_factor| wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor, operation: wgpu::BlendOperation::Add };
        let blend = node.particles.then(|| wgpu::BlendState { color: add(wgpu::BlendFactor::One), alpha: add(wgpu::BlendFactor::OneMinusSrcAlpha) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&node.name),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("velo_vs"), compilation_options: Default::default(), buffers: if node.particles { &sprites } else { &[] } },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("velo_fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState { topology: if node.particles { wgpu::PrimitiveTopology::TriangleStrip } else { wgpu::PrimitiveTopology::TriangleList }, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sim = node.particles.then(|| {
            let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
            let storage = wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(&node.name), entries: &[entry(0, uniform), entry(1, uniform), entry(SIM_BINDING, storage)] });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(&node.name), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let compute = |entry_point| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(&node.name),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    cache: None,
                })
            };
            Sim { emit: compute("velo_emit"), step: compute("velo_step"), layout }
        });
        Self { node, pipeline, layout, sim }
    }
}

/// Texture bindings a node takes: inputs, then `prev`, then `wave` when requested.
fn textures(node: &ShaderNode) -> usize {
    node.inputs.len() + usize::from(node.feedback) + usize::from(node.audio)
}

/// A node that reads only some of its inputs, as its values say (by parameter index): what lets
/// a frame skip a picture nobody sees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lazy {
    /// A Switch reads the inputs it shows.
    Switch { value: usize, count: usize, fade: usize },
    /// A Blend at no opacity reads its first input only.
    Blend { opacity: usize },
}

impl Lazy {
    /// Whether it reads input `i` with these values.
    pub fn reads(self, value: impl Fn(usize) -> f32, i: usize) -> bool {
        match self {
            Lazy::Switch { value: v, count, fade } => switch_shows(value(v), value(count), value(fade)).contains(&Some(i)),
            Lazy::Blend { opacity } => i == 0 || value(opacity) > 0.0,
        }
    }
}

/// How many inputs a Switch has.
pub const SWITCH_INPUTS: usize = 16;

/// The inputs a Switch shows (switch.wgsl): the one `value` picks, and the next one while it
/// fades over. A little either side of each edge counts as both, so the GPU's own rounding
/// never shows one that didn't render.
pub fn switch_shows(value: f32, count: f32, fade: f32) -> [Option<usize>; 2] {
    const EDGE: f32 = 0.01;
    let count = (count.round() as i32).clamp(2, SWITCH_INPUTS as i32);
    let x = value.clamp(0.0, 1.0) * (count - 1) as f32;
    let first = (x.floor() as i32).min(count - 2);
    let t = x - first as f32;
    // how far over to the next one it is (switch.wgsl's k)
    let k = |t: f32| {
        if fade > 0.0 {
            let s = ((t - 0.5 + fade * 0.5) / fade).clamp(0.0, 1.0);
            s * s * (3.0 - 2.0 * s)
        } else {
            f32::from(u8::from(t >= 0.5))
        }
    };
    [(k(t - EDGE) < 1.0).then_some(first as usize), (k(t + EDGE) > 0.0).then_some(first as usize + 1)]
}

/// One node of a graph to run: which program, and which node feeds each of its inputs.
pub struct NodeSpec {
    pub program: usize,
    pub inputs: Vec<Option<usize>>,
}

struct RunNode {
    program: usize,
    params: wgpu::Buffer,
    /// One texture, or two for feedback (written alternately).
    targets: Vec<wgpu::TextureView>,
    /// Per frame parity: the textures it reads change when an input (or it) has feedback.
    groups: [wgpu::BindGroup; 2],
    /// A particle node's particles (zeroed, so none, in a new graph) and their compute binding.
    sim: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    past: Option<Past>,
}

/// A node's history: its first input drawn into a frame of a ring each frame, before it runs.
struct Past {
    /// A view a frame, drawn into in turn.
    frames: Vec<wgpu::TextureView>,
    /// Which frame is this one's (and how many), for `past_frame`.
    head: wgpu::Buffer,
    /// The record pass reading the first input, by parity.
    groups: [wgpu::BindGroup; 2],
}

/// A graph, ready to render frames: textures allocated, bind groups built. Nothing in
/// [`Runner::render`] allocates or compiles.
/// The most nodes of a graph whose cost is measured.
pub const MAX_TIMED: usize = 64;

pub struct Runner {
    size: [u32; 2],
    globals: wgpu::Buffer,
    nodes: Vec<RunNode>,
    /// Which node feeds each input of each node.
    inputs: Vec<Vec<Option<usize>>>,
    order: Vec<usize>,
    frame: u64,
    /// What draws a history's frames, when a node keeps one.
    record: Option<Program>,
}

impl Runner {
    /// `nodes` index `programs`; inputs point at other nodes, and a node in `media` reads that
    /// texture as its first input instead (an Image node's file). Fails on a bad index, or on
    /// a cycle with no feedback node on it.
    pub fn new(device: &wgpu::Device, programs: &[Program], nodes: &[NodeSpec], media: &[(usize, wgpu::TextureView)], size: [u32; 2], wave: &wgpu::TextureView) -> Result<Self, String> {
        let feedback: Vec<bool> = nodes.iter().map(|n| programs.get(n.program).is_some_and(|p| p.node.feedback)).collect();
        let order = topological(nodes, &feedback)?;
        let buffer = |label, size| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let globals = buffer("globals", (GLOBAL_FLOATS * 4) as u64);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let black = texture(device, [1, 1]);
        let record = match nodes.iter().any(|n| programs.get(n.program).is_some_and(|p| p.node.history)) {
            true => Some(ShaderNode::parse(RECORD).and_then(|node| Program::new(device, node)).map_err(|e| format!("history: {e}"))?),
            false => None,
        };
        let no_params = buffer("params", 16);
        let mut targets = Vec::with_capacity(nodes.len());
        for spec in nodes {
            let program = programs.get(spec.program).ok_or_else(|| format!("no program {}", spec.program))?;
            targets.push((0..1 + usize::from(program.node.feedback)).map(|_| texture(device, size)).collect::<Vec<_>>());
        }
        // what node `n` shows after a frame of parity `p`
        let shown = |n: usize, p: usize| &targets[n][p % targets[n].len()];
        let mut run = Vec::with_capacity(nodes.len());
        for (n, spec) in nodes.iter().enumerate() {
            let program = &programs[spec.program];
            let params = buffer("params", (program.node.params.len().max(1) * 16) as u64);
            let first = |p: usize| spec.inputs.first().copied().flatten().map_or(&black, |from| shown(from, p));
            let past = record.as_ref().filter(|_| program.node.history).map(|record| {
                let ring = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("history"),
                    size: wgpu::Extent3d { width: (size[0] / 2).max(1), height: (size[1] / 2).max(1), depth_or_array_layers: HISTORY },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let frame = |i| ring.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: i, array_layer_count: Some(1), ..Default::default() });
                let all = ring.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
                let group = |p: usize| {
                    let entries = [
                        wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: no_params.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(first(p)) },
                    ];
                    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("history"), layout: &record.layout, entries: &entries })
                };
                (Past { frames: (0..HISTORY).map(frame).collect(), head: buffer("history", 16), groups: [group(0), group(1)] }, all)
            });
            let group = |p: usize| {
                let file = media.iter().find(|m| m.0 == n).map(|m| &m.1);
                let mut views: Vec<&wgpu::TextureView> = (0..program.node.inputs.len())
                    .map(|i| match (i, file) {
                        (0, Some(file)) => file,
                        _ => spec.inputs.get(i).copied().flatten().map_or(&black, |from| shown(from, p)),
                    })
                    .collect();
                if program.node.feedback {
                    views.push(shown(n, p + 1));
                }
                if program.node.audio {
                    views.push(wave);
                }
                let mut entries = vec![
                    wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                ];
                let at = 3 + views.len() as u32;
                entries.extend(views.into_iter().enumerate().map(|(i, v)| wgpu::BindGroupEntry { binding: 3 + i as u32, resource: wgpu::BindingResource::TextureView(v) }));
                if let Some((past, all)) = &past {
                    entries.push(wgpu::BindGroupEntry { binding: at, resource: wgpu::BindingResource::TextureView(all) });
                    entries.push(wgpu::BindGroupEntry { binding: at + 1, resource: past.head.as_entire_binding() });
                }
                device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(&program.node.name), layout: &program.layout, entries: &entries })
            };
            let groups = [group(0), group(1)];
            let sim = program.sim.as_ref().map(|sim| {
                let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX;
                let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("particles"), size: EMITTER + u64::from(PARTICLES) * PARTICLE, usage, mapped_at_creation: false });
                let entries = [
                    wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: SIM_BINDING, resource: buffer.as_entire_binding() },
                ];
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("particles"), layout: &sim.layout, entries: &entries });
                (buffer, group)
            });
            run.push(RunNode { program: spec.program, params, targets: Vec::new(), groups, sim, past: past.map(|p| p.0) });
        }
        for (node, views) in run.iter_mut().zip(targets) {
            node.targets = views;
        }
        Ok(Self { size, globals, nodes: run, inputs: nodes.iter().map(|n| n.inputs.clone()).collect(), order, frame: 0, record })
    }

    /// Sets a node's parameter values (one vec4 per parameter, in header order).
    pub fn set_params(&self, queue: &wgpu::Queue, programs: &[Program], node: usize, values: &[[f32; 4]]) {
        let n = &self.nodes[node];
        queue.write_buffer(&n.params, 0, &programs[n.program].node.param_bytes(values));
    }

    /// Writes a node's parameter buffer as it is (what [`crate::Modulator`] computed).
    pub fn write_params(&self, queue: &wgpu::Queue, node: usize, bytes: &[u8]) {
        queue.write_buffer(&self.nodes[node].params, 0, bytes);
    }

    /// Records one frame: every node, upstream first. `globals` are the `g` values after
    /// `resolution` (time, delta, frame…, see [`crate::GLOBALS`]); the resolution is filled in here.
    pub fn render(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, programs: &[Program], globals: &[f32]) {
        self.render_timed(queue, encoder, programs, globals, None, None);
    }

    /// [`Self::render`], each of the first [`MAX_TIMED`] nodes' pass between two timestamps of
    /// `timing` (node n's at 2n and 2n + 1): what it cost the GPU. With `awake`, only the nodes
    /// it marks render; the rest keep their last picture.
    pub fn render_timed(&mut self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, programs: &[Program], globals: &[f32], timing: Option<&wgpu::QuerySet>, awake: Option<&[bool]>) {
        let mut g = [0.0f32; GLOBAL_FLOATS];
        g[0] = self.size[0] as f32;
        g[1] = self.size[1] as f32;
        for (slot, v) in g[2..].iter_mut().zip(globals) {
            *slot = *v;
        }
        let mut bytes = [0u8; GLOBAL_FLOATS * 4];
        for (chunk, v) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(g) {
            *chunk = v.to_ne_bytes();
        }
        queue.write_buffer(&self.globals, 0, &bytes);
        let parity = (self.frame % 2) as usize;
        for &n in &self.order {
            if awake.is_some_and(|a| !a[n]) {
                continue;
            }
            let node = &self.nodes[n];
            let program = &programs[node.program];
            let target = &node.targets[parity % node.targets.len()];
            // a history: the first input into this frame's place in the ring, before the node reads it
            if let (Some(past), Some(record)) = (&node.past, &self.record) {
                let head = (self.frame % u64::from(HISTORY)) as usize;
                let mut at = [0u8; 16];
                for (chunk, v) in at.as_chunks_mut::<4>().0.iter_mut().zip([head as f32, HISTORY as f32]) {
                    *chunk = v.to_ne_bytes();
                }
                queue.write_buffer(&past.head, 0, &at);
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("history"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &past.frames[head], resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store }, depth_slice: None })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&record.pipeline);
                pass.set_bind_group(0, &past.groups[parity], &[]);
                pass.draw(0..3, 0..1);
            }
            if let (Some(sim), Some((_, group))) = (&program.sim, &node.sim) {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_bind_group(0, group, &[]);
                pass.set_pipeline(&sim.emit);
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(&sim.step);
                pass.dispatch_workgroups(PARTICLES.div_ceil(64), 1, 1);
            }
            // particles add up over nothing
            let clear = if node.sim.is_some() { wgpu::Color::TRANSPARENT } else { wgpu::Color::BLACK };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&program.node.name),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store }, depth_slice: None })],
                depth_stencil_attachment: None,
                timestamp_writes: timing.filter(|_| n < MAX_TIMED).map(|query_set| wgpu::RenderPassTimestampWrites { query_set, beginning_of_pass_write_index: Some(2 * n as u32), end_of_pass_write_index: Some(2 * n as u32 + 1) }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&program.pipeline);
            pass.set_bind_group(0, &node.groups[parity], &[]);
            match &node.sim {
                Some((buffer, _)) => {
                    pass.set_vertex_buffer(0, buffer.slice(EMITTER..));
                    pass.draw(0..4, 0..PARTICLES);
                }
                None => pass.draw(0..3, 0..1),
            }
        }
        self.frame += 1;
    }

    /// The nodes a frame needs: those `roots` read through, where `reads(n, i)` says whether
    /// node n reads its input i this frame.
    pub fn needed(&self, roots: impl IntoIterator<Item = usize>, reads: impl Fn(usize, usize) -> bool) -> Vec<bool> {
        let mut needed = vec![false; self.nodes.len()];
        let mut todo: Vec<usize> = roots.into_iter().collect();
        while let Some(n) = todo.pop() {
            if n >= needed.len() || std::mem::replace(&mut needed[n], true) {
                continue;
            }
            todo.extend(self.inputs[n].iter().enumerate().filter(|(i, _)| reads(n, *i)).filter_map(|(_, from)| *from));
        }
        needed
    }

    /// What `node` drew in the last frame rendered.
    pub fn output(&self, node: usize) -> &wgpu::TextureView {
        let n = &self.nodes[node];
        &n.targets[((self.frame + 1) % 2) as usize % n.targets.len()]
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// How many nodes it runs.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// A Probe's cells across and down: a row is 256 bytes, as a copy into a buffer wants.
const PROBE_CELLS: [u32; 2] = [32, 16];
const FREE: u8 = 0;
const COPIED: u8 = 1;
const PENDING: u8 = 2;

/// A Probe node's picture side: its region of a node's picture drawn into [`PROBE_CELLS`] cells
/// (probe.wgsl), copied out and read back a frame or two later as the region's average color.
pub struct Probe {
    /// The engine node it reads.
    pub node: usize,
    /// Its signal node, an index into the graph's signals.
    pub signal: usize,
    program: Program,
    params: wgpu::Buffer,
    target: wgpu::Texture,
    /// One per texture the node it reads draws into (two for a feedback node).
    groups: Vec<wgpu::BindGroup>,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// The average color (premultiplied) as f32 bits.
    color: Arc<[AtomicU32; 4]>,
}

impl Probe {
    /// Reading `node` of `runner` for the signal node `signal`, with probe.wgsl's `program`.
    pub fn new(device: &wgpu::Device, program: &Program, runner: &Runner, node: usize, signal: usize) -> Self {
        let buffer = |size: u64, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some("probe"), size, usage, mapped_at_creation: false });
        let params = buffer((program.node.params.len().max(1) * 16) as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        // never read, but the layout has it
        let globals = buffer((GLOBAL_FLOATS * 4) as u64, wgpu::BufferUsages::UNIFORM);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let [w, h] = PROBE_CELLS;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("probe"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let groups = runner.nodes[node]
            .targets
            .iter()
            .map(|view| {
                let entries = [
                    wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(view) },
                ];
                device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("probe"), layout: &program.layout, entries: &entries })
            })
            .collect();
        let readback = buffer(u64::from(w * h * 8), wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        Self { node, signal, program: program.clone(), params, target, groups, readback, state: Arc::new(AtomicU8::new(FREE)), color: Arc::new(Default::default()) }
    }

    /// Draws its region (`size` of the picture each way round `position`, -1 to 1) of what the
    /// node showed this frame and copies it out, unless the last copy is still being read.
    pub fn encode(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, runner: &Runner, position: [f32; 2], size: f32) {
        if self.state.load(Relaxed) != FREE {
            return;
        }
        let shown = runner.output(self.node);
        let Some(group) = runner.nodes[self.node].targets.iter().position(|t| std::ptr::eq(t, shown)).and_then(|i| self.groups.get(i)) else { return };
        queue.write_buffer(&self.params, 0, &self.program.node.param_bytes(&[[position[0], position[1], 0.0, 0.0], [size, 0.0, 0.0, 0.0]]));
        let view = self.target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store }, depth_slice: None })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.program.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        let [w, h] = PROBE_CELLS;
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 8), rows_per_image: None };
        encoder.copy_texture_to_buffer(self.target.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &self.readback, layout }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        self.state.store(COPIED, Relaxed);
    }

    /// After the submit: what was copied read back, its cells averaged into the color.
    pub fn read(&self) {
        if self.state.load(Relaxed) != COPIED {
            return;
        }
        self.state.store(PENDING, Relaxed);
        let (buffer, state, color) = (self.readback.clone(), self.state.clone(), self.color.clone());
        self.readback.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_ok() {
                if let Ok(view) = buffer.slice(..).get_mapped_range() {
                    let mut sum = [0.0f32; 4];
                    let texels = view.as_chunks::<8>().0;
                    for t in texels {
                        for (s, c) in sum.iter_mut().zip(t.as_chunks::<2>().0) {
                            *s += half(u16::from_ne_bytes(*c));
                        }
                    }
                    for (slot, s) in color.iter().zip(sum) {
                        slot.store((s / texels.len().max(1) as f32).to_bits(), Relaxed);
                    }
                }
                buffer.unmap();
            }
            state.store(FREE, Relaxed);
        });
    }

    /// The region's average color (premultiplied: see-through counts as dark), as last read.
    pub fn color(&self) -> [f32; 4] {
        std::array::from_fn(|i| f32::from_bits(self.color[i].load(Relaxed)))
    }
}

/// A half float's value.
pub(crate) fn half(h: u16) -> f32 {
    let (e, m) = (i32::from((h >> 10) & 31), f32::from(h & 1023));
    let v = if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
    if h & 0x8000 != 0 { -v } else { v }
}

pub(crate) fn wave_texture(device: &wgpu::Device) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("audio wave"),
            size: wgpu::Extent3d { width: crate::runtime::WAVE_COLUMNS as u32, height: crate::runtime::WAVE_ROWS as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

pub(crate) fn texture(device: &wgpu::Device, [width, height]: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// Upstream first. A loop is only allowed through a feedback node (`feedback[n]`): it goes
/// before its inputs and reads what they drew last frame, as it reads its own `prev`; without
/// a loop it waits for its inputs like any node. An error names a node on a cycle.
pub(crate) fn topological(nodes: &[NodeSpec], feedback: &[bool]) -> Result<Vec<usize>, String> {
    // Kahn's algorithm, lowest index first; when it stalls on a loop, a feedback node on it goes
    let mut waiting = vec![0usize; nodes.len()];
    let mut consumers = vec![Vec::new(); nodes.len()];
    for (n, spec) in nodes.iter().enumerate() {
        for &from in spec.inputs.iter().flatten() {
            if from >= nodes.len() {
                return Err(format!("node {n} reads a node {from} that doesn't exist"));
            }
            waiting[n] += 1;
            consumers[from].push(n);
        }
    }
    let (mut order, mut placed) = (Vec::with_capacity(nodes.len()), vec![false; nodes.len()]);
    let mut ready: std::collections::VecDeque<usize> = (0..nodes.len()).filter(|&n| waiting[n] == 0).collect();
    while order.len() < nodes.len() {
        let Some(n) = ready.pop_front().or_else(|| (0..nodes.len()).find(|&n| !placed[n] && feedback.get(n).copied().unwrap_or(false))) else {
            let n = (0..nodes.len()).find(|&n| !placed[n]).unwrap_or(0);
            return Err(format!("node {n} is on a cycle"));
        };
        if placed[n] {
            continue;
        }
        placed[n] = true;
        order.push(n);
        for &c in &consumers[n] {
            waiting[c] = waiting[c].saturating_sub(1);
            if waiting[c] == 0 && !placed[c] {
                ready.push_back(c);
            }
        }
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BUILTINS;

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let required_features = adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features, ..Default::default() })).ok()
    }

    fn program(device: &wgpu::Device, id: &str) -> Program {
        let source = BUILTINS.iter().find(|b| b.0 == id).unwrap().1;
        Program::new(device, ShaderNode::parse(source).unwrap()).unwrap_or_else(|e| panic!("{id}: {e}"))
    }

    /// Reads a node's output back as raw half floats.
    fn read(device: &wgpu::Device, queue: &wgpu::Queue, runner: &Runner, node: usize) -> Vec<u16> {
        let [w, h] = runner.size();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(w * h * 8), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = device.create_command_encoder(&Default::default());
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 8), rows_per_image: None };
        encoder.copy_texture_to_buffer(runner.output(node).texture().as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buffer, layout }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = buffer.slice(..).get_mapped_range().expect("mapped");
        data.as_chunks::<2>().0.iter().map(|c| u16::from_ne_bytes(*c)).collect()
    }

    #[test]
    fn audio_binds_with_inputs_and_feedback_and_waveform_modes_render() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let wave = wave_texture(&device);
        let shared = crate::SharedWave::default();
        for _ in 0..crate::WAVE_COLUMNS {
            shared.push_column([0.25, 0.8, 0.4, 0.2]);
        }
        shared.push_samples(&[-0.5; crate::WAVE_COLUMNS]);
        let mut pixels = [[0.0; 4]; crate::WAVE_COLUMNS * crate::runtime::WAVE_ROWS];
        shared.load(&mut pixels);
        let bytes: Vec<u8> = pixels.iter().flatten().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_texture(
            wave.texture().as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(crate::WAVE_COLUMNS as u32 * 16), rows_per_image: None },
            wgpu::Extent3d { width: crate::WAVE_COLUMNS as u32, height: crate::runtime::WAVE_ROWS as u32, depth_or_array_layers: 1 },
        );
        let audio = ShaderNode::parse(
            r#"/*{ "name": "Audio", "inputs": [{ "id": "src" }], "feedback": true, "audio": true }*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return vec4f(textureLoad(wave, vec2i(511, 0), 0).r + textureLoad(prev, vec2i(px), 0).r,
                 textureLoad(src, vec2i(px), 0).g, textureLoad(wave, vec2i(511, 1), 0).r + 1.0, 1.0);
}"#,
        )
        .unwrap();
        let programs = [program(&device, "gradient"), Program::new(&device, audio).unwrap(), program(&device, "waveform")];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![Some(0)] }, NodeSpec { program: 2, inputs: vec![] }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [128, 128], &wave).unwrap();
        let green = programs[0].node.values(|id| id.starts_with("color").then_some([0.0, 1.0, 0.0, 1.0]));
        runner.set_params(&queue, &programs, 0, &green);
        runner.set_params(&queue, &programs, 1, &[]);
        for (mode, red) in [0x3400, 0x3800, 0x3A00, 0x3C00].into_iter().enumerate() {
            // no glow: a ring's halo would reach every pixel
            let values = programs[2].node.values(|id| match id {
                "mode" => Some([mode as f32, 0.0, 0.0, 0.0]),
                "glow" => Some([0.0; 4]),
                _ => None,
            });
            runner.set_params(&queue, &programs, 2, &values);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, mode as f32, 1.0]);
            queue.submit([encoder.finish()]);
            assert_eq!(&read(&device, &queue, &runner, 1)[..4], &[red, 0x3C00, 0x3800, 0x3C00]);
            let pixels = read(&device, &queue, &runner, 2);
            let texels = pixels.as_chunks::<4>().0;
            let lit = texels.iter().filter(|p| p[3] != 0).count();
            assert!(lit > 0 && lit < texels.len(), "mode {mode}: visible waveform with transparent space");
            assert!(texels.iter().filter(|p| p[3] == 0).all(|p| *p == [0; 4]));
        }
    }

    #[test]
    fn isf_shaders_render_on_the_gpu() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        // ISF rings into an ISF effect, as two nodes
        let isf = |code: &str| Program::new(&device, crate::isf::parse(code).unwrap()).unwrap_or_else(|e| panic!("{e}"));
        let programs = [isf(crate::isf::tests::RINGS), isf(crate::isf::tests::SHIFT)];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![Some(0)] }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [128, 72], &wave_texture(&device)).unwrap();
        for n in 0..nodes.len() {
            runner.set_params(&queue, &programs, n, &[]);
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        runner.render(&queue, &mut encoder, &programs, &[0.3, 1.0 / 60.0, 1.0, 16.0 / 9.0]);
        queue.submit([encoder.finish()]);
        let pixels = read(&device, &queue, &runner, 1);
        let texels = pixels.as_chunks::<4>().0;
        let lit = texels.iter().filter(|p| p[0] != 0).count();
        assert!(lit > texels.len() / 20 && lit < texels.len(), "rings, not a flat picture ({lit} of {} lit)", texels.len());
    }

    #[test]
    fn a_chain_with_feedback_renders_on_the_gpu() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let programs: Vec<Program> = ["gradient", "kaleido", "color", "feedback", "shapes", "blend"].iter().map(|id| program(&device, id)).collect();
        // gradient → kaleido → color → feedback; shapes over it with blend
        let nodes = [
            NodeSpec { program: 0, inputs: vec![] },
            NodeSpec { program: 1, inputs: vec![Some(0)] },
            NodeSpec { program: 2, inputs: vec![Some(1)] },
            NodeSpec { program: 3, inputs: vec![Some(2)] },
            NodeSpec { program: 4, inputs: vec![] },
            NodeSpec { program: 5, inputs: vec![Some(3), Some(4)] },
        ];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [128, 72], &wave_texture(&device)).unwrap();
        for n in 0..nodes.len() {
            runner.set_params(&queue, &programs, n, &[]);
        }
        for frame in 0..3 {
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[frame as f32 / 60.0, 1.0 / 60.0, frame as f32, 16.0 / 9.0]);
            queue.submit([encoder.finish()]);
        }
        let pixels = read(&device, &queue, &runner, 5);
        let texels = pixels.as_chunks::<4>().0;
        let lit = texels.iter().filter(|p| p[..3].iter().any(|&c| c != 0)).count();
        let distinct: std::collections::BTreeSet<_> = texels.iter().map(|p| p[0]).collect();
        assert!(lit > pixels.len() / 8, "most of the picture is lit ({lit} of {} pixels)", pixels.len() / 4);
        assert!(distinct.len() > 20, "the picture has structure, not one flat color ({} reds)", distinct.len());
    }

    #[test]
    fn an_image_node_shows_its_file_fitted_and_plays_it() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        // two frames of 2×1 pixels: red | green, then blue | blue; half a second each
        let px = |c: [[u8; 3]; 2]| c.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect::<Vec<u8>>();
        let frames = vec![px([[255, 0, 0], [0, 255, 0]]), px([[0, 0, 255], [0, 0, 255]])];
        let media = std::sync::Arc::new(crate::media::Media { size: [2, 1], frames, ends: vec![0.5, 1.0], movie: None, video: None });
        let programs = [program(&device, "image")];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }];
        let mut playing = crate::media::Playing::new(&device, &queue, 0, Some(1), media);
        let views = [(0, playing.texture.create_view(&Default::default()))];
        // a square canvas (a row as wide as a copy wants): a 2:1 picture fitted leaves bands above and below, filled it doesn't
        let mut runner = Runner::new(&device, &programs, &nodes, &views, [32, 32], &wave_texture(&device)).unwrap();
        let frame = |runner: &mut Runner, fit: f32| {
            runner.set_params(&queue, &programs, 0, &[[fit, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 1.0]);
            queue.submit([encoder.finish()]);
            let pixels = read(&device, &queue, runner, 0);
            let texels: Vec<[u16; 4]> = pixels.as_chunks::<4>().0.to_vec();
            texels
        };
        let at = |t: &[[u16; 4]], x: usize, y: usize| t[y * 32 + x];
        let bright = 0x3800; // 0.5 as a half float
        let fitted = frame(&mut runner, 1.0);
        assert_eq!(at(&fitted, 1, 0)[3], 0, "a band above: nothing there");
        assert!(at(&fitted, 0, 16)[0] > bright && at(&fitted, 31, 16)[1] > bright, "red at the left, green at the right");
        let filled = frame(&mut runner, 0.0);
        assert!(at(&filled, 1, 0)[3] > bright, "filled: no band");
        // 0.6 s later the second frame is due
        playing.step(&queue, 0.6, 1.0, (0.0, 1.0));
        let next = frame(&mut runner, 1.0);
        assert!(at(&next, 0, 16)[2] > bright && at(&next, 0, 16)[0] < bright, "blue now");
    }

    #[test]
    fn text_breaks_spaces_and_draws_its_glyphs_by_its_parameters() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        const W: usize = 192;
        const H: usize = 108;
        // 0.5 as a half float; a unit (a line at size 0.3) in pixels
        const HALF: u16 = 0x3800;
        const UNIT: f32 = 0.3 * H as f32;
        let programs = [program(&device, "text")];
        // `words` with these parameters over the defaults, at size 0.3: their pixels
        let draw = |words: &str, set: &[(&str, [f32; 4])]| -> Vec<[u16; 4]> {
            let media = std::sync::Arc::new(crate::media::Media::still(crate::text::set(words, None).unwrap()).unwrap());
            let playing = crate::media::Playing::new(&device, &queue, 0, None, media);
            let views = [(0, playing.texture.create_view(&Default::default()))];
            let mut runner = Runner::new(&device, &programs, &[NodeSpec { program: 0, inputs: vec![] }], &views, [W as u32, H as u32], &wave_texture(&device)).unwrap();
            let values = programs[0].node.values(|id| set.iter().find(|s| s.0 == id).map(|s| s.1).or((id == "size").then_some([0.3, 0.0, 0.0, 0.0])));
            runner.set_params(&queue, &programs, 0, &values);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, W as f32 / H as f32]);
            queue.submit([encoder.finish()]);
            read(&device, &queue, &runner, 0).as_chunks::<4>().0.to_vec()
        };
        // the box round what's more than half covered in these rows: x0, y0, x1, y1
        let ink = |t: &[[u16; 4]], rows: std::ops::Range<usize>| {
            let mut b: Option<[usize; 4]> = None;
            for (x, y) in rows.flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| t[y * W + x][3] > HALF) {
                let e = b.get_or_insert([x, y, x, y]);
                *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
            }
            b
        };
        let (width, height) = (|b: [usize; 4]| b[2] - b[0], |b: [usize; 4]| b[3] - b[1]);
        let middle = |b: [usize; 4]| ((b[0] + b[2]) as f32 / 2.0, (b[1] + b[3]) as f32 / 2.0);
        let plain = draw("VELO", &[]);
        let b = ink(&plain, 0..H).expect("VELO shows");
        let (mx, my) = middle(b);
        assert!((mx - W as f32 / 2.0).abs() < 3.0 && (my - H as f32 / 2.0).abs() < 3.0, "in the middle: {b:?}");
        assert_eq!(plain[0], [0; 4], "on clear");
        // tracking spreads the letters about the middle: three gaps of 0.3 units
        let tracked = ink(&draw("VELO", &[("tracking", [0.3, 0.0, 0.0, 0.0])]), 0..H).unwrap();
        let wider = width(tracked) as f32 - width(b) as f32;
        assert!((wider - 0.9 * UNIT).abs() < 4.0 && (middle(tracked).0 - mx).abs() < 3.0, "{wider} px wider");
        // reveal shows the first letters, then none
        let half = ink(&draw("VELO", &[("reveal", [0.5, 0.0, 0.0, 0.0])]), 0..H).unwrap();
        assert!(half[0] == b[0] && half[2] < W / 2 + 2, "V and E: {half:?} of {b:?}");
        assert!(ink(&draw("VELO", &[("reveal", [0.0; 4])]), 0..H).is_none());
        // wrap breaks the words into lines under it; line height spaces them
        let wrapped = ink(&draw("VELO VELO", &[("wrap", [0.4, 0.0, 0.0, 0.0])]), 0..H).unwrap();
        assert!(width(wrapped) <= width(b) + 1 && height(wrapped) as f32 > height(b) as f32 + 0.9 * UNIT, "two lines: {wrapped:?}");
        let (near, apart) = (ink(&draw("VELO\nV", &[]), 0..H).unwrap(), ink(&draw("VELO\nV", &[("line_height", [2.0, 0.0, 0.0, 0.0])]), 0..H).unwrap());
        assert!((height(apart) as f32 - height(near) as f32 - UNIT).abs() < 3.0, "{near:?} {apart:?}");
        // align: a short second line at the left, or at the right, of the long one
        for (align, side) in [(0.0, 0), (2.0, 2)] {
            let t = draw("VELO\nV", &[("align", [align, 0.0, 0.0, 0.0])]);
            let (top, bottom) = (ink(&t, 0..H / 2).unwrap(), ink(&t, H / 2..H).unwrap());
            assert!(top[side].abs_diff(bottom[side]) <= 3 && width(bottom) < width(top) / 2, "align {align}: {top:?} {bottom:?}");
        }
        // an outline in red round white letters, none at 0
        let red = |t: &[[u16; 4]]| t.iter().filter(|p| p[0] > HALF && p[1] < 0x3000).count();
        assert!(red(&draw("VELO", &[("outline", [1.0, 0.0, 0.0, 0.0]), ("outline_color", [1.0, 0.0, 0.0, 1.0])])) > 50);
        assert_eq!(red(&plain), 0);
        // a box behind: blue just past the letters, inside its padding
        let boxed = draw("VELO", &[("box", [1.0, 0.0, 0.0, 0.0]), ("box_color", [0.0, 0.0, 1.0, 1.0])]);
        let near_corner = (b[1] - 4) * W + b[0] - 4;
        assert!(boxed[near_corner][2] > HALF && boxed[near_corner][3] > HALF && plain[near_corner][3] == 0, "{:?}", boxed[near_corner]);
        // capitals: "velo" set as VELO; the monospaced font sets it apart; File with no file is Inter
        assert_ne!(ink(&draw("velo", &[]), 0..H), Some(b));
        assert_eq!(ink(&draw("velo", &[("uppercase", [1.0, 0.0, 0.0, 0.0])]), 0..H), Some(b));
        assert_ne!(ink(&draw("VELO", &[("font", [1.0, 0.0, 0.0, 0.0])]), 0..H), Some(b));
        assert_eq!(ink(&draw("VELO", &[("font", [2.0, 0.0, 0.0, 0.0])]), 0..H), Some(b));
        // a hard shadow, dark and covering, below and to the right
        let shadowed = draw("VELO", &[("shadow", [1.0, 0.0, 0.0, 0.0]), ("shadow_offset", [0.2, 0.2, 0.0, 0.0]), ("shadow_blur", [0.0; 4])]);
        let s = ink(&shadowed, 0..H).unwrap();
        assert!(s[2] > b[2] + 4 && s[3] > b[3] + 4 && shadowed.iter().any(|p| p[3] > HALF && p[0] < 0x2000), "{s:?}");
        // slant leans an I: its head right of its foot; weight thickens; a quarter turn stands it up
        let leaning = draw("I", &[("slant", [0.3, 0.0, 0.0, 0.0])]);
        let i = ink(&leaning, 0..H).unwrap();
        let (head, foot) = (ink(&leaning, i[1]..i[1] + 2).unwrap(), ink(&leaning, i[3] - 1..i[3] + 1).unwrap());
        assert!(head[0] > foot[0] + 3, "{head:?} {foot:?}");
        let lit = |t: &[[u16; 4]]| t.iter().filter(|p| p[3] > HALF).count();
        assert!(lit(&draw("VELO", &[("weight", [1.0, 0.0, 0.0, 0.0])])) > lit(&plain) * 11 / 10);
        let turned = ink(&draw("VELO", &[("rotate", [0.5, 0.0, 0.0, 0.0])]), 0..H).unwrap();
        assert!(height(turned) > width(turned), "{turned:?}");
    }

    #[test]
    fn hap_movies_play_their_frames_on_the_gpu() {
        let Some((device, queue)) = gpu().filter(|(d, _)| d.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC)) else {
            eprintln!("no GPU adapter with BC textures here: skipped");
            return;
        };
        let image = program(&device, "image");
        let ycocg = Program::new(&device, ShaderNode::parse(&crate::compile::IMAGE_YCOCG).unwrap()).unwrap();
        // HAP, and (macOS, Windows) H.264 through the system's decoder
        let names: &[&str] = if cfg!(any(target_os = "macos", windows)) { &["hap1.mov", "hap5-chunks.mov", "hapq-none.mov", "h264.mp4"] } else { &["hap1.mov", "hap5-chunks.mov", "hapq-none.mov"] };
        for &name in names {
            let media = std::sync::Arc::new(crate::media::Media::load(&crate::hap::tests::fixture(name)).unwrap());
            let programs = [if media.ycocg() { ycocg.clone() } else { image.clone() }];
            let mut playing = crate::media::Playing::new(&device, &queue, 0, None, media);
            let views = [(0, playing.texture.create_view(&Default::default()))];
            let nodes = [NodeSpec { program: 0, inputs: vec![] }];
            let mut runner = Runner::new(&device, &programs, &nodes, &views, [64, 32], &wave_texture(&device)).unwrap();
            // stretched over the canvas: the movie's pixels, about where they are in it
            runner.set_params(&queue, &programs, 0, &[[2.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
            let frame = |runner: &mut Runner| {
                let mut encoder = device.create_command_encoder(&Default::default());
                runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
                queue.submit([encoder.finish()]);
                read(&device, &queue, runner, 0).as_chunks::<4>().0.to_vec()
            };
            // which channel is lit (over 0.75) with the other two dark (under 0.25)
            let lit = |t: &[[u16; 4]], x: usize, y: usize| {
                let p = t[y * 64 + x];
                (0..3).filter(|&c| p[c] > 0x3A00).collect::<Vec<_>>().pop().filter(|_| (0..3).filter(|&c| p[c] < 0x3400).count() == 2)
            };
            let first = frame(&mut runner);
            assert_eq!([lit(&first, 8, 4), lit(&first, 56, 4), lit(&first, 32, 26)], [Some(0), Some(1), Some(2)], "{name}: red | green over blue");
            // read ahead on the reader's thread: the frame due at 0.2 s is blue
            playing.step(&queue, 0.1, 1.0, (0.0, 1.0));
            std::thread::sleep(std::time::Duration::from_millis(200));
            playing.step(&queue, 0.1, 1.0, (0.0, 1.0));
            let third = frame(&mut runner);
            assert_eq!([lit(&third, 8, 4), lit(&third, 56, 26)], [Some(2), Some(2)], "{name}: the third frame");
        }
    }

    #[test]
    fn layer_stacks_stack_and_masks_keep_or_cut() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let flat = |rgb: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"Flat\" }}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f({rgb}, 1.0); }}")).unwrap()).unwrap();
        let programs = [flat("1.0, 0.0, 0.0"), flat("0.0, 1.0, 0.0"), program(&device, "layers"), program(&device, "mask")];
        // red under green (added), and red through a green mask (bright enough to count)
        let nodes = [
            NodeSpec { program: 0, inputs: vec![] },
            NodeSpec { program: 1, inputs: vec![] },
            NodeSpec { program: 2, inputs: vec![Some(0), Some(1), None, None, None, None, None, None] },
            NodeSpec { program: 3, inputs: vec![Some(0), Some(1)] },
        ];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [64, 32], &wave_texture(&device)).unwrap();
        runner.set_params(&queue, &programs, 0, &[]);
        runner.set_params(&queue, &programs, 1, &[]);
        let v = |x: f32| [x, 0.0, 0.0, 0.0];
        let stack = |runner: &mut Runner, set: &[(&str, f32)]| runner.set_params(&queue, &programs, 2, &programs[2].node.values(|p| set.iter().find(|s| s.0 == p).map(|s| v(s.1))));
        stack(&mut runner, &[("mode_2", 1.0)]);
        let frame = |runner: &mut Runner, mask_mode: f32| {
            runner.set_params(&queue, &programs, 3, &[v(mask_mode), v(0.0), v(0.5), v(0.1), v(0.0)]);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
            queue.submit([encoder.finish()]);
            (read(&device, &queue, runner, 2)[..4].to_vec(), read(&device, &queue, runner, 3)[..4].to_vec())
        };
        let one = 0x3C00; // 1.0 as a half float
        let (stacked, kept) = frame(&mut runner, 0.0);
        assert_eq!(stacked[..3], [one, one, 0], "red plus green: yellow, unwired layers change nothing");
        assert_eq!(kept, [one, 0, 0, one], "kept where the mask is");
        let (_, cut) = frame(&mut runner, 1.0);
        assert_eq!(cut, [0, 0, 0, 0], "cut out there: clear");
        // the order: red's above green's, so red covers it
        stack(&mut runner, &[("order_1", 1.0)]);
        let (lifted, _) = frame(&mut runner, 0.0);
        assert_eq!(lifted[..3], [one, 0, 0], "red lifted over green");
    }

    #[test]
    fn a_grid_output_puts_each_input_in_its_cell() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let flat = |rgb: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"Flat\" }}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f({rgb}, 1.0); }}")).unwrap()).unwrap();
        let programs = [flat("1.0, 0.0, 0.0"), flat("0.0, 1.0, 0.0"), program(&device, "grid")];
        let mut inputs = vec![None; 8];
        (inputs[0], inputs[1]) = (Some(0), Some(1));
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![] }, NodeSpec { program: 2, inputs }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [64, 32], &wave_texture(&device)).unwrap();
        runner.set_params(&queue, &programs, 0, &[]);
        runner.set_params(&queue, &programs, 1, &[]);
        // 2 × 2, no gap, stretched: red in cell 1 (top left), green in cell 4 (bottom right)
        let v = |x: f32| [x, 0.0, 0.0, 0.0];
        let mut values = vec![v(2.0), v(2.0), v(0.0), v(0.0), [0.0, 0.0, 0.0, 1.0]];
        values.extend((1..=16).map(|c| match c {
            1 => [1.0, 2.0, 0.0, 0.0],
            4 => [2.0, 2.0, 0.0, 0.0],
            _ => [0.0; 4],
        }));
        runner.set_params(&queue, &programs, 2, &values);
        let mut encoder = device.create_command_encoder(&Default::default());
        runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
        queue.submit([encoder.finish()]);
        let texels: Vec<[u16; 4]> = read(&device, &queue, &runner, 2).as_chunks::<4>().0.to_vec();
        let at = |x: usize, y: usize| texels[y * 64 + x][..3].to_vec();
        let one = 0x3C00;
        assert_eq!(at(8, 4), [one, 0, 0], "input 1 in cell 1");
        assert_eq!(at(56, 28), [0, one, 0], "input 2 in cell 4");
        assert_eq!(at(56, 4), [0, 0, 0], "nothing picked cell 2");
    }

    #[test]
    fn a_switch_shows_the_input_its_value_picks() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let flat = |rgb: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"Flat\" }}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f({rgb}, 1.0); }}")).unwrap()).unwrap();
        let programs = [flat("1.0, 0.0, 0.0"), flat("0.0, 1.0, 0.0"), flat("0.0, 0.0, 1.0"), program(&device, "switch")];
        let mut inputs = vec![None; 8];
        (inputs[0], inputs[1], inputs[2]) = (Some(0), Some(1), Some(2));
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![] }, NodeSpec { program: 2, inputs: vec![] }, NodeSpec { program: 3, inputs }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [32, 8], &wave_texture(&device)).unwrap();
        for n in 0..3 {
            runner.set_params(&queue, &programs, n, &[]);
        }
        // value, count, fade (crossfade style): the first texel's color
        let mut shown = |value: f32, count: f32, fade: f32| {
            runner.set_params(&queue, &programs, 3, &[[value, 0.0, 0.0, 0.0], [count, 0.0, 0.0, 0.0], [fade, 0.0, 0.0, 0.0], [0.0; 4]]);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
            queue.submit([encoder.finish()]);
            read(&device, &queue, &runner, 3)[..3].to_vec()
        };
        let one = 0x3C00;
        assert_eq!(shown(0.0, 2.0, 0.0), [one, 0, 0], "0: the first");
        assert_eq!(shown(0.4, 2.0, 0.0), [one, 0, 0], "nearer 0: the first");
        assert_eq!(shown(0.6, 2.0, 0.0), [0, one, 0], "nearer 1: the second");
        assert_eq!(shown(0.5, 3.0, 0.0), [0, one, 0], "the middle of three");
        assert_eq!(shown(1.0, 3.0, 0.0), [0, 0, one], "1: the last of three");
        // a crossfade halfway through: some of each
        let half = shown(0.5, 2.0, 1.0);
        assert!(half[0] > 0 && half[1] > 0 && half[2] == 0, "{half:?}");
    }

    #[test]
    fn a_switch_reads_only_what_it_shows_and_a_clear_blend_its_base() {
        assert_eq!(switch_shows(0.0, 2.0, 0.0), [Some(0), None]);
        assert_eq!(switch_shows(0.5, 3.0, 0.0), [Some(1), None], "the middle of three");
        assert_eq!(switch_shows(1.0, 16.0, 0.0), [None, Some(15)], "the last of sixteen");
        assert_eq!(switch_shows(0.25, 3.0, 0.0), [Some(0), Some(1)], "on an edge: both, whichever way the GPU rounds");
        assert_eq!(switch_shows(0.5, 2.0, 1.0), [Some(0), Some(1)], "halfway through a fade: both");
        assert_eq!(switch_shows(0.1, 2.0, 0.5), [Some(0), None], "before the fade starts");
        let blend = Lazy::Blend { opacity: 2 };
        assert!(blend.reads(|_| 0.0, 0) && !blend.reads(|_| 0.0, 1), "no opacity: the base alone");
        assert!(blend.reads(|_| 0.5, 1));
    }

    #[test]
    fn what_a_switch_doesnt_show_never_renders() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let flat = |rgb: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"Flat\" }}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f({rgb}, 1.0); }}")).unwrap()).unwrap();
        let programs = [flat("1.0, 0.0, 0.0"), flat("0.0, 1.0, 0.0"), program(&device, "switch")];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![] }, NodeSpec { program: 2, inputs: vec![Some(0), Some(1)] }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [32, 8], &wave_texture(&device)).unwrap();
        let lazy = Lazy::Switch { value: 0, count: 1, fade: 2 };
        let one = 0x3C00;
        for (value, red, green) in [(0.0, one, 0), (1.0, one, one)] {
            runner.set_params(&queue, &programs, 2, &[[value, 0.0, 0.0, 0.0], [2.0, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4]]);
            let values = [value, 2.0, 0.0];
            let awake = runner.needed([2], |n, i| n != 2 || lazy.reads(|p| values[p], i));
            assert_eq!(awake, [value == 0.0, value == 1.0, true]);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render_timed(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0], None, Some(&awake));
            queue.submit([encoder.finish()]);
            // the red one rendered the first time and kept its picture; the green one only once shown
            assert_eq!(read(&device, &queue, &runner, 0)[0], red);
            assert_eq!(read(&device, &queue, &runner, 1)[1], green);
            assert_eq!(read(&device, &queue, &runner, 2)[..3], if value == 0.0 { [one, 0, 0] } else { [0, one, 0] });
        }
    }

    #[test]
    fn a_media_list_shows_its_files_in_turn() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let flat = |rgb: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"Flat\" }}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f({rgb}, 1.0); }}")).unwrap()).unwrap();
        let programs = [flat("1.0, 0.0, 0.0"), flat("0.0, 1.0, 0.0"), program(&device, "medialist")];
        let mut inputs = vec![None; 8];
        (inputs[0], inputs[1]) = (Some(0), Some(1));
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![] }, NodeSpec { program: 2, inputs }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [32, 8], &wave_texture(&device)).unwrap();
        for n in 0..2 {
            runner.set_params(&queue, &programs, n, &[]);
        }
        // in order, on the seconds, a new one each second
        let v = |x: f32| [x, 0.0, 0.0, 0.0];
        runner.set_params(
            &queue,
            &programs,
            2,
            &programs[2].node.values(|p| match p {
                "clock" => Some(v(1.0)),
                "every" => Some(v(1.0)),
                _ => None,
            }),
        );
        let mut shown = |time: f32| {
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[time, 1.0 / 60.0, 0.0, 2.0]);
            queue.submit([encoder.finish()]);
            read(&device, &queue, &runner, 2)[..3].to_vec()
        };
        let one = 0x3C00;
        assert_eq!(shown(0.5), [one, 0, 0], "the first");
        assert_eq!(shown(1.5), [0, one, 0], "a second on, the second");
        assert_eq!(shown(2.5), [one, 0, 0], "and round again");
    }

    #[test]
    fn every_preset_and_effect_builds_on_the_gpu() {
        let _gpu = crate::gpu_lock();
        let Some((device, _queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        for (slug, code) in crate::PRESETS.iter().chain(&crate::EFFECT_PRESETS) {
            let node = ShaderNode::parse(code).unwrap();
            if let Err(e) = Program::new(&device, node) {
                panic!("{slug}: {}", e.message);
            }
        }
    }

    #[test]
    fn what_wgpu_rejects_is_the_shaders_error_not_a_panic() {
        let _gpu = crate::gpu_lock();
        let Some((device, _queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        // naga lets it through; the audio texture can't be sampled with a filtering sampler
        let node = ShaderNode::parse("/*{ \"name\": \"Filtered wave\", \"audio\": true }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f { return textureSample(wave, smp, uv); }").unwrap();
        assert!(Program::new(&device, node).is_err());
        // and the device goes on: a good one still builds
        assert!(Program::new(&device, ShaderNode::parse(crate::BUILTINS[0].1).unwrap()).is_ok());
    }

    #[test]
    fn particles_are_born_fly_and_die() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let programs = [program(&device, "particles")];
        let mut runner = Runner::new(&device, &programs, &[NodeSpec { program: 0, inputs: vec![] }], &[], [64, 64], &wave_texture(&device)).unwrap();
        let mut frames = 0.0;
        // a frame at 60 fps with these values: whether each texel is lit
        let mut frame = |set: &[(&str, f32)]| {
            let values = programs[0].node.values(|id| set.iter().find(|s| s.0 == id).map(|s| [s.1, 0.0, 0.0, 0.0]));
            runner.set_params(&queue, &programs, 0, &values);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[frames / 60.0, 1.0 / 60.0, frames, 1.0]);
            queue.submit([encoder.finish()]);
            frames += 1.0;
            read(&device, &queue, &runner, 0).as_chunks::<4>().0.iter().map(|p| p[3] != 0).collect::<Vec<bool>>()
        };
        let at = |lit: &[bool], x: usize, y: usize| lit[y * 64 + x];
        assert!(!frame(&[]).contains(&true), "emission starts at 0: nothing by itself");
        // a burst at the centre, sent right
        let right = [("emission", 3000.0), ("speed", 2.0), ("direction", 0.0), ("spread", 0.0), ("size", 0.1), ("life", 0.3)];
        let born = frame(&right);
        assert!(at(&born, 32, 32) && !at(&born, 2, 2), "born at the emitter, drawn round it");
        let still = [("speed", 2.0), ("direction", 0.0), ("size", 0.1)];
        let mut flown = Vec::new();
        for _ in 0..6 {
            flown = frame(&still);
        }
        assert!(!at(&flown, 32, 32) && (40..56).any(|x| at(&flown, x, 32)), "flown to the right");
        // a burst that doesn't move: there until its life is over
        frame(&[("emission", 3000.0), ("speed", 0.0), ("size", 0.1), ("life", 0.2)]);
        let mut aged = Vec::new();
        for _ in 0..5 {
            aged = frame(&[("size", 0.1)]);
        }
        assert!(at(&aged, 32, 32), "a tenth of a second old, still there");
        for _ in 0..15 {
            aged = frame(&[("size", 0.1)]);
        }
        assert!(!aged.contains(&true), "older than their life: dead");
    }
    #[test]
    fn a_particle_can_be_a_picture() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        // a 2×2 picture, all green: the sprites take it, tinted white, instead of soft dots
        let media = std::sync::Arc::new(crate::media::Media { size: [2, 2], frames: vec![[0u8, 255, 0, 255].repeat(4)], ends: vec![1.0], movie: None, video: None });
        let programs = [program(&device, "particles")];
        let nodes = [NodeSpec { program: 0, inputs: vec![None] }];
        let playing = crate::media::Playing::new(&device, &queue, 0, None, media);
        let views = [(0, playing.texture.create_view(&Default::default()))];
        let mut runner = Runner::new(&device, &programs, &nodes, &views, [32, 32], &wave_texture(&device)).unwrap();
        let v = |x: f32| [x, 0.0, 0.0, 0.0];
        let set = [("emission", 3000.0), ("speed", 0.0), ("size", 0.2), ("life", 5.0)];
        runner.set_params(&queue, &programs, 0, &programs[0].node.values(|p| set.iter().find(|s| s.0 == p).map(|s| v(s.1)).or_else(|| (p == "tint").then_some([1.0; 4]))));
        for frame in 0..3 {
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[frame as f32 / 60.0, 1.0 / 60.0, frame as f32, 1.0]);
            queue.submit([encoder.finish()]);
        }
        let pixels = read(&device, &queue, &runner, 0);
        let (r, g) = pixels.as_chunks::<4>().0.iter().fold((0u32, 0u32), |(r, g), p| (r.max(u32::from(p[0])), g.max(u32::from(p[1]))));
        assert!(g > 0 && r == 0, "green sprites, no red: ({r}, {g})");
    }

    #[test]
    fn every_preset_draws_a_picture() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        for (slug, source) in crate::PRESETS {
            let programs = [Program::new(&device, ShaderNode::parse(source).unwrap()).unwrap()];
            let mut runner = Runner::new(&device, &programs, &[NodeSpec { program: 0, inputs: vec![] }], &[], [32, 32], &wave_texture(&device)).unwrap();
            runner.set_params(&queue, &programs, 0, &programs[0].node.values(|_| None));
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
            queue.submit([encoder.finish()]);
            let texels = read(&device, &queue, &runner, 0);
            let shades = texels.chunks(4).map(|t| t[..3].to_vec()).collect::<std::collections::HashSet<_>>().len();
            assert!(shades > 8, "{slug} draws {shades} colors");
        }
    }

    #[test]
    fn the_crystal_ball_draws_its_glass_over_its_background() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let source = crate::PRESETS.iter().find(|p| p.0 == "crystal_ball").unwrap().1;
        let programs = [Program::new(&device, ShaderNode::parse(source).unwrap()).unwrap()];
        let mut runner = Runner::new(&device, &programs, &[NodeSpec { program: 0, inputs: vec![] }], &[], [64, 32], &wave_texture(&device)).unwrap();
        runner.set_params(&queue, &programs, 0, &programs[0].node.values(|_| None));
        let mut encoder = device.create_command_encoder(&Default::default());
        runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
        queue.submit([encoder.finish()]);
        let texels: Vec<[u16; 4]> = read(&device, &queue, &runner, 0).as_chunks::<4>().0.to_vec();
        // half floats: positive ones order as their bits do
        let (middle, corner) = (texels[16 * 64 + 32], texels[0]);
        assert!(middle[0] > corner[0] && middle[2] > corner[2], "the glass {middle:?} over the background {corner:?}");
    }

    #[test]
    fn a_blend_lays_each_input_over_the_ones_before() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let flat = |rgb: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"Flat\" }}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f({rgb}, 1.0); }}")).unwrap()).unwrap();
        let programs = [flat("1.0, 0.0, 0.0"), flat("0.0, 1.0, 0.0"), flat("0.0, 0.0, 1.0"), program(&device, "blend")];
        let mut inputs = vec![None; 16];
        (inputs[0], inputs[1], inputs[2]) = (Some(0), Some(1), Some(2));
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![] }, NodeSpec { program: 2, inputs: vec![] }, NodeSpec { program: 3, inputs }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [32, 8], &wave_texture(&device)).unwrap();
        for n in 0..3 {
            runner.set_params(&queue, &programs, n, &[]);
        }
        let mut shown = |mode: f32, key: f32| {
            // inputs, mode, opacity, intensity, key, softness
            let values = [3.0, mode, 1.0, 1.0, key, 0.1].map(|v| [v, 0.0, 0.0, 0.0]);
            runner.set_params(&queue, &programs, 3, &values);
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
            queue.submit([encoder.finish()]);
            read(&device, &queue, &runner, 3)[..3].to_vec()
        };
        let one = 0x3C00;
        // added up: red, green and blue make white; the thirteen unwired inputs change nothing
        assert_eq!(shown(1.0, 0.0), [one, one, one]);
        // normal: the last wired one covers the rest
        assert_eq!(shown(0.0, 0.0), [0, 0, one]);
        // keyed at half: the dark blue lets the green under it through
        assert_eq!(shown(0.0, 0.5), [0, one, 0]);
    }

    #[test]
    fn every_kind_of_noise_draws_its_own_picture() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let programs = [program(&device, "noise")];
        let mut runner = Runner::new(&device, &programs, &[NodeSpec { program: 0, inputs: vec![] }], &[], [32, 32], &wave_texture(&device)).unwrap();
        let pictures: Vec<Vec<u16>> = (0..6)
            .map(|kind| {
                runner.set_params(&queue, &programs, 0, &programs[0].node.values(|p| (p == "noise").then_some([kind as f32, 0.0, 0.0, 0.0])));
                let mut encoder = device.create_command_encoder(&Default::default());
                runner.render(&queue, &mut encoder, &programs, &[0.0, 1.0 / 60.0, 0.0, 2.0]);
                queue.submit([encoder.finish()]);
                read(&device, &queue, &runner, 0)
            })
            .collect();
        for (i, a) in pictures.iter().enumerate() {
            assert!(a.chunks(4).map(|t| t[0]).collect::<std::collections::HashSet<_>>().len() > 8, "noise {i} isn't flat");
            for (j, b) in pictures.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "noise {i} and {j} draw the same");
            }
        }
    }

    #[test]
    fn cycles_and_bad_inputs_are_refused() {
        let cycle = [NodeSpec { program: 0, inputs: vec![Some(1)] }, NodeSpec { program: 0, inputs: vec![Some(0)] }];
        assert!(topological(&cycle, &[false, false]).unwrap_err().contains("cycle"));
        assert!(topological(&[NodeSpec { program: 0, inputs: vec![Some(7)] }], &[false]).unwrap_err().contains("doesn't exist"));
        let chain = [NodeSpec { program: 0, inputs: vec![Some(2)] }, NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 0, inputs: vec![Some(1)] }];
        assert_eq!(topological(&chain, &[false; 3]).unwrap(), vec![1, 2, 0]);
        // a feedback node with no loop waits for its input like any other…
        assert_eq!(topological(&chain, &[true, false, false]).unwrap(), vec![1, 2, 0]);
        // …and on a loop it goes first: noise (0) → blend (1) → feedback (2) → blend's second input
        let looped = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 0, inputs: vec![Some(0), Some(2)] }, NodeSpec { program: 0, inputs: vec![Some(1)] }];
        assert_eq!(topological(&looped, &[false, false, true]).unwrap(), vec![0, 2, 1]);
        assert!(topological(&looped, &[false; 3]).is_err(), "the same loop without feedback");
    }

    #[test]
    fn the_picture_math_and_layout_nodes_do_what_they_say() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let wave = wave_texture(&device);
        let shader = |body: &str| Program::new(&device, ShaderNode::parse(&format!("/*{{ \"name\": \"T\" }}*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return {body}; }}")).unwrap()).unwrap();
        // the last node's pixel at (x, y) of 32 × 32, its inputs the nodes before it, after a frame at
        // each time; `set` its parameters by id
        let pixel = |sources: &[&str], node: &str, set: &[(&str, [f32; 4])], times: &[f32], at: (u32, u32)| -> [f32; 4] {
            let mut programs: Vec<Program> = sources.iter().map(|b| shader(b)).collect();
            programs.push(program(&device, node));
            let n = programs.len() - 1;
            let mut nodes: Vec<NodeSpec> = (0..n).map(|i| NodeSpec { program: i, inputs: vec![] }).collect();
            nodes.push(NodeSpec { program: n, inputs: (0..n).map(Some).collect() });
            let mut runner = Runner::new(&device, &programs, &nodes, &[], [32, 32], &wave).unwrap();
            for i in 0..n {
                runner.set_params(&queue, &programs, i, &[]);
            }
            runner.set_params(&queue, &programs, n, &programs[n].node.values(|id| set.iter().find(|s| s.0 == id).map(|s| s.1)));
            for &t in times {
                let mut encoder = device.create_command_encoder(&Default::default());
                runner.render(&queue, &mut encoder, &programs, &[t, 1.0 / 60.0, 0.0, 1.0, 120.0]);
                queue.submit([encoder.finish()]);
            }
            let texels = read(&device, &queue, &runner, n);
            let i = (at.1 * 32 + at.0) as usize * 4;
            [0, 1, 2, 3].map(|k| half(texels[i + k]))
        };
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02);
        let v = |x: f32| [x, 0.0, 0.0, 0.0];
        let gray = "vec4f(0.5, 0.5, 0.5, 1.0)";
        let uv = "vec4f(uv, 0.0, 1.0)";

        // Levels: black at a quarter, white at three quarters keeps the middle; black at the middle darkens it
        assert!(near(pixel(&[gray], "levels", &[("black", v(0.25)), ("white", v(0.75))], &[0.0], (16, 16)), [0.5, 0.5, 0.5, 1.0]));
        assert!(near(pixel(&[gray], "levels", &[("black", v(0.5)), ("channel", v(1.0))], &[0.0], (16, 16)), [0.0, 0.5, 0.5, 1.0]), "red only");
        // Threshold: over the level the over color, under it the under one
        assert!(near(pixel(&["vec4f(0.6, 0.6, 0.6, 1.0)"], "threshold", &[], &[0.0], (16, 16)), [1.0; 4]));
        assert!(near(pixel(&["vec4f(0.4, 0.4, 0.4, 1.0)"], "threshold", &[], &[0.0], (16, 16)), [0.0, 0.0, 0.0, 1.0]));
        // Channels: green and blue from red, alpha from the brightness
        assert!(near(pixel(&["vec4f(1.0, 0.0, 0.0, 1.0)"], "channels", &[("green", v(0.0)), ("blue", v(5.0))], &[0.0], (16, 16)), [1.0, 1.0, 0.0, 1.0]));
        // Merge: the first one's brightness as red, the second's as green, the third's as blue
        assert!(near(pixel(&["vec4f(1.0)", "vec4f(0.0, 0.0, 0.0, 1.0)", gray], "merge", &[], &[0.0], (16, 16)), [1.0, 0.0, 0.5, 1.0]));
        // UV Map: a flat map reads one place of the picture everywhere; at amount 0, where it is
        assert!(near(pixel(&[uv, "vec4f(0.25, 0.75, 0.0, 1.0)"], "uvmap", &[], &[0.0], (4, 26)), [0.25, 0.75, 0.0, 1.0]));
        assert!(near(pixel(&[uv, "vec4f(0.25, 0.75, 0.0, 1.0)"], "uvmap", &[("amount", v(0.0))], &[0.0], (4, 26)), [4.5 / 32.0, 26.5 / 32.0, 0.0, 1.0]));
        // Lookup: brightness a half reads the palette half way along
        assert!(near(pixel(&[gray, uv], "lookup", &[], &[0.0], (6, 6)), [0.5, 0.5, 0.0, 1.0]));
        // Crop: see-through outside its edges, the picture inside; fill stretches the inside over it all
        assert!(near(pixel(&[uv], "crop", &[], &[0.0], (0, 16)), [0.0; 4]));
        assert!(near(pixel(&[uv], "crop", &[], &[0.0], (16, 16)), [16.5 / 32.0, 16.5 / 32.0, 0.0, 1.0]));
        assert!(near(pixel(&[uv], "crop", &[("left", v(0.5)), ("right", v(0.0)), ("fit", v(1.0))], &[0.0], (0, 16)), [0.5 + 0.25 / 32.0, 0.1 + 0.8 * 16.5 / 32.0, 0.0, 1.0]));
        // Polar: the picture's bottom edge round the middle, its top at the top
        assert!(pixel(&[uv], "polar", &[], &[0.0], (16, 16))[1] > 0.85);
        assert!(pixel(&[uv], "polar", &[], &[0.0], (16, 0))[1] < 0.15);
        // Edges: a line where black meets white, nothing (see-through) away from it
        let step = "vec4f(vec3f(step(0.5, uv.x)), 1.0)";
        assert!(pixel(&[step], "edges", &[], &[0.0], (16, 16))[3] > 0.9 && pixel(&[step], "edges", &[], &[0.0], (4, 16))[3] == 0.0);
        // Freeze: held, it keeps the frame it had; let go, it's live again; every 4 beats, a step's first frame
        let clock = "vec4f(g.time, 0.0, 0.0, 1.0)";
        assert!((pixel(&[clock], "freeze", &[("hold", v(1.0))], &[0.25, 0.5], (16, 16))[0] - 0.25).abs() < 0.01, "held");
        assert!((pixel(&[clock], "freeze", &[], &[0.25, 0.5], (16, 16))[0] - 0.5).abs() < 0.01, "live");
    }

    #[test]
    fn the_outputs_grade_changes_nothing_until_it_is_set() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let wave = wave_texture(&device);
        let programs = [Program::new(&device, ShaderNode::parse(PATTERN).unwrap()).unwrap(), program(&device, "output")];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![Some(0)] }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [64, 36], &wave).unwrap();
        runner.set_params(&queue, &programs, 0, &[]);
        // the picture and the Output's, its parameters `set` by id
        let mut graded = |set: &[(&str, f32)]| {
            runner.set_params(&queue, &programs, 1, &programs[1].node.values(|id| set.iter().find(|s| s.0 == id).map(|s| [s.1, 0.0, 0.0, 0.0])));
            let mut encoder = device.create_command_encoder(&Default::default());
            runner.render(&queue, &mut encoder, &programs, &[1.0, 1.0 / 60.0, 0.0, 16.0 / 9.0]);
            queue.submit([encoder.finish()]);
            (read(&device, &queue, &runner, 0), read(&device, &queue, &runner, 1))
        };
        let (picture, out) = graded(&[]);
        assert_eq!(out, picture, "the defaults leave it as it is, to the bit");
        let (_, out) = graded(&[("saturation", 0.0)]);
        assert!(out.as_chunks::<4>().0.iter().all(|p| half(p[0]) == half(p[1]) && half(p[1]) == half(p[2])), "grey");
        assert_ne!(out, picture);
        let (_, out) = graded(&[("fade", 1.0)]);
        assert!(out.as_chunks::<4>().0.iter().all(|p| p[..3].iter().all(|&c| half(c) == 0.0) && half(p[3]) == 1.0), "black, still opaque");
    }

    /// What an effect is shown in the contact sheets: a dark field, a ring, bars, a moving disc.
    const PATTERN: &str = r#"/*{ "name": "Pattern" }*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    var c = mix(vec3f(0.03, 0.02, 0.12), vec3f(0.85, 0.2, 0.5), smoothstep(0.7, -0.5, p.y + p.x * 0.6));
    c = mix(c, vec3f(1.0, 0.85, 0.3), smoothstep(0.012, 0.0, abs(length(p) - 0.3)));
    c = mix(c, vec3f(0.1, 0.7, 1.0), step(0.5, fract(uv.x * 8.0)) * step(0.85, uv.y));
    c = mix(c, vec3f(0.2, 1.0, 0.4), smoothstep(0.1, 0.09, length(p - vec2f(0.45 * sin(g.time), 0.1))));
    c = mix(c, vec3f(1.0), step(0.97, fract(uv.y * 12.0)) * step(uv.x, 0.2));
    return vec4f(c, 1.0);
}"#;

    /// Not a check: `VELO_SHOTS=<dir> cargo test -p velo-engine --release contact_sheets -- --ignored`
    /// draws every built-in picture, preset and effect (or those `VELO_SHOTS_ONLY` names, commas
    /// between) into `<dir>/<slug>.png`: 1 s and 4 s still, 4 s on a kick with Automagic at 1, 9 s,
    /// each 384 × 216 times `VELO_SHOTS_SCALE`; and prints what each costs at 1080p.
    #[test]
    #[ignore]
    fn contact_sheets() {
        let dir = std::path::PathBuf::from(std::env::var_os("VELO_SHOTS").expect("VELO_SHOTS=<dir>"));
        std::fs::create_dir_all(&dir).unwrap();
        let only = std::env::var("VELO_SHOTS_ONLY").unwrap_or_default();
        let _gpu = crate::gpu_lock();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY, ..Default::default() })).unwrap();
        // a spectrum falling off to the highs, levels and a sine in the scope
        let wave = wave_texture(&device);
        let columns = crate::WAVE_COLUMNS;
        let pixels: Vec<f32> = (0..3 * columns)
            .flat_map(|i| {
                let x = (i % columns) as f32 / columns as f32;
                match i / columns {
                    0 => [0.6 + 0.3 * (x * 40.0).sin(), 0.8, 0.5, 0.3],
                    1 => [(x * 37.7).sin() * 0.6, 0.0, 0.0, 0.0],
                    _ => [(0.9 - 0.6 * x + 0.15 * (x * 90.0).sin()).clamp(0.0, 1.0), 0.0, 0.0, 0.0],
                }
            })
            .collect();
        let bytes: Vec<u8> = pixels.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_texture(wave.texture().as_image_copy(), &bytes, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(columns as u32 * 16), rows_per_image: None }, wgpu::Extent3d { width: columns as u32, height: 3, depth_or_array_layers: 1 });
        let skip = ["image", "text", "textlist", "medialist", "particles"];
        let all = BUILTINS.iter().filter(|b| !skip.contains(&b.0)).chain(&crate::PRESETS).chain(&crate::EFFECT_PRESETS);
        for (slug, code) in all.filter(|(slug, _)| only.is_empty() || only.split(',').any(|o| o == *slug)) {
            let node = ShaderNode::parse(code).unwrap();
            let programs = [Program::new(&device, ShaderNode::parse(PATTERN).unwrap()).unwrap(), program(&device, "noise"), Program::new(&device, node).unwrap()];
            let inputs = (0..programs[2].node.inputs.len()).map(|i| (i < 2).then_some(i)).collect();
            let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![] }, NodeSpec { program: 2, inputs }];
            let globals = |t: f32, loud: bool| {
                let a = if loud { 1.0 } else { 0.0 };
                // time, delta, frame, aspect, bpm, beat and bar phase, level, bass, mid, high, kick,
                // snare, hat, the band clocks, melody, beats
                vec![t, 1.0 / 30.0, t * 30.0, 16.0 / 9.0, 120.0, (t * 2.0).fract(), (t * 0.5).fract(), 0.8 * a, 0.9 * a, 0.6 * a, 0.5 * a, a, 0.5 * a, 0.7 * a, t * 0.5, t * 0.4, t * 0.3, t * 0.45, 0.6 * a, (t * 2.0).floor()]
            };
            let scale: u32 = std::env::var("VELO_SHOTS_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
            let (tw, th) = (384 * scale, 216 * scale);
            let mut sheet = image::RgbaImage::new(tw * 2, th * 2);
            for (tile, (t, loud)) in [(1.0, false), (4.0, false), (4.0, true), (9.0, false)].into_iter().enumerate() {
                let mut runner = Runner::new(&device, &programs, &nodes, &[], [tw, th], &wave).unwrap();
                runner.set_params(&queue, &programs, 0, &[]);
                runner.set_params(&queue, &programs, 1, &programs[1].node.values(|_| None));
                runner.set_params(&queue, &programs, 2, &programs[2].node.values(|id| (loud && (id == "audio" || id == "music")).then_some([1.0, 0.0, 0.0, 0.0])));
                // a second of frames before, for what feeds back; the music only on the last
                for f in 0..30 {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    runner.render(&queue, &mut encoder, &programs, &globals(t - 1.0 + f as f32 / 30.0, loud && f == 29));
                    queue.submit([encoder.finish()]);
                }
                let texels = read(&device, &queue, &runner, 2);
                for (i, p) in texels.as_chunks::<4>().0.iter().enumerate() {
                    let rgba = p.map(|c| (half(c).clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                    // what's see-through over a checker, as a Blend would show it
                    let (x, y) = (i as u32 % tw, i as u32 / tw);
                    let back = if (x / 12 + y / 12) % 2 == 0 { 40.0 } else { 70.0 };
                    let a = f32::from(rgba[3]) / 255.0;
                    let over = |c: u8| (f32::from(c) + back * (1.0 - a)).min(255.0) as u8;
                    sheet.put_pixel(x + tw * (tile as u32 % 2), y + th * (tile as u32 / 2), image::Rgba([over(rgba[0]), over(rgba[1]), over(rgba[2]), 255]));
                }
            }
            sheet.save(dir.join(format!("{slug}.png"))).unwrap();
            // the cost: the node alone at 1080p, timed on the GPU
            let Ok(mut runner) = Runner::new(&device, &programs, &nodes, &[], [1920, 1080], &wave) else { continue };
            for n in 0..3 {
                runner.set_params(&queue, &programs, n, &programs[n].node.values(|_| None));
            }
            let ms = if device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
                let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: 6 });
                let resolve = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 48, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
                let back = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 48, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
                let mut best = f32::MAX;
                for f in 0..12 {
                    let mut encoder = device.create_command_encoder(&Default::default());
                    runner.render_timed(&queue, &mut encoder, &programs, &globals(f as f32 / 30.0, false), Some(&set), None);
                    encoder.resolve_query_set(&set, 0..6, &resolve, 0);
                    encoder.copy_buffer_to_buffer(&resolve, 0, &back, 0, 48);
                    queue.submit([encoder.finish()]);
                    back.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    let ticks: Vec<u64> = back.slice(..).get_mapped_range().unwrap().as_chunks::<8>().0.iter().map(|b| u64::from_le_bytes(*b)).collect();
                    back.unmap();
                    best = best.min(ticks[5].saturating_sub(ticks[4]) as f32 * queue.get_timestamp_period() / 1e6);
                }
                best
            } else {
                f32::NAN
            };
            println!("{slug}: {ms:.2} ms at 1080p");
        }
    }

    #[test]
    fn a_time_machine_shows_its_input_as_it_was_frames_ago() {
        let _gpu = crate::gpu_lock();
        let Some((device, queue)) = gpu() else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        // a picture whose red is the time, into a Time Machine
        let clock = ShaderNode::parse(r#"/*{ "name": "Clock" }*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(g.time, 0.0, 0.0, 1.0); }"#).unwrap();
        let programs = [Program::new(&device, clock).unwrap(), program(&device, "timemachine")];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![Some(0), None] }];
        let mut runner = Runner::new(&device, &programs, &nodes, &[], [64, 36], &wave_texture(&device)).unwrap();
        let frames = |runner: &mut Runner, back: f32, from: usize, to: usize| {
            let values = programs[1].node.values(|p| (p == "frames").then_some([back, 0.0, 0.0, 0.0]));
            runner.set_params(&queue, &programs, 1, &values);
            for frame in from..to {
                let mut encoder = device.create_command_encoder(&Default::default());
                runner.render(&queue, &mut encoder, &programs, &[frame as f32 / 10.0, 1.0 / 60.0, frame as f32, 16.0 / 9.0]);
                queue.submit([encoder.finish()]);
            }
            half(read(&device, &queue, runner, 1)[0])
        };
        // the tenth frame (0.9) shows the seventh's (0.6); between two frames, between their pictures
        let got = frames(&mut runner, 3.0, 0, 10);
        assert!((got - 0.6).abs() < 0.01, "{got}");
        assert!((frames(&mut runner, 2.5, 10, 11) - 0.75).abs() < 0.01, "frame 10 (1.0), 2.5 back: between 0.8 and 0.7");
        assert!((frames(&mut runner, 0.0, 11, 12) - 1.1).abs() < 0.01, "none back: as it is");
        // further back than it has kept: the ring wraps over what came last, never past it
        assert!((frames(&mut runner, 31.0, 12, 50) - (4.9 - 3.1)).abs() < 0.01);
    }
}
