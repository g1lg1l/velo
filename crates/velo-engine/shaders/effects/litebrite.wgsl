/*{
  "name": "Lite Brite",
  "category": "effect",
  "description": "The picture in round pegs packed on a hexagonal grid, each one color with dark between; the pegs swell on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 16.0, "min": 4.0, "max": 120.0 },
    { "id": "peg", "type": "float", "default": 0.8, "min": 0.2, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's litebrite.wgsl (MIT, Zach Banks).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    // pegs `size` pixels apart, packed in rows: the nearer of two offset grids' centers
    let s = vec2f(1.0, 1.7320508);
    let p = px / size;
    let a = (floor(p / s) + 0.5) * s;
    let b = (floor((p - s * 0.5) / s) + 0.5) * s + s * 0.5;
    let center = select(b, a, length(p - a) < length(p - b));
    let color = textureSampleLevel(src, smp, center * size / g.resolution, 0.0).rgb;
    // a disc as wide as `peg` of the space to the next, wider on the kick
    let r = 0.5 * min(peg * (1.0 + 0.25 * music * g.kick), 1.0);
    let aa = 1.0 / size;
    let lit = 1.0 - smoothstep(r - aa, r + aa, length(p - center));
    return vec4f(color * lit, original.a);
}
