/*{
  "name": "Mirror",
  "category": "effect",
  "description": "Reflect the left, top or upper left image with a gentle bass zoom.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "mode", "type": "int", "default": 0, "names": ["Left/right", "Top/bottom", "Quad"] },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 1.0, "max": 4.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    var q = (uv - 0.5) / (zoom * (1.0 + 0.08 * music * g.bass)) + 0.5;
    q.x = select(q.x, 0.5 - abs(q.x - 0.5), mode == 0 || mode == 2);
    q.y = select(q.y, 0.5 - abs(q.y - 0.5), mode == 1 || mode == 2);
    let color = textureSample(src, smp, q);
    return vec4f(color.rgb, original.a);
}
