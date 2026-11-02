/*{
  "name": "Chromatic Aberration",
  "category": "effect",
  "description": "Separate red and blue along a direction or away from the center with bass accents.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "spread", "type": "float", "default": 4.0, "min": 0.0, "max": 32.0 },
    { "id": "angle", "type": "float", "default": 0.0, "min": -3.14159, "max": 3.14159 },
    { "id": "radial", "type": "float", "default": 0.7, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * 2.0;
    let rotated = vec2f(p.x * cos(angle) - p.y * sin(angle), p.x * sin(angle) + p.y * cos(angle));
    let direction = mix(vec2f(cos(angle), sin(angle)), rotated, radial);
    let offset = direction * spread * (1.0 + music * g.bass) / g.resolution;
    let red = textureSample(src, smp, uv + offset).r;
    let blue = textureSample(src, smp, uv - offset).b;
    return vec4f(red, original.g, blue, original.a);
}
