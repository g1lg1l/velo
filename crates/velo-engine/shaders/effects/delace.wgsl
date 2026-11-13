/*{
  "name": "Delace",
  "category": "effect",
  "description": "Interlacing gone wrong: every other band of lines shifted sideways, as two fields of a moving picture woven back together; on the kick each band jumps a random way further.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "shift", "type": "float", "default": 0.03, "min": 0.0, "max": 0.2 },
    { "id": "lines", "type": "float", "default": 2.0, "min": 1.0, "max": 16.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's delace.wgsl (MIT, Zach Banks).

fn delace_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    // which field this band of `lines` rows is in: half a shift one way or the other
    let band = floor(px.y / max(lines, 1.0));
    let field = band - 2.0 * floor(band * 0.5) - 0.5;
    // on the kick, a random push for each band, new each quarter beat
    let jump = music * g.kick * delace_hash(vec2f(band, floor(g.beats * 4.0) % 256.0));
    let x = clamp(uv.x + field * shift * (1.0 + jump), 0.0, 1.0);
    let color = textureSampleLevel(src, smp, vec2f(x, uv.y), 0.0);
    return vec4f(color.rgb, original.a);
}
