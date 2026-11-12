/*{
  "name": "Magnet Lines",
  "category": "generator",
  "description": "A field of short lines that all swing to point at a magnet as it wanders (or where you put it: map a signal onto magnet); the kick sends a ripple of spin out from it",
  "params": [
    { "id": "density", "type": "float", "default": 14.0, "min": 4.0, "max": 50.0 },
    { "id": "reach", "type": "float", "default": 0.8, "min": 0.2, "max": 1.0 },
    { "id": "width", "type": "float", "default": 2.0, "min": 0.5, "max": 6.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "wander", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "magnet", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.45, 0.5, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.4, 0.7, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Magnet Lines (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let q = s * density;
    // the middle of this pixel's cell, in picture heights
    let c = (floor(q) + 0.5) / density;
    let t = g.time * speed;
    let m = magnet * 0.5 * vec2f(g.aspect, 1.0) + wander * vec2f(0.35 * g.aspect * sin(t * 0.9), 0.3 * sin(t * 1.3 + 1.0));
    let to = m - c;
    let far = length(to);
    // each line points at the magnet; the kick's ripple spins the ones it passes, the bass
    // makes them all tremble
    let a = atan2(to.y, to.x) + 3.0 * audio * g.kick * exp(-abs(far - (1.0 - g.kick) * 0.9) * 8.0) + 0.3 * audio * g.bass * sin(far * 12.0 - g.time * 4.0);
    let f = fract(q) - 0.5;
    let l = vec2f(f.x * cos(a) + f.y * sin(a), f.y * cos(a) - f.x * sin(a));
    let near = exp(-far * 1.8);
    // longer near the magnet; a line with round ends
    let half = reach * 0.5 * (0.55 + 0.45 * near);
    let aa = density / g.resolution.y;
    let w = width * 0.5 * aa;
    let d = length(vec2f(max(abs(l.x) - half + w, 0.0), l.y));
    let line = 1.0 - smoothstep(w, w + aa, d);
    let col = background.rgb + mix(color_a.rgb, color_b.rgb, near) * line * (0.6 + 0.9 * near) * (1.0 + 0.5 * audio * g.kick);
    return vec4f(col, 1.0);
}
