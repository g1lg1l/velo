/*{
  "name": "Fisheye",
  "category": "effect",
  "description": "A lens bulging the middle out (or pinching it in, below zero), its edge dark or see-through; the bass bulges it more.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "bulge", "type": "float", "default": 0.5, "min": -1.0, "max": 1.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.5, "max": 2.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // 1 at the picture's nearer edges
    let r = length(p) * 2.0;
    let k = bulge + 0.3 * music * g.bass;
    // r bent by a power: the middle magnified when k > 0
    let bent = pow(r, 1.0 + k) / max(r, 0.0001);
    let q = p * mix(1.0, bent, step(0.0001, r)) / zoom / vec2f(g.aspect, 1.0) + 0.5;
    let inside = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    return textureSampleLevel(src, smp, q, 0.0) * inside;
}
