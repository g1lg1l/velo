/*{
  "name": "God Rays",
  "category": "effect",
  "description": "Light streaming out of the picture's bright parts, away from a point, as through mist; longer on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "center", "type": "vec2", "default": [0.0, -0.3], "min": -1.0, "max": 1.0 },
    { "id": "length", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "threshold", "type": "float", "default": 0.75, "min": 0.0, "max": 1.0 },
    { "id": "strength", "type": "float", "default": 1.2, "min": 0.0, "max": 4.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let c = 0.5 + center * 0.5;
    let reach = length * (1.0 + 0.6 * music * g.kick);
    // 32 steps back toward the point, what's bright along the way adding up, the far fainter
    var rays = vec3f(0.0);
    var weight = 0.0;
    for (var i = 0; i < 32; i++) {
        let k = f32(i) / 32.0;
        let s = textureSampleLevel(src, smp, uv + (c - uv) * k * reach, 0.0).rgb;
        let l = max(s.r, max(s.g, s.b));
        let w = 1.0 - k;
        rays += s * max(l - threshold, 0.0) / max(l, 0.0001) * w;
        weight += w;
    }
    return vec4f(original.rgb + rays / weight * strength * 2.0, original.a);
}
