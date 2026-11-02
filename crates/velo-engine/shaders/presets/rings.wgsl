/*{
  "name": "Rings",
  "category": "generator",
  "description": "Wobbling rings flowing out from the center",
  "params": [
    { "id": "count", "type": "float", "default": 10.0, "min": 2.0, "max": 40.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": -2.0, "max": 2.0 },
    { "id": "width", "type": "float", "default": 0.2, "min": 0.02, "max": 0.9 },
    { "id": "wobble", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.3, 0.9, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.7, 0.2, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.0, 0.05, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = length(s);
    let a = atan2(s.y, s.x);
    let t = g.time * speed;
    // the bass shakes the rings out of round
    let bend = wobble * 0.015 * (1.0 + 1.5 * audio * g.bass) * (sin(a * 3.0 + t * 2.0) + 0.5 * sin(a * 5.0 - t * 3.0)) * r * 4.0;
    let v = (r + bend) * count - t * 2.0;
    let ring = floor(v);
    // 0 in the middle of a ring, 0.5 between two
    let d = abs(fract(v) - 0.5);
    let e = count / g.resolution.y * 1.5;
    let line = 1.0 - smoothstep(width * 0.5, width * 0.5 + e, d);
    let glow = exp(-d * 10.0 / (0.3 + width)) * 0.35;
    let flicker = 1.0 + 0.35 * audio * g.hat * sin(ring * 3.7 + g.time * 30.0);
    let tint = mix(color_a.rgb, color_b.rgb, smoothstep(0.0, 0.8, r));
    let col = background.rgb + tint * (line + glow) * flicker * (1.2 - r) * (1.0 + 0.4 * audio * g.kick);
    return vec4f(col, 1.0);
}
