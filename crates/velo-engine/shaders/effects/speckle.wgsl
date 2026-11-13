/*{
  "name": "Speckle",
  "category": "effect",
  "description": "The picture twinkling pixel by pixel: each pixel takes the live picture now and then, at random, and holds it, dimming, until its next turn; on the hats they dim faster.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "held" }],
  "params": [
    { "id": "twinkle", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's speckle.wgsl (MIT, Zach Banks).

// a random number from 0 to 1 for each pixel and frame (pcg3d)
fn speckle_hash(p: vec3u) -> f32 {
    var v = p * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v ^= v >> vec3u(16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return f32(v.x) / 4294967295.0;
}

// each pixel as it was last taken, dimming; a few take the live picture each frame
fn buffer_held(uv: vec2f, px: vec2f) -> vec4f {
    let frames = g.delta * 60.0;
    // how likely a pixel is to take the picture in a 60th of a second: less as twinkle goes up
    let chance = exp(-3.0 * twinkle) * 0.5;
    let turn = 1.0 - pow(1.0 - chance, frames);
    let fade = pow(exp(-0.05 * twinkle * (1.0 + 1.5 * music * g.hat)), frames);
    let r = speckle_hash(vec3u(vec2u(px), u32(g.frame)));
    return select(textureSampleLevel(held, smp, uv, 0.0) * fade, textureSampleLevel(src, smp, uv, 0.0), r < turn);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return textureSample(held, smp, uv);
}
