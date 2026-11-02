/*{
  "name": "Glitch",
  "category": "effect",
  "description": "Broken horizontal blocks and split colors jump harder on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "shift", "type": "float", "default": 0.04, "min": 0.0, "max": 0.3 },
    { "id": "blocks", "type": "float", "default": 24.0, "min": 2.0, "max": 100.0 },
    { "id": "split", "type": "float", "default": 3.0, "min": 0.0, "max": 24.0 },
    { "id": "rate", "type": "float", "default": 12.0, "min": 1.0, "max": 60.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn glitch_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(43.17, 91.73))) * 17431.37);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let pulse = music * g.kick;
    let block = vec2f(floor(uv.y * blocks), floor(g.time * rate));
    let block_mask = step(0.72 - 0.4 * pulse, glitch_hash(block));
    let offset = (glitch_hash(block + 7.0) - 0.5) * shift * (1.0 + 3.0 * pulse) * block_mask;
    let q = vec2f(fract(uv.x + offset), uv.y);
    let chroma = vec2f(split * (1.0 + 2.0 * pulse) / g.resolution.x, 0.0);
    let red = textureSample(src, smp, q + chroma).r;
    let center = textureSample(src, smp, q);
    let blue = textureSample(src, smp, q - chroma).b;
    return vec4f(red, center.g, blue, original.a);
}
