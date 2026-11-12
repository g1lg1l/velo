/*{
  "name": "Line Waves",
  "category": "generator",
  "description": "Fine lines flowing side by side in silky waves, a band of light drifting slowly through them; the bass deepens the waves, the kick sends the light across",
  "params": [
    { "id": "lines", "type": "float", "default": 60.0, "min": 10.0, "max": 200.0 },
    { "id": "amplitude", "type": "float", "default": 0.12, "min": 0.0, "max": 0.4 },
    { "id": "scale", "type": "float", "default": 1.5, "min": 0.3, "max": 6.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "angle", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.5, "max": 4.0 },
    { "id": "color_a", "type": "color", "default": [0.3, 0.35, 0.9, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.8, 0.95, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Line Waves (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x), mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let a = angle * 6.2831853;
    let p = vec2f(s.x * cos(a) - s.y * sin(a), s.x * sin(a) + s.y * cos(a));
    let t = g.time * speed;
    // the sheet of lines pushed up and down by slow noise, bent by a sine so it folds
    let q = p * scale;
    let push = noise(q + vec2f(t * 0.6, 0.0)) - 0.5 + 0.35 * sin(q.x * 2.3 + q.y * 1.1 + t * 1.3) + 0.3 * (noise(q * 2.3 - vec2f(0.0, t * 0.8)) - 0.5);
    let v = (p.y + amplitude * (1.0 + 0.7 * audio * g.bass) * push) * lines;
    // a pixel or two wide wherever the lines crowd or spread
    let per_pixel = max(fwidth(v), 0.0001);
    let d = abs(fract(v) - 0.5) / per_pixel;
    let line = (1.0 - smoothstep(0.5 * width, 0.5 * width + 1.0, d)) * (1.0 - smoothstep(0.35, 0.9, per_pixel));
    // crowded lines read as lit cloth
    let crowd = smoothstep(0.35, 0.9, per_pixel) * 0.25;
    // a band of light crossing at a slant; the kick sends another straight across
    let band = exp(-pow((p.x + p.y * 0.4 - 0.9 * g.aspect * sin(t * 0.5)) * 2.5, 2.0));
    let sweep = audio * g.kick * exp(-pow((p.x / g.aspect + 0.5 - (1.0 - g.kick)) * 6.0, 2.0));
    let lit = 0.3 + 0.5 * (push + 0.5) + 0.8 * band + 1.2 * sweep;
    let tint = mix(color_a.rgb, color_b.rgb, clamp(band + sweep, 0.0, 1.0));
    return vec4f(background.rgb + tint * (line + crowd) * lit, 1.0);
}
