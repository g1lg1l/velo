/*{
  "name": "Ghost",
  "category": "effect",
  "description": "The picture smoothed over time: what moves leaves a soft ghost of where it was and what holds still stays sharp; the kick snaps it back to the live picture.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "avg" }],
  "params": [
    { "id": "linger", "type": "float", "default": 0.8, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's lpf.wgsl (MIT, Zach Banks and Eric Van Albert).

// the input averaged over time; on the kick the average lets go of what it held
fn buffer_avg(uv: vec2f, px: vec2f) -> vec4f {
    let cur = textureSampleLevel(src, smp, uv, 0.0);
    // how much of the average stays each 60th of a second, so it's the same at any frame rate
    let keep = pow(mix(0.5, 0.985, linger), g.delta * 60.0) * (1.0 - music * g.kick);
    return mix(cur, textureSampleLevel(avg, smp, uv, 0.0), keep);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return clamp(textureSample(avg, smp, uv), vec4f(0.0), vec4f(1.0));
}
