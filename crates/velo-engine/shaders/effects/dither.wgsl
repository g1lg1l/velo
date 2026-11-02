/*{
  "name": "Dither",
  "category": "effect",
  "description": "A four by four Bayer pattern reduces colors with a small musical exposure lift.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "levels", "type": "int", "default": 3, "min": 2, "max": 8 },
    { "id": "scale", "type": "float", "default": 2.0, "min": 1.0, "max": 8.0 },
    { "id": "monochrome", "type": "bool", "default": false },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let cell = vec2u(floor(px / scale)) % vec2u(4u);
    // Interleave the two coordinate bits to form the Bayer rank.
    let low = ((cell.x & 1u) ^ (cell.y & 1u)) * 2u + (cell.y & 1u);
    let high = ((cell.x >> 1u) ^ (cell.y >> 1u)) * 2u + (cell.y >> 1u);
    let threshold = (f32(low * 4u + high) + 0.5) / 16.0;
    let gray = vec3f(dot(original.rgb, vec3f(0.2126, 0.7152, 0.0722)));
    let color = clamp(select(original.rgb, gray, monochrome) + 0.08 * music * g.level, vec3f(0.0), vec3f(1.0));
    let steps = f32(max(levels - 1, 1));
    return vec4f(floor(color * steps + threshold) / steps, original.a);
}
