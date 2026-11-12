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
    // the three guns never quite meet toward the edges, less still when the kick shakes them
    let split = (q - 0.5) * (0.004 + 0.012 * music * g.kick) * dot(p, p);
    let color = vec3f(textureSample(src, smp, q + split).r, textureSample(src, smp, q).g, textureSample(src, smp, q - split).b);
    let inside = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    let lines = 1.0 - scanlines * (0.5 + 0.5 * sin(px.y * 3.14159265)) * 0.65;
    let corners = 1.0 - vignette * smoothstep(0.2, 1.6, dot(p, p));
    // a brighter band rolling slowly down the tube; the bass makes the whole picture breathe
    let roll = 1.0 + 0.06 * scanlines * smoothstep(0.85, 1.0, sin(uv.y * 3.0 - g.time * 0.7));
    let pulse = 1.0 + 0.3 * music * g.bass;
    return vec4f(color * lines * corners * roll * pulse * inside, original.a);
}
