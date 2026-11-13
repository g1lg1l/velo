/*{
  "name": "Solarize",
  "category": "effect",
  "description": "What's brighter than the threshold turned negative, as a print left in the light: the lights go dark and the midtones are the brightest; on each channel or on the brightness alone; the kick lowers the threshold.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "threshold", "type": "float", "default": 0.5, "min": 0.05, "max": 0.95 },
    { "id": "colored", "type": "bool", "default": false },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// up to the threshold as it is, over it going back down: a peak at the threshold, lifted to 1
fn solarize_curve(x: vec3f, t: f32) -> vec3f {
    return select(x / t, (1.0 - x) / (1.0 - t), x > vec3f(t));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    let c = clamp(s.rgb / max(s.a, 0.0001), vec3f(0.0), vec3f(1.0));
    let t = clamp(threshold * (1.0 - 0.4 * music * g.kick), 0.05, 0.95);
    let l = dot(c, vec3f(0.2126, 0.7152, 0.0722));
    // on the brightness alone the hue stays: the color scaled to the new brightness
    let lone = c * solarize_curve(vec3f(l), t).x / max(l, 0.0001);
    let out = select(lone, solarize_curve(c, t), colored);
    return vec4f(clamp(out, vec3f(0.0), vec3f(1.0)) * s.a, s.a);
}
