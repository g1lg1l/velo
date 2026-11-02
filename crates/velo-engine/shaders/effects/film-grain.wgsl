/*{
  "name": "Film Grain",
  "category": "effect",
  "description": "Animated fine grain adds texture with a little extra grit on the hats.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "grain", "type": "float", "default": 0.12, "min": 0.0, "max": 0.5 },
    { "id": "size", "type": "float", "default": 1.0, "min": 1.0, "max": 6.0 },
    { "id": "colored", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn grain_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(17.73, 93.41))) * 27491.53);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let cell = floor(px / size) + vec2f(37.0, 13.0) * floor(g.time * 24.0);
    let mono = grain_hash(cell) - 0.5;
    let noise_color = vec3f(mono, grain_hash(cell + 31.0) - 0.5, grain_hash(cell + 67.0) - 0.5);
    let noise_value = mix(vec3f(mono), noise_color, colored);
    let luma = clamp(dot(original.rgb, vec3f(0.2126, 0.7152, 0.0722)), 0.0, 1.0);
    let strength = grain * (1.0 + 0.5 * music * g.hat) * (0.4 + 0.6 * sqrt(luma));
    return vec4f(max(original.rgb + noise_value * strength, vec3f(0.0)), original.a);
}
