/*{
  "name": "CRT",
  "category": "effect",
  "description": "A curved tube with scanlines and dark corners flickers gently with the bass.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "curvature", "type": "float", "default": 0.12, "min": 0.0, "max": 0.4 },
    { "id": "scanlines", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "vignette", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let p = uv * 2.0 - 1.0;
    let q = p * (1.0 + curvature * p.yx * p.yx) * 0.5 + 0.5;
    let color = textureSample(src, smp, q).rgb;
    let inside = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    let lines = 1.0 - scanlines * (0.5 + 0.5 * sin(px.y * 3.14159265)) * 0.65;
    let corners = 1.0 - vignette * smoothstep(0.2, 1.6, dot(p, p));
    let pulse = 1.0 + 0.12 * music * g.bass;
    return vec4f(color * lines * corners * pulse * inside, original.a);
}
