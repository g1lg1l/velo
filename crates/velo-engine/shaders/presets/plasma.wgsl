/*{
  "name": "Plasma",
  "category": "generator",
  "description": "The classic sine plasma: three colors flowing into each other",
  "params": [
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "scale", "type": "float", "default": 4.0, "min": 0.5, "max": 16.0 },
    { "id": "color_a", "type": "color", "default": [0.08, 0.02, 0.35, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.95, 0.15, 0.55, 1.0] },
    { "id": "color_c", "type": "color", "default": [1.0, 0.8, 0.25, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the bass breathes the scale
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale * (1.0 - 0.1 * audio * g.bass);
    let t = g.time * speed;
    var v = sin(p.x + t);
    v += sin((p.y + t) * 0.7);
    v += sin((p.x + p.y) * 0.6 + t * 1.3);
    v += sin(length(p + vec2f(sin(t * 0.5), cos(t * 0.4)) * scale * 0.4) * 1.2 - t);
    // the hats nudge the colors along
    v += 0.3 * audio * g.hat;
    // three weights a third of a turn apart: the colors take turns
    let w = pow(0.5 + 0.5 * cos(v * 1.5707963 + vec3f(0.0, 2.0943951, 4.1887902)), vec3f(2.0));
    let col = (color_a.rgb * w.x + color_b.rgb * w.y + color_c.rgb * w.z) / (w.x + w.y + w.z + 0.0001);
    return vec4f(col * (1.0 + 0.3 * audio * g.kick), 1.0);
}
