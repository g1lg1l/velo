/*{
  "name": "Motion",
  "category": "effect",
  "description": "Only what moves: whatever holds still fades to black and what changes shows in its own colors, memory setting how long a thing must stand still to go; the kick lifts the gain.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "slow" }],
  "params": [
    { "id": "memory", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "gain", "type": "float", "default": 4.0, "min": 0.5, "max": 16.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's hpf.wgsl (MIT, Zach Banks and Eric Van Albert).

// the input averaged over time: what stands still for long enough is all that's left in it
fn buffer_slow(uv: vec2f, px: vec2f) -> vec4f {
    let cur = textureSampleLevel(src, smp, uv, 0.0);
    // how much of the average stays each 60th of a second, so it's the same at any frame rate
    let keep = pow(mix(0.6, 0.97, memory), g.delta * 60.0);
    return mix(cur, textureSampleLevel(slow, smp, uv, 0.0), keep);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let cur = textureSample(src, smp, uv);
    // how far the picture is from its average, as one brightness: the picture shows that much
    let d = abs(cur.rgb - textureSample(slow, smp, uv).rgb);
    let moved = clamp(max(d.r, max(d.g, d.b)) * gain * (1.0 + 2.0 * music * g.kick), 0.0, 1.0);
    return vec4f(cur.rgb * moved, cur.a);
}
