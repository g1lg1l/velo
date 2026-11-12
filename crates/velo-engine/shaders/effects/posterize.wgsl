/*{
  "name": "Posterize",
  "category": "effect",
  "description": "Flat color bands with adjustable contrast lift slightly on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "levels", "type": "int", "default": 5, "min": 2, "max": 16 },
    { "id": "contrast", "type": "float", "default": 1.1, "min": 0.25, "max": 3.0 },
    { "id": "saturation", "type": "float", "default": 1.1, "min": 0.0, "max": 2.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let gray = vec3f(dot(original.rgb, vec3f(0.2126, 0.7152, 0.0722)));
    let saturated = mix(gray, original.rgb, saturation);
    // the kick brightens what is lit; black stays black
    let color = clamp(((saturated - 0.5) * contrast + 0.5) * (1.0 + 0.5 * music * g.kick), vec3f(0.0), vec3f(1.0));
    let steps = f32(max(levels - 1, 1));
    return vec4f(floor(color * steps + 0.5) / steps, original.a);
}
