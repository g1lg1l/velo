/*{
  "name": "Color Bends",
  "category": "generator",
  "description": "Three vivid colors bending round each other in smooth folds with dark creases between; the bass bends them further, the kick lights them",
  "params": [
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "scale", "type": "float", "default": 1.0, "min": 0.3, "max": 4.0 },
    { "id": "bends", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "crease", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [1.0, 0.25, 0.45, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.2, 0.4, 1.0, 1.0] },
    { "id": "color_c", "type": "color", "default": [0.1, 1.0, 0.75, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Color Bends (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    var q = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale * 2.0;
    let t = g.time * speed;
    let amount = bends * (1.0 + 0.4 * audio * g.bass);
    // the plane bent by waves of itself, each finer and weaker than the last
    for (var i = 1; i < 4; i++) {
        let f = f32(i);
        q += amount * 0.4 / f * vec2f(sin(q.y * f * 1.3 + t + f * 0.7), cos(q.x * f * 1.1 - t * 0.9 + f * 1.9));
    }
    // three weights a third of a turn apart: the colors take turns across the bends
    // (never a hair below zero: a power of that is no number at all, a white speck)
    let w = pow(max(0.5 + 0.5 * cos((q.x * 0.9 + q.y * 0.6) * 1.6 + vec3f(0.0, 2.0943951, 4.1887902)), vec3f(0.0)), vec3f(1.5));
    var col = (color_a.rgb * w.x + color_b.rgb * w.y + color_c.rgb * w.z) / (w.x + w.y + w.z + 0.0001);
    // a dark seam where a fold turns under, a sheen on its shoulder
    let fold = abs(sin(q.x * 1.3 - q.y * 0.8));
    col *= mix(1.0, 0.25 + 0.85 * sqrt(fold), crease);
    col += vec3f(1.0) * pow(fold, 12.0) * 0.18 * crease;
    return vec4f(col * (1.0 + 0.35 * audio * g.kick), 1.0);
}
