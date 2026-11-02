/*{
  "name": "Gradient Map",
  "category": "effect",
  "description": "The picture recolored by its brightness through three colors, shadows to highlights (duotone, thermal, night vision); the bass slides them along.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "dark", "type": "color", "default": [0.1, 0.0, 0.3, 1.0] },
    { "id": "mid", "type": "color", "default": [0.95, 0.2, 0.45, 1.0] },
    { "id": "light", "type": "color", "default": [1.0, 0.9, 0.5, 1.0] },
    { "id": "contrast", "type": "float", "default": 1.0, "min": 0.25, "max": 3.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    let v = dot(s.rgb / max(s.a, 0.0001), vec3f(0.2126, 0.7152, 0.0722));
    let t = clamp((v - 0.5) * contrast + 0.5 + 0.25 * music * g.bass, 0.0, 1.0);
    let c = select(mix(mid.rgb, light.rgb, t * 2.0 - 1.0), mix(dark.rgb, mid.rgb, t * 2.0), t < 0.5);
    return vec4f(c * s.a, s.a);
}
