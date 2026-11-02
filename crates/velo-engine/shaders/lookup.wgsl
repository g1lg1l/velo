/*{
  "name": "Lookup",
  "category": "effect",
  "description": "The picture recolored by a second one, its palette: each pixel's brightness picks a color along it from left (dark) to right (light); wire a Gradient, an image or anything into it",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "palette", "type": "texture" }],
  "params": [
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "offset", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "repeat", "type": "int", "default": 1, "min": 1, "max": 8 },
    { "id": "row", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    let v = dot(s.rgb / max(s.a, 0.0001), vec3f(0.2126, 0.7152, 0.0722));
    // along the palette and back as it repeats or slides (the bass slides it), so no seam
    let x = v * f32(repeat) + offset + 0.25 * music * g.bass;
    let c = textureSampleLevel(palette, smp, vec2f(1.0 - abs(fract(x * 0.5) * 2.0 - 1.0), row), 0.0);
    return vec4f(c.rgb * s.a, s.a);
}
