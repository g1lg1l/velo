/*{
  "name": "Hex Mosaic",
  "category": "effect",
  "description": "The picture in hexagonal tiles, each its area's color, with thin grout between that the bass widens.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 20.0, "min": 4.0, "max": 120.0 },
    { "id": "grout", "type": "float", "default": 0.06, "min": 0.0, "max": 0.4 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    // in hexes of `size` pixels across: the nearer of two offset grids' centers
    let s = vec2f(1.0, 1.7320508);
    let p = px / size;
    let a = (floor(p / s) + 0.5) * s;
    let b = (floor((p - s * 0.5) / s) + 0.5) * s + s * 0.5;
    let center = select(b, a, length(p - a) < length(p - b));
    let at = center * size / g.resolution;
    // four taps round the middle: the tile's color
    let o = size * 0.25 / g.resolution;
    let color = (textureSampleLevel(src, smp, at + vec2f(o.x, 0.0), 0.0) + textureSampleLevel(src, smp, at - vec2f(o.x, 0.0), 0.0) + textureSampleLevel(src, smp, at + vec2f(0.0, o.y), 0.0) + textureSampleLevel(src, smp, at - vec2f(0.0, o.y), 0.0)).rgb * 0.25;
    // distance to the hex's edge: 0.5 at its flat sides
    let q = abs(p - center);
    let edge = max(dot(q, vec2f(0.5, 0.8660254)), q.x);
    let w = grout * (1.0 + music * g.bass) * 0.5;
    let aa = 1.0 / size;
    let tile = 1.0 - smoothstep(0.5 - w - aa, 0.5 - w + aa, edge);
    return vec4f(color * tile, original.a);
}
