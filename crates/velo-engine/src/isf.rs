//! ISF shaders (isf.video: the Interactive Shader Format of VDMX, Resolume, Synesthesia and a
//! large library online) as Velo Shader nodes. An ISF file is a GLSL fragment shader under a
//! JSON header of inputs; Velo wraps it in GLSL 4.50 that binds what Velo binds (its globals,
//! its parameters, the sampler, the input textures) and defines ISF's names (TIME, RENDERSIZE,
//! isf_FragNormCoord, IMG_NORM_PIXEL…, gl_FragColor), lets naga translate that to WGSL, and adds
//! Velo's vertex stage: from there it's a node like any other. One pass, one image input for
//! now; audio inputs and persistent buffers aren't read yet.

use serde_json::Value;

use crate::GLOBALS;
use crate::shader::{Param, ParamType, ShaderError, ShaderNode};

/// Whether `code` is an ISF file: a JSON header with ISF's own (upper-case) keys, a GLSL main.
pub fn is_isf(code: &str) -> bool {
    let header = code.trim_start().strip_prefix("/*").and_then(|r| r.split_once("*/")).map(|(h, _)| h);
    header.is_some_and(|h| ["\"INPUTS\"", "\"ISFVSN\"", "\"CATEGORIES\"", "\"DESCRIPTION\""].iter().any(|k| h.contains(k))) && code.contains("void main")
}

fn error(message: impl Into<String>) -> ShaderError {
    ShaderError { line: None, message: message.into() }
}

/// What GLSL type an ISF input is, and how its value comes out of its parameter's vec4.
fn glsl(ty: ParamType, slot: usize) -> (&'static str, String) {
    let v = format!("velo_params.v[{slot}]");
    match ty {
        ParamType::Float => ("float", format!("{v}.x")),
        ParamType::Int => ("int", format!("int({v}.x)")),
        ParamType::Bool => ("bool", format!("{v}.x > 0.5")),
        ParamType::Color | ParamType::Vec4 => ("vec4", v),
        ParamType::Vec2 => ("vec2", format!("{v}.xy")),
    }
}

fn number(v: Option<&Value>) -> Option<f32> {
    v.and_then(Value::as_f64).map(|x| x as f32)
}

/// A list's numbers (a color, a point), padded.
fn list(v: Option<&Value>, pad: f32) -> Option<[f32; 4]> {
    let a = v?.as_array()?;
    let mut out = [0.0, 0.0, 0.0, pad];
    for (slot, x) in out.iter_mut().zip(a) {
        *slot = x.as_f64()? as f32;
    }
    Some(out)
}

/// An ISF file as a Shader node.
pub fn parse(code: &str) -> Result<ShaderNode, ShaderError> {
    let json = code.trim_start().strip_prefix("/*").and_then(|r| r.split_once("*/")).map(|(h, _)| h).ok_or_else(|| error("an ISF file starts with its /*{ … }*/ JSON header"))?;
    let header: Value = serde_json::from_str(json.trim()).map_err(|e| error(format!("ISF header: {e}")))?;
    if header["PASSES"].as_array().is_some_and(|p| p.len() > 1 || p.iter().any(|p| p.get("TARGET").is_some())) || header.get("PERSISTENT_BUFFERS").is_some() {
        return Err(error("this ISF shader renders in several passes or keeps buffers: Velo reads one pass for now"));
    }
    let (mut params, mut inputs) = (Vec::new(), Vec::new());
    for input in header["INPUTS"].as_array().into_iter().flatten() {
        let name = input["NAME"].as_str().ok_or_else(|| error("an ISF input without a NAME"))?.to_string();
        if name.starts_with("velo") || ["main", "isf_main", "TIME", "RENDERSIZE"].contains(&name.as_str()) {
            return Err(error(format!("the input name {name:?} is reserved")));
        }
        let (ty, default, min, max) = match input["TYPE"].as_str().unwrap_or("") {
            "float" => (ParamType::Float, [number(input.get("DEFAULT")).unwrap_or(0.0), 0.0, 0.0, 0.0], number(input.get("MIN")).unwrap_or(0.0), number(input.get("MAX")).unwrap_or(1.0)),
            "bool" | "event" => (ParamType::Bool, [number(input.get("DEFAULT")).or_else(|| input["DEFAULT"].as_bool().map(|b| f32::from(u8::from(b)))).unwrap_or(0.0), 0.0, 0.0, 0.0], 0.0, 1.0),
            "long" => {
                let values: Vec<f32> = input["VALUES"].as_array().into_iter().flatten().filter_map(|v| v.as_f64()).map(|v| v as f32).collect();
                let lo = values.iter().copied().reduce(f32::min).or(number(input.get("MIN"))).unwrap_or(0.0);
                let hi = values.iter().copied().reduce(f32::max).or(number(input.get("MAX"))).unwrap_or(1.0);
                (ParamType::Int, [number(input.get("DEFAULT")).unwrap_or(lo), 0.0, 0.0, 0.0], lo, hi)
            }
            "color" => (ParamType::Color, list(input.get("DEFAULT"), 1.0).unwrap_or([1.0; 4]), 0.0, 1.0),
            "point2D" => {
                let (lo, hi) = (list(input.get("MIN"), 0.0), list(input.get("MAX"), 0.0));
                (ParamType::Vec2, list(input.get("DEFAULT"), 0.0).unwrap_or([0.0; 4]), lo.map_or(0.0, |v| v[0].min(v[1])), hi.map_or(1.0, |v| v[0].max(v[1])))
            }
            "image" => {
                inputs.push(name);
                continue;
            }
            other => return Err(error(format!("the ISF input {name:?} is of type {other:?}, which Velo doesn't read yet"))),
        };
        // a menu's LABELS name its VALUES when they count from 0
        let labels: Vec<String> = input["LABELS"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect();
        let counted = input["VALUES"].as_array().is_some_and(|v| v.iter().enumerate().all(|(i, x)| x.as_f64() == Some(i as f64)) && v.len() == labels.len());
        let names = if ty == ParamType::Int && counted { labels } else { Vec::new() };
        params.push(Param { id: name, ty, default, min, max, names });
    }
    if inputs.len() > 1 {
        return Err(error("this ISF shader takes several images: a Shader node takes one for now"));
    }
    // an image in makes it an effect, with Velo's amount (0 passes the input through)
    let effect = !inputs.is_empty();
    if effect {
        params.push(Param { id: "velo_amount".into(), ty: ParamType::Float, default: [1.0, 0.0, 0.0, 0.0], min: 0.0, max: 1.0, names: Vec::new() });
    }

    let mut pre = String::from("#version 450\n// Velo: an ISF shader's surroundings (crates/velo-engine/src/isf.rs)\n");
    pre += "layout(set = 0, binding = 0) uniform VeloGlobals { vec2 resolution;";
    for name in &GLOBALS[1..] {
        pre += &format!(" float {name};");
    }
    pre += " } velo_g;\n";
    pre += &format!("layout(set = 0, binding = 1) uniform VeloParams {{ vec4 v[{}]; }} velo_params;\n", params.len().max(1));
    pre += "layout(set = 0, binding = 2) uniform sampler velo_smp;\n";
    for (binding, name) in (3..).zip(&inputs) {
        pre += &format!("layout(set = 0, binding = {binding}) uniform texture2D {name};\n");
    }
    pre += "layout(location = 0) in vec2 velo_uv;\nlayout(location = 0) out vec4 velo_out;\n";
    pre += "vec2 velo_isf_coord;\nvec4 velo_fragcoord;\nvec2 velo_flip(vec2 c) { return vec2(c.x, 1.0 - c.y); }\n";
    // ISF's names: its coordinates start at the bottom left, Velo's at the top left
    for (name, value) in [
        ("TIME", "velo_g.time"),
        ("TIMEDELTA", "velo_g.delta"),
        ("FRAMEINDEX", "int(velo_g.frame)"),
        ("RENDERSIZE", "velo_g.resolution"),
        ("PASSINDEX", "0"),
        ("DATE", "vec4(0.0)"),
        ("gl_FragColor", "velo_out"),
        ("gl_FragCoord", "velo_fragcoord"),
        ("isf_FragNormCoord", "velo_isf_coord"),
        ("vv_FragNormCoord", "velo_isf_coord"),
        ("texture2D(t, c)", "texture(sampler2D(t, velo_smp), velo_flip(c))"),
        ("IMG_NORM_PIXEL(t, c)", "texture(sampler2D(t, velo_smp), velo_flip(c))"),
        ("IMG_PIXEL(t, c)", "texture(sampler2D(t, velo_smp), velo_flip((c) / vec2(textureSize(sampler2D(t, velo_smp), 0))))"),
        ("IMG_THIS_NORM_PIXEL(t)", "texture(sampler2D(t, velo_smp), velo_flip(velo_isf_coord))"),
        ("IMG_THIS_PIXEL(t)", "texture(sampler2D(t, velo_smp), velo_flip(velo_isf_coord))"),
        ("IMG_SIZE(t)", "vec2(textureSize(sampler2D(t, velo_smp), 0))"),
    ] {
        pre += &format!("#define {name} {value}\n");
    }
    for (slot, p) in params.iter().enumerate() {
        pre += &format!("{} {};\n", glsl(p.ty, slot).0, p.id);
    }
    pre += "#define main isf_main\n";
    let preamble = pre.lines().count();
    let mut post = String::from("\n#undef main\nvoid main() {\n    velo_isf_coord = vec2(velo_uv.x, 1.0 - velo_uv.y);\n    velo_fragcoord = vec4(velo_isf_coord * velo_g.resolution, 0.0, 1.0);\n");
    for (slot, p) in params.iter().enumerate() {
        post += &format!("    {} = {};\n", p.id, glsl(p.ty, slot).1);
    }
    post += "    isf_main();\n";
    if let Some(image) = inputs.first() {
        post += &format!("    velo_out = mix(texture(sampler2D({image}, velo_smp), velo_uv), velo_out, clamp(velo_amount, 0.0, 1.0));\n");
    }
    post += "}\n";
    let glsl = format!("{pre}{code}{post}");

    // the author's line, from a span in the wrapped source
    let line = |span: naga::Span| Some(span.location(&glsl).line_number as usize).map(|l| l.saturating_sub(preamble)).filter(|&l| l > 0);
    let mut module = naga::front::glsl::Frontend::default().parse(&naga::front::glsl::Options::from(naga::ShaderStage::Fragment), &glsl).map_err(|errors| {
        let first = errors.errors.first();
        ShaderError { line: first.and_then(|e| line(e.meta)), message: first.map_or_else(|| "the GLSL didn't parse".to_string(), |e| e.kind.to_string()) }
    })?;
    for entry in &mut module.entry_points {
        entry.name = "velo_fs".into();
    }
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|e| ShaderError { line: e.spans().next().and_then(|(span, _)| line(*span)), message: e.as_inner().to_string() })?;
    let mut wgsl = naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty()).map_err(|e| error(e.to_string()))?;
    wgsl += "\n// Velo: the vertex stage\nstruct VeloVertex {\n    @builtin(position) pos: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n}\n";
    wgsl +=
        "@vertex\nfn velo_vs(@builtin(vertex_index) i: u32) -> VeloVertex {\n    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));\n    return VeloVertex(vec4<f32>(p * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), p);\n}\n";
    // what the inspector shows: the amount as "amount"
    if let Some(p) = params.iter_mut().find(|p| p.id == "velo_amount") {
        p.id = "amount".into();
    }
    let description = header["DESCRIPTION"].as_str().unwrap_or_default().to_string();
    Ok(ShaderNode::assembled("ISF shader".into(), if effect { "effect" } else { "generator" }.into(), description, params, inputs, wgsl))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A generator in ISF's usual style: inputs of every kind, TIME, RENDERSIZE, gl_FragCoord.
    pub const RINGS: &str = r#"/*{
    "DESCRIPTION": "Rings from a point, tinted",
    "CREDIT": "Velo tests",
    "ISFVSN": "2",
    "CATEGORIES": ["Generator"],
    "INPUTS": [
        { "NAME": "speed", "TYPE": "float", "DEFAULT": 1.0, "MIN": 0.0, "MAX": 4.0 },
        { "NAME": "count", "TYPE": "long", "VALUES": [2, 4, 8], "LABELS": ["2", "4", "8"], "DEFAULT": 4 },
        { "NAME": "invert", "TYPE": "bool", "DEFAULT": false },
        { "NAME": "tint", "TYPE": "color", "DEFAULT": [1.0, 0.2, 0.6, 1.0] },
        { "NAME": "center", "TYPE": "point2D", "DEFAULT": [0.5, 0.5], "MIN": [0.0, 0.0], "MAX": [1.0, 1.0] }
    ]
}*/

void main() {
    vec2 p = gl_FragCoord.xy / RENDERSIZE - center;
    float r = fract(length(p) * float(count) - TIME * speed);
    float v = smoothstep(0.4, 0.5, r) - smoothstep(0.5, 0.6, r);
    if (invert) v = 1.0 - v;
    gl_FragColor = vec4(tint.rgb * v, 1.0);
}
"#;

    /// An effect: its image through IMG_THIS_PIXEL and IMG_NORM_PIXEL.
    pub const SHIFT: &str = r#"/*{
    "DESCRIPTION": "Splits the colors apart",
    "ISFVSN": "2",
    "INPUTS": [
        { "NAME": "inputImage", "TYPE": "image" },
        { "NAME": "shift", "TYPE": "float", "DEFAULT": 0.01, "MIN": 0.0, "MAX": 0.1 }
    ]
}*/

void main() {
    vec4 c = IMG_THIS_PIXEL(inputImage);
    float r = IMG_NORM_PIXEL(inputImage, isf_FragNormCoord + vec2(shift, 0.0)).r;
    gl_FragColor = vec4(r, c.g, c.b, c.a);
}
"#;

    #[test]
    fn isf_generators_and_effects_become_nodes() {
        assert!(is_isf(RINGS) && is_isf(SHIFT));
        assert!(!is_isf(crate::compile::TEMPLATE), "Velo's own format isn't ISF");
        let rings = parse(RINGS).unwrap_or_else(|e| panic!("rings: {e}"));
        assert_eq!(rings.category, "generator");
        let kinds: Vec<(&str, ParamType)> = rings.params.iter().map(|p| (p.id.as_str(), p.ty)).collect();
        assert_eq!(kinds, [("speed", ParamType::Float), ("count", ParamType::Int), ("invert", ParamType::Bool), ("tint", ParamType::Color), ("center", ParamType::Vec2)]);
        let count = &rings.params[1];
        assert_eq!((count.min, count.max, count.default[0]), (2.0, 8.0, 4.0), "a long's VALUES are its range");
        rings.validate().unwrap_or_else(|e| panic!("rings' WGSL: {e}"));
        let shift = parse(SHIFT).unwrap_or_else(|e| panic!("shift: {e}"));
        assert_eq!((shift.category.as_str(), shift.inputs.as_slice()), ("effect", ["inputImage".to_string()].as_slice()));
        assert!(shift.params.iter().any(|p| p.id == "amount"), "an effect gets Velo's amount");
        shift.validate().unwrap_or_else(|e| panic!("shift's WGSL: {e}"));
    }

    #[test]
    fn isf_mistakes_come_back_at_the_authors_line() {
        let broken = RINGS.replace("float v = smoothstep", "float v = smoothstepp");
        let e = parse(&broken).unwrap_err();
        let at = RINGS.lines().position(|l| l.contains("float v = smoothstep")).unwrap() + 1;
        assert_eq!(e.line, Some(at), "{e}");
        let several = SHIFT.replace(r#"{ "NAME": "shift""#, r#"{ "NAME": "other", "TYPE": "image" }, { "NAME": "shift""#);
        assert!(parse(&several).unwrap_err().message.contains("several images"));
    }
}
