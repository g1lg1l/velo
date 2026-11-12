/*{
  "name": "Scanner",
  "category": "generator",
  "description": "Glowing interference lines between two sets of rings, swept by a beam of light that leaves them glowing behind it, on an old tube; the kick pings a ring out, the bass pulls the rings apart",
  "params": [
    { "id": "density", "type": "float", "default": 14.0, "min": 3.0, "max": 50.0 },
    { "id": "apart", "type": "float", "default": 0.3, "min": 0.0, "max": 0.8 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "afterglow", "type": "float", "default": 0.5, "min": 0.05, "max": 1.0 },
    { "id": "scanlines", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.2, 1.0, 0.8, 1.0] },
    { "id": "beam", "type": "color", "default": [0.9, 1.0, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.02, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Scanner (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    // two sets of rings whose middles go round each other; where one's crest meets the
    // other's trough is a line
    let c = apart * (1.0 + 0.4 * audio * g.bass) * vec2f(cos(t * 0.7), sin(t * 0.5));
    let v = sin(length(p - c) * density * 6.2831853) * sin(length(p + c) * density * 6.2831853 + t * 2.0);
    let per_pixel = max(fwidth(v), 0.0001);
    let d = abs(v) / per_pixel;
    let line = (1.0 - smoothstep(0.5, 1.5, d) + 0.25 * exp(-d * 0.25)) * (1.0 - smoothstep(0.5, 1.2, per_pixel));
    // the beam goes across and starts over; what it passed keeps glowing for a while
    let wide = g.aspect + 0.4;
    let at = fract(t * 0.5) * wide;
    let behind = fract((at - (p.x + 0.5 * wide)) / wide);
    let trail = exp(-behind * 5.0 / afterglow);
    let head = exp(-abs(behind) * 220.0) + exp(-abs(behind - 1.0) * 220.0);
    // the kick's ping
    let r = length(p);
    let ping = audio * g.kick * exp(-abs(r - (1.0 - g.kick) * 0.9) * 14.0);
    var col = background.rgb + tint.rgb * line * (0.12 + 1.3 * trail + 1.5 * ping);
    col += beam.rgb * head * (0.5 + line);
    // the tube: scanlines and dark corners
    col *= 1.0 - scanlines * (0.5 + 0.5 * sin(px.y * 3.1415927)) * 0.6;
    col *= 1.0 - 0.5 * dot(uv - 0.5, uv - 0.5) * 2.0;
    return vec4f(col, 1.0);
}
