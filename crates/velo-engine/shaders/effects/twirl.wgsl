/*{
  "name": "Twirl",
  "category": "effect",
  "description": "The picture twisted round a point, most at the middle and not at all past the radius; the kick twists it further.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "turns", "type": "float", "default": 0.5, "min": -3.0, "max": 3.0 },
    { "id": "radius", "type": "float", "default": 0.45, "min": 0.05, "max": 1.5 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = 0.5 + center * 0.5;
    let p = (uv - c) * vec2f(g.aspect, 1.0);
    let k = max(1.0 - length(p) / radius, 0.0);
    let a = (turns + 0.25 * music * g.kick) * 6.2831853 * k * k;
    let q = vec2f(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a));
    return textureSampleLevel(src, smp, q / vec2f(g.aspect, 1.0) + c, 0.0);
}
