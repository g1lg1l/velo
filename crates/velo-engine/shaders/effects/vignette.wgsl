/*{
  "name": "Vignette",
  "category": "effect",
  "description": "The picture's edges darkened (or tinted) toward a color, round or the picture's shape; the kick closes it in.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 0.75, "min": 0.1, "max": 1.5 },
    { "id": "softness", "type": "float", "default": 0.6, "min": 0.01, "max": 1.0 },
    { "id": "round", "type": "bool", "default": true },
    { "id": "color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    let q = uv - 0.5;
    // 1 at the middle of the left and right edges: round, or the picture's shape
    let d = select(length(q * 2.0), length(q * vec2f(g.aspect, 1.0)) * 2.0 / g.aspect, round);
    let r = size * (1.0 - 0.2 * music * g.kick);
    let k = smoothstep(r, r + softness, d) * color.a;
    return vec4f(mix(s.rgb, color.rgb * s.a, k), s.a);
}
