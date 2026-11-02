/*{
  "name": "Bloom",
  "category": "effect",
  "description": "What's bright bleeds soft light around it, wider and brighter on the kick with music up.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "threshold", "type": "float", "default": 0.7, "min": 0.0, "max": 1.0 },
    { "id": "radius", "type": "float", "default": 0.03, "min": 0.005, "max": 0.15 },
    { "id": "strength", "type": "float", "default": 1.2, "min": 0.0, "max": 6.0 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// what of a color is over the threshold
fn bright(c: vec3f) -> vec3f {
    let l = max(c.r, max(c.g, c.b));
    return c * max(l - threshold, 0.0) / max(l, 0.0001);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    // 32 taps on a golden-angle spiral out to radius (in heights), the nearer weighing more
    let pump = 1.0 + music * g.kick;
    let r = radius * pump;
    var glow = vec3f(0.0);
    var total = 0.0;
    for (var i = 0; i < 32; i++) {
        let k = (f32(i) + 0.5) / 32.0;
        let a = f32(i) * 2.39996323;
        let o = vec2f(cos(a), sin(a)) * sqrt(k) * r * vec2f(1.0 / g.aspect, 1.0);
        let w = exp(-3.0 * k);
        glow += bright(textureSampleLevel(src, smp, uv + o, 0.0).rgb) * w;
        total += w;
    }
    glow = glow / total * strength * pump;
    return vec4f(original.rgb + glow * tint.rgb, original.a);
}
