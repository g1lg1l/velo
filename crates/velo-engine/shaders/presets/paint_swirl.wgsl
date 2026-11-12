/*{
  "name": "Paint Swirl",
  "category": "generator",
  "description": "Thick paint in three colors folding into a slow whirlpool, in crisp pixels or smooth; the kick winds it tighter, the bass pulls it in",
  "params": [
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.3, "max": 4.0 },
    { "id": "swirl", "type": "float", "default": 1.0, "min": -3.0, "max": 3.0 },
    { "id": "contrast", "type": "float", "default": 3.0, "min": 0.5, "max": 6.0 },
    { "id": "pixel", "type": "float", "default": 3.0, "min": 1.0, "max": 24.0 },
    { "id": "color_a", "type": "color", "default": [0.87, 0.21, 0.2, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.0, 0.42, 0.71, 1.0] },
    { "id": "color_c", "type": "color", "default": [0.07, 0.1, 0.12, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Balatro (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // crisp pixels: every pixel of a block shows the block's middle (1: smooth)
    let block = max(pixel, 1.0);
    let at = (floor(px / block) + 0.5) * block / g.resolution;
    var q = (at - 0.5) * vec2f(g.aspect, 1.0) * 2.4 / zoom;
    let t = g.time * speed;
    // the whole pool turns, the middle faster: a whirlpool; the kick winds it tighter
    let w = swirl * (1.2 / (length(q) + 0.35) + 0.3) * (1.0 + 0.5 * audio * g.kick) + t * 0.15;
    q = vec2f(q.x * cos(w) - q.y * sin(w), q.x * sin(w) + q.y * cos(w)) * 2.2 * (1.0 - 0.15 * audio * g.bass);
    // the paint folded into itself five times
    for (var i = 0; i < 5; i++) {
        let f = f32(i);
        q += vec2f(sin(q.y * (1.0 + 0.35 * f) + t + f * 1.7), cos(q.x * (1.0 + 0.3 * f) - t * 0.8 + f * 2.3)) * 0.75 / (1.0 + 0.5 * f);
    }
    let v = 0.5 + 0.5 * sin(q.x * 0.9 + q.y * 0.7);
    let u = 0.5 + 0.5 * sin(q.x * 0.5 - q.y * 1.1 + 2.0);
    // three paints that keep their edges: the ground, one poured over it, one over both
    let e = 0.25 / contrast;
    var col = mix(color_c.rgb, color_b.rgb, smoothstep(0.45 - e, 0.45 + e, u));
    col = mix(col, color_a.rgb, smoothstep(0.5 - e, 0.5 + e, v));
    // wet: lighter where it lies thick
    col *= 0.8 + 0.35 * v * u + 0.25 * audio * g.kick;
    return vec4f(col, 1.0);
}
