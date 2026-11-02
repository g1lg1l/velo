/*{
  "name": "Strobe",
  "category": "effect",
  "description": "A burst of light every so many beats that dies away, and on every kick with music up.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "every", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
    { "id": "decay", "type": "float", "default": 0.25, "min": 0.02, "max": 1.0 },
    { "id": "strength", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// every and decay are in beats: the flash starts on each `every`th beat and is all but gone
// `decay` beats later

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let since = fract(g.beats / max(every, 0.01)) * every;
    let flash = max(exp(-4.0 * since / max(decay, 0.001)), music * g.kick) * strength;
    return vec4f(mix(original.rgb, tint.rgb, flash), max(original.a, flash));
}
