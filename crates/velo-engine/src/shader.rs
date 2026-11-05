//! Shader nodes (docs/plugins.md §2): one WGSL file whose leading `/*{ … }*/` JSON header
//! declares the node; its body defines `fn main_image(uv: vec2f, px: vec2f) -> vec4f`. Velo
//! writes the rest around it: the globals `g` (time, audio, beat), every parameter as a plain
//! variable named by its id (ISF-style: `speed`, not `params.speed`), one texture per input plus
//! the sampler `smp`, `prev` for feedback, `wave` for audio, `past_frame` for a history, the
//! entry points, and for effects the `amount` contract (0 passes the input through, docs/graph.md).

use std::fmt;

use serde::Deserialize;
use serde_json::Value;

/// What `g` holds, in buffer order. The host writes these as consecutive f32s: `resolution` is
/// a vec2 (two floats), every other field one float.
pub const GLOBALS: [&str; 21] =
    ["resolution", "time", "delta", "frame", "aspect", "bpm", "beat_phase", "bar_phase", "level", "bass", "mid", "high", "kick", "snare", "hat", "bass_time", "mid_time", "high_time", "level_time", "melody", "beats"];
/// Floats the globals take in the buffer (`resolution` counts two), a multiple of four.
pub const GLOBAL_FLOATS: usize = 24;

/// The built-in nodes, written in the same format as anyone's (dogfooding the format).
pub const BUILTINS: [(&str, &str); 35] = [
    ("noise", include_str!("../shaders/noise.wgsl")),
    ("gradient", include_str!("../shaders/gradient.wgsl")),
    ("shapes", include_str!("../shaders/shapes.wgsl")),
    ("waveform", include_str!("../shaders/waveform.wgsl")),
    ("transform", include_str!("../shaders/transform.wgsl")),
    ("kaleido", include_str!("../shaders/kaleido.wgsl")),
    ("color", include_str!("../shaders/color.wgsl")),
    ("blur", include_str!("../shaders/blur.wgsl")),
    ("feedback", include_str!("../shaders/feedback.wgsl")),
    ("displace", include_str!("../shaders/displace.wgsl")),
    ("blend", include_str!("../shaders/blend.wgsl")),
    ("image", include_str!("../shaders/image.wgsl")),
    // the Text and the Text List draw their words alike
    ("text", concat!(include_str!("../shaders/text.wgsl"), include_str!("../shaders/text-draw.wgsl"))),
    ("textlist", concat!(include_str!("../shaders/textlist.wgsl"), include_str!("../shaders/text-draw.wgsl"))),
    ("layers", include_str!("../shaders/layers.wgsl")),
    ("mask", include_str!("../shaders/mask.wgsl")),
    ("grid", include_str!("../shaders/grid.wgsl")),
    ("switch", include_str!("../shaders/switch.wgsl")),
    ("particles", include_str!("../shaders/particles.wgsl")),
    ("opacity", include_str!("../shaders/opacity.wgsl")),
    ("medialist", include_str!("../shaders/medialist.wgsl")),
    // math on pictures, layout and time (docs/graph.md §4)
    ("levels", include_str!("../shaders/levels.wgsl")),
    ("threshold", include_str!("../shaders/threshold.wgsl")),
    ("edges", include_str!("../shaders/edges.wgsl")),
    ("channels", include_str!("../shaders/channels.wgsl")),
    ("merge", include_str!("../shaders/merge.wgsl")),
    ("uvmap", include_str!("../shaders/uvmap.wgsl")),
    ("lookup", include_str!("../shaders/lookup.wgsl")),
    ("crop", include_str!("../shaders/crop.wgsl")),
    ("polar", include_str!("../shaders/polar.wgsl")),
    ("freeze", include_str!("../shaders/freeze.wgsl")),
    ("key", include_str!("../shaders/key.wgsl")),
    ("lumablur", include_str!("../shaders/lumablur.wgsl")),
    ("timemachine", include_str!("../shaders/timemachine.wgsl")),
    // the Output's last grade of what goes out
    ("output", include_str!("../shaders/output.wgsl")),
];

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamType {
    Float,
    Int,
    Bool,
    Color,
    Vec2,
    /// Four numbers a custom inspector edits (a Grid's cell): no slider of its own.
    Vec4,
}

impl ParamType {
    fn wgsl(self) -> &'static str {
        match self {
            ParamType::Float => "f32",
            ParamType::Int => "i32",
            ParamType::Bool => "bool",
            ParamType::Color => "vec4<f32>",
            ParamType::Vec2 => "vec2<f32>",
            ParamType::Vec4 => "vec4<f32>",
        }
    }

    /// How the variable is read from its vec4 slot in the parameter buffer.
    fn read(self, slot: usize) -> String {
        let v = format!("velo_params.v[{slot}]");
        match self {
            ParamType::Float => format!("{v}.x"),
            ParamType::Int => format!("i32(round({v}.x))"),
            ParamType::Bool => format!("{v}.x > 0.5"),
            ParamType::Color => v,
            ParamType::Vec2 => format!("{v}.xy"),
            ParamType::Vec4 => v,
        }
    }
}

/// A parameter as declared in the header; each takes one vec4 slot in the parameter buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub id: String,
    pub ty: ParamType,
    pub default: [f32; 4],
    pub min: f32,
    pub max: f32,
    /// An int's values by name ("Normal", "Add"…), from 0: shown as choices, not a slider.
    pub names: Vec<String>,
}

/// A shader file, parsed and wrapped: ready to validate and build a pipeline from.
#[derive(Clone, Debug)]
pub struct ShaderNode {
    pub name: String,
    /// "generator", "effect", "composite".
    pub category: String,
    pub description: String,
    pub params: Vec<Param>,
    /// Texture inputs, in binding order.
    pub inputs: Vec<String>,
    /// Reads its own previous frame as `prev`.
    pub feedback: bool,
    /// Reads the shared waveform texture as `wave`.
    pub audio: bool,
    /// Keeps its first input's last [`crate::gpu::HISTORY`] frames, read with `past_frame`.
    pub history: bool,
    /// Pictures it keeps from frame to frame, each drawn by a pass of its own (`buffer_<id>`)
    /// before the picture itself, and read by its id in every pass.
    pub buffers: Vec<Buffer>,
    /// A particle system ([`crate::gpu`]): instead of `main_image`, the file has the entry points
    /// `velo_emit` and `velo_step` (compute) and `velo_vs`, `velo_fs` (a sprite per particle), and
    /// calls `velo_load()` to read its parameters.
    pub particles: bool,
    /// The whole module: preamble, the file as written, entry points.
    pub wgsl: String,
    /// Lines before the file's own first line, to report errors at the author's line numbers.
    preamble_lines: usize,
}

/// A picture a node keeps: `scale` of the canvas each way, drawn again each frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Buffer {
    pub id: String,
    pub scale: f32,
}

/// The most buffers a node keeps.
pub const MAX_BUFFERS: usize = 4;

/// What went wrong, at the line of the author's file when there is one.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

fn error(message: impl Into<String>) -> ShaderError {
    ShaderError { line: None, message: message.into() }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    params: Vec<HeaderParam>,
    #[serde(default)]
    inputs: Vec<HeaderInput>,
    #[serde(default)]
    feedback: bool,
    #[serde(default)]
    audio: bool,
    #[serde(default)]
    history: bool,
    #[serde(default)]
    particles: bool,
    #[serde(default)]
    buffers: Vec<HeaderBuffer>,
    /// "manual": the shader honours `amount` itself instead of Velo mixing with the input;
    /// "none": an effect with no amount knob (Opacity: its own knob is the amount).
    #[serde(default)]
    amount: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderParam {
    id: String,
    #[serde(rename = "type")]
    ty: ParamType,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    min: Option<f32>,
    #[serde(default)]
    max: Option<f32>,
    /// An int's values by name, from 0.
    #[serde(default)]
    names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderBuffer {
    id: String,
    /// Of the canvas each way, 0.0625 to 1 (1 when missing).
    #[serde(default)]
    scale: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderInput {
    id: String,
    #[serde(rename = "type", default)]
    _ty: Option<String>,
    #[serde(rename = "optional", default)]
    _optional: bool,
}

/// Words the generated code uses: a parameter or input can't take them.
const RESERVED: [&str; 8] = ["g", "smp", "prev", "wave", "past_frame", "velo_params", "main_image", "uv"];

/// What a node with a history reads its first input's past frames with (gpu::Runner keeps them,
/// half the canvas each way): `n` frames back, 0 this one, between two frames for a fraction.
const PAST_FRAME: &str = "fn past_frame(uv: vec2f, n: f32) -> vec4f {
    let count = velo_past.y;
    let back = clamp(n, 0.0, count - 1.0);
    let a = floor(back);
    let near = textureSampleLevel(velo_history, smp, uv, i32((velo_past.x - a + count) % count), 0.0);
    let far = textureSampleLevel(velo_history, smp, uv, i32((velo_past.x - a - 1.0 + count * 2.0) % count), 0.0);
    return mix(near, far, back - a);
}
";

impl ShaderNode {
    /// Reads the header and wraps the file. Doesn't compile it: see [`ShaderNode::validate`].
    pub fn parse(source: &str) -> Result<ShaderNode, ShaderError> {
        let body = source.trim_start();
        let json = body.strip_prefix("/*").and_then(|rest| rest.split_once("*/")).map(|(json, _)| json).ok_or_else(|| error("the file must start with a /*{ … }*/ JSON header"))?;
        let header: Header = serde_json::from_str(json.trim()).map_err(|e| error(format!("header: {e}")))?;
        if !header.particles && !source.contains("fn main_image") {
            return Err(error("the file must define fn main_image(uv: vec2f, px: vec2f) -> vec4f"));
        }
        let mut params = Vec::new();
        for p in header.params {
            if RESERVED.contains(&p.id.as_str()) || p.id.starts_with("velo_") {
                return Err(error(format!("parameter id {:?} is reserved", p.id)));
            }
            let default = match (&p.default, p.ty) {
                (None, ParamType::Color) => [1.0; 4],
                (None, _) => [0.0; 4],
                (Some(Value::Bool(b)), _) => [f32::from(u8::from(*b)), 0.0, 0.0, 0.0],
                (Some(Value::Number(n)), _) => [n.as_f64().unwrap_or(0.0) as f32, 0.0, 0.0, 0.0],
                (Some(Value::Array(a)), _) => {
                    let mut v = [0.0, 0.0, 0.0, 1.0];
                    for (slot, x) in v.iter_mut().zip(a) {
                        *slot = x.as_f64().unwrap_or(0.0) as f32;
                    }
                    v
                }
                (Some(other), _) => return Err(error(format!("parameter {:?}: default {other} isn't a number, bool or list", p.id))),
            };
            // named values run from 0 to the last name
            let last = p.names.len().saturating_sub(1) as f32;
            let (min, max) = if p.names.is_empty() { (p.min.unwrap_or(0.0), p.max.unwrap_or(1.0)) } else { (0.0, last) };
            params.push(Param { id: p.id, ty: p.ty, default, min, max, names: p.names });
        }
        // effects get the amount contract (docs/graph.md): one knob, 0 = bypass
        let effect = header.category == "effect";
        let amount = effect && header.amount.as_deref() != Some("none");
        if amount && !params.iter().any(|p| p.id == "amount") {
            params.push(Param { id: "amount".into(), ty: ParamType::Float, default: [1.0, 0.0, 0.0, 0.0], min: 0.0, max: 1.0, names: Vec::new() });
        }
        // and pictures an opacity: how much of them shows (over black on their own, over what's
        // under them in a Blend); one that has its own (Text's) keeps it
        let fade = header.category == "generator" && !header.particles && !params.iter().any(|p| p.id == "opacity");
        if fade {
            params.push(Param { id: "opacity".into(), ty: ParamType::Float, default: [1.0, 0.0, 0.0, 0.0], min: 0.0, max: 1.0, names: Vec::new() });
        }
        let inputs: Vec<String> = header.inputs.into_iter().map(|i| i.id).collect();
        if effect && inputs.is_empty() {
            return Err(error("an effect needs at least one texture input"));
        }
        for id in &inputs {
            if RESERVED.contains(&id.as_str()) || params.iter().any(|p| &p.id == id) {
                return Err(error(format!("input id {id:?} is reserved or taken by a parameter")));
            }
        }
        let mix_input = (amount && header.amount.as_deref() != Some("manual")).then(|| inputs[0].clone());
        if header.particles && !header.buffers.is_empty() {
            return Err(error("a particle system has no buffers"));
        }
        if header.buffers.len() > MAX_BUFFERS {
            return Err(error(format!("a node keeps at most {MAX_BUFFERS} buffers")));
        }
        let mut buffers: Vec<Buffer> = Vec::new();
        for b in header.buffers {
            let scale = b.scale.unwrap_or(1.0);
            if !(0.0625..=1.0).contains(&scale) {
                return Err(error(format!("buffer {:?}: scale runs from 0.0625 to 1", b.id)));
            }
            if RESERVED.contains(&b.id.as_str()) || b.id.starts_with("velo_") || params.iter().any(|p| p.id == b.id) || inputs.contains(&b.id) || buffers.iter().any(|o| o.id == b.id) {
                return Err(error(format!("buffer id {:?} is reserved or taken", b.id)));
            }
            if !source.contains(&format!("fn buffer_{}", b.id)) {
                return Err(error(format!("the file must define fn buffer_{}(uv: vec2f, px: vec2f) -> vec4f", b.id)));
            }
            buffers.push(Buffer { id: b.id, scale });
        }

        let mut pre = String::from("// Velo: globals, parameters and bindings (docs/plugins.md)\nstruct VeloGlobals {\n    resolution: vec2<f32>,\n");
        for name in &GLOBALS[1..] {
            pre += &format!("    {name}: f32,\n");
        }
        pre += "}\n";
        pre += &format!("struct VeloParams {{\n    v: array<vec4<f32>, {}>,\n}}\n", params.len().max(1));
        pre += "@group(0) @binding(0) var<uniform> g: VeloGlobals;\n@group(0) @binding(1) var<uniform> velo_params: VeloParams;\n@group(0) @binding(2) var smp: sampler;\n";
        // textures from binding 3 on: inputs, buffers, previous frame, audio; then a history's frames and where it is
        let textures: Vec<&str> = inputs.iter().map(String::as_str).chain(buffers.iter().map(|b| b.id.as_str())).chain(header.feedback.then_some("prev")).chain(header.audio.then_some("wave")).collect();
        for (binding, id) in (3..).zip(&textures) {
            pre += &format!("@group(0) @binding({binding}) var {id}: texture_2d<f32>;\n");
        }
        let history = header.history && !header.particles && !inputs.is_empty();
        if history {
            let at = 3 + textures.len();
            pre += &format!("@group(0) @binding({at}) var velo_history: texture_2d_array<f32>;\n@group(0) @binding({}) var<uniform> velo_past: vec4<f32>;\n{PAST_FRAME}", at + 1);
        }
        for p in &params {
            pre += &format!("var<private> {}: {};\n", p.id, p.ty.wgsl());
        }
        let preamble_lines = pre.lines().count();
        let load: String = params.iter().enumerate().map(|(slot, p)| format!("    {} = {};\n", p.id, p.ty.read(slot))).collect();
        if header.particles {
            let wgsl = format!("{pre}{source}\n// Velo: the parameters\nfn velo_load() {{\n{load}}}\n");
            return Ok(ShaderNode { name: header.name, category: header.category, description: header.description, params, inputs, feedback: false, audio: false, history: false, buffers: Vec::new(), particles: true, wgsl, preamble_lines });
        }

        let mut post = String::from("\n// Velo: entry points\nstruct VeloVertex {\n    @builtin(position) pos: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n}\n");
        post += "@vertex\nfn velo_vs(@builtin(vertex_index) i: u32) -> VeloVertex {\n    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));\n    return VeloVertex(vec4<f32>(p * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), p);\n}\n";
        post += "@fragment\nfn velo_fs(v: VeloVertex) -> @location(0) vec4<f32> {\n";
        post += &load;
        post += "    let c = main_image(v.uv, v.uv * g.resolution);\n";
        match &mix_input {
            Some(input) => post += &format!("    return mix(textureSample({input}, smp, v.uv), c, clamp(amount, 0.0, 1.0));\n}}\n"),
            None if fade => post += "    return c * clamp(opacity, 0.0, 1.0);\n}\n",
            None => post += "    return c;\n}\n",
        }
        // a buffer's pass: its function, with the parameters loaded as in the picture's
        for b in &buffers {
            post += &format!("@fragment\nfn velo_fs_{id}(v: VeloVertex) -> @location(0) vec4<f32> {{\n{load}    return buffer_{id}(v.uv, v.uv * g.resolution);\n}}\n", id = b.id);
        }
        let wgsl = format!("{pre}{source}{post}");
        Ok(ShaderNode { name: header.name, category: header.category, description: header.description, params, inputs, feedback: header.feedback, audio: header.audio, history, buffers, particles: false, wgsl, preamble_lines })
    }

    /// A node whose whole module was written elsewhere (an ISF shader, translated by naga):
    /// it has Velo's bindings and entry points already.
    pub(crate) fn assembled(name: String, category: String, description: String, params: Vec<Param>, inputs: Vec<String>, wgsl: String) -> Self {
        ShaderNode { name, category, description, params, inputs, feedback: false, audio: false, history: false, buffers: Vec::new(), particles: false, wgsl, preamble_lines: 0 }
    }

    /// Compiles the module with naga, as wgpu will: syntax, types, uniformity. Errors come back at
    /// the author's line numbers.
    pub fn validate(&self) -> Result<naga::Module, ShaderError> {
        let at = |line: Option<usize>| line.map(|l| l.saturating_sub(self.preamble_lines)).filter(|&l| l > 0);
        let module = naga::front::wgsl::parse_str(&self.wgsl).map_err(|e| ShaderError { line: at(e.location(&self.wgsl).map(|l| l.line_number as usize)), message: e.message().to_string() })?;
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .map_err(|e| ShaderError { line: at(e.location(&self.wgsl).map(|l| l.line_number as usize)), message: e.as_inner().to_string() })?;
        Ok(module)
    }

    /// One vec4 per parameter, in order: `set` when it has the parameter's id, else its default.
    pub fn values(&self, set: impl Fn(&str) -> Option<[f32; 4]>) -> Vec<[f32; 4]> {
        self.params.iter().map(|p| set(&p.id).unwrap_or(p.default)).collect()
    }

    /// The parameter buffer for these values (one vec4 per parameter, in order).
    pub fn param_bytes(&self, values: &[[f32; 4]]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.params.len().max(1) * 16);
        for i in 0..self.params.len().max(1) {
            let v = values.get(i).or(self.params.get(i).map(|p| &p.default)).copied().unwrap_or([0.0; 4]);
            out.extend(v.iter().flat_map(|x| x.to_ne_bytes()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_compiles() {
        for (id, source) in BUILTINS {
            let node = ShaderNode::parse(source).unwrap_or_else(|e| panic!("{id}: {e}"));
            if let Err(e) = node.validate() {
                panic!("{id}: {e}\n{}", node.wgsl);
            }
        }
    }

    #[test]
    fn audio_is_optional_and_bound_after_inputs_and_feedback() {
        for audio in [false, true] {
            for feedback in [false, true] {
                for inputs in [vec![], vec![serde_json::json!({ "id": "src" }), serde_json::json!({ "id": "other" })]] {
                    let header = serde_json::json!({ "name": "Audio", "audio": audio, "feedback": feedback, "inputs": inputs });
                    let body = if audio { "textureLoad(wave, vec2i(0, 0), 0)" } else { "vec4f(0.0)" };
                    let node = ShaderNode::parse(&format!("/*{header}*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return {body}; }}")).unwrap();
                    assert_eq!(node.audio, audio);
                    let module = node.validate().unwrap();
                    let binding = module.global_variables.iter().find(|(_, v)| v.name.as_deref() == Some("wave")).and_then(|(_, v)| v.binding.as_ref());
                    assert_eq!(binding.map(|b| (b.group, b.binding)), audio.then_some((0, 3 + inputs.len() as u32 + u32::from(feedback))));
                }
            }
        }
        let node = ShaderNode::parse("/*{ \"name\": \"No audio\" }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(0.0); }").unwrap();
        assert!(!node.audio);
    }

    #[test]
    fn buffers_become_passes_and_bindings_and_are_checked() {
        let one = |id: &str| format!("fn buffer_{id}(uv: vec2f, px: vec2f) -> vec4f {{ return textureSampleLevel(b, smp, uv, 0.0); }}\n");
        let file = |header: &str, ids: &[&str]| ShaderNode::parse(&format!("/*{header}*/\n{}fn main_image(uv: vec2f, px: vec2f) -> vec4f {{ return vec4f(0.0); }}", ids.iter().map(|id| one(id)).collect::<String>()));
        let node = file(r#"{ "name": "B", "inputs": [{ "id": "src" }], "buffers": [{ "id": "a" }, { "id": "b", "scale": 0.5 }] }"#, &["a", "b"]).unwrap();
        assert_eq!(node.buffers, [Buffer { id: "a".into(), scale: 1.0 }, Buffer { id: "b".into(), scale: 0.5 }]);
        let module = node.validate().unwrap();
        // textures after the inputs, in order; an entry point a buffer
        let binding = |name: &str| module.global_variables.iter().find(|(_, v)| v.name.as_deref() == Some(name)).and_then(|(_, v)| v.binding.as_ref()).map(|b| b.binding);
        assert_eq!((binding("src"), binding("a"), binding("b")), (Some(3), Some(4), Some(5)));
        assert!(node.wgsl.contains("fn velo_fs_a") && node.wgsl.contains("fn velo_fs_b"));
        // mistakes are named
        let bad = |header: &str, ids: &[&str]| file(header, ids).unwrap_err().message;
        assert!(bad(r#"{ "name": "B", "buffers": [{ "id": "a" }] }"#, &[]).contains("fn buffer_a"));
        assert!(bad(r#"{ "name": "B", "buffers": [{ "id": "a" }, { "id": "a" }] }"#, &["a"]).contains("taken"));
        assert!(bad(r#"{ "name": "B", "buffers": [{ "id": "prev" }] }"#, &["prev"]).contains("reserved"));
        assert!(bad(r#"{ "name": "B", "buffers": [{ "id": "a", "scale": 2.0 }] }"#, &["a"]).contains("scale"));
        let five = r#"{ "name": "B", "buffers": [{ "id": "a" }, { "id": "b" }, { "id": "c" }, { "id": "d" }, { "id": "e" }] }"#;
        assert!(bad(five, &["a", "b", "c", "d", "e"]).contains("at most"));
    }

    #[test]
    fn the_header_becomes_parameters_and_bindings() {
        let node = ShaderNode::parse(
            r#"/*{
  "name": "Tunnel",
  "category": "generator",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "tint", "type": "color", "default": [1.0, 0.35, 0.1, 1.0] },
    { "id": "rings", "type": "int", "default": 6, "min": 1, "max": 32 }
  ]
}*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let r = length(uv - 0.5) * f32(rings);
    return vec4f(tint.rgb * (0.5 + 0.5 * sin(r * 6.0 - g.time * speed + g.kick)), 1.0);
}
"#,
        )
        .unwrap();
        // its three, and the opacity every picture gets
        assert_eq!((node.name.as_str(), node.params.len(), node.inputs.len()), ("Tunnel", 4, 0));
        assert_eq!(node.params[1].default, [1.0, 0.35, 0.1, 1.0]);
        assert_eq!((node.params[0].min, node.params[0].max), (0.0, 2.0));
        node.validate().unwrap();
        assert_eq!(node.param_bytes(&[]).len(), 4 * 16);
    }

    #[test]
    fn pictures_get_an_opacity_unless_they_have_their_own() {
        let node = ShaderNode::parse("/*{ \"name\": \"Red\", \"category\": \"generator\" }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {\n    return vec4f(1.0, 0.0, 0.0, 1.0);\n}\n").unwrap();
        assert_eq!(node.params.last().map(|p| (p.id.as_str(), p.default[0])), Some(("opacity", 1.0)));
        assert!(node.wgsl.contains("return c * clamp(opacity, 0.0, 1.0);"));
        node.validate().unwrap();
        let text = ShaderNode::parse(BUILTINS.iter().find(|b| b.0 == "text").unwrap().1).unwrap();
        assert_eq!(text.params.iter().filter(|p| p.id == "opacity").count(), 1, "Text keeps its own");
        let opacity = ShaderNode::parse(BUILTINS.iter().find(|b| b.0 == "opacity").unwrap().1).unwrap();
        assert!(!opacity.params.iter().any(|p| p.id == "amount"), "the Opacity node's knob is its amount");
    }

    #[test]
    fn effects_get_an_amount_knob_that_bypasses_at_zero() {
        let node = ShaderNode::parse(
            r#"/*{ "name": "Invert", "category": "effect", "inputs": [{ "id": "src" }] }*/
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = textureSample(src, smp, uv);
    return vec4f(1.0 - c.rgb, c.a);
}"#,
        )
        .unwrap();
        assert_eq!(node.params.last().map(|p| p.id.as_str()), Some("amount"));
        assert!(node.wgsl.contains("mix(textureSample(src, smp, v.uv), c, clamp(amount, 0.0, 1.0))"));
        node.validate().unwrap();
    }

    #[test]
    fn errors_point_at_the_authors_line() {
        let node = ShaderNode::parse("/*{ \"name\": \"Oops\" }*/\nfn main_image(uv: vec2f, px: vec2f) -> vec4f {\n    let x = 1.0;\n    return vec4f(x, y, 0.0, 1.0);\n}\n").unwrap();
        let e = node.validate().unwrap_err();
        assert_eq!(e.line, Some(4), "{e}");
        assert!(e.message.contains('y') || e.message.contains("identifier"), "{e}");
    }

    #[test]
    fn bad_headers_are_explained() {
        assert!(ShaderNode::parse("fn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(1.0); }").unwrap_err().message.contains("JSON header"));
        assert!(ShaderNode::parse("/*{ \"name\": \"X\" }*/ fn other() {}").unwrap_err().message.contains("main_image"));
        assert!(ShaderNode::parse("/*{ \"name\": \"X\", \"params\": [{ \"id\": \"g\", \"type\": \"float\" }] }*/ fn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(1.0); }").unwrap_err().message.contains("reserved"));
        assert!(ShaderNode::parse("/*{ \"name\": \"X\", \"colour\": 1 }*/ fn main_image(uv: vec2f, px: vec2f) -> vec4f { return vec4f(1.0); }").unwrap_err().message.contains("unknown field"));
    }
}
