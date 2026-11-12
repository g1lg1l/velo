/*{
  "name": "Orb",
  "category": "generator",
  "description": "A glowing ring of three colors swirling round, breathing, a spark of light orbiting it with a tail; the bass swells it, the kick flares it",
  "params": [
    { "id": "size", "type": "float", "default": 0.3, "min": 0.05, "max": 0.8 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "glow", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "color_a", "type": "color", "default": [0.6, 0.3, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.2, 0.8, 1.0, 1.0] },
    { "id": "color_c", "type": "color", "default": [1.0, 0.4, 0.7, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Orb (its code isn't used: MIT + Commons Clause).

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
    let t = g.time * speed;
    // in the ring's radii from its middle; it breathes, and swells with the bass
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0) / (size * (1.0 + 0.06 * sin(t * 1.3) + 0.15 * audio * g.bass));
    let r = length(s);
    let out = s / max(r, 0.0001);
    let around = atan2(s.y, s.x);
    // how thick and how bright the ring is flows round it: noise read round a circle, no seam
    let slow = noise(out * 2.0 + vec2f(t, -t * 0.7));
    let quick = noise(out * 5.0 + vec2f(9.0 - t * 1.3, t * 0.5));
    // light dies away slowly toward the middle, fast outward
    let d = r - 1.0;
    let band = select(exp(-d * (14.0 - 6.0 * quick)), exp(d * (3.0 + 4.0 * slow)), d < 0.0);
    let line = exp(-abs(d) * 45.0);
    // three colors a third of a turn apart, going round
    let w = pow(max(0.5 + 0.5 * cos(around - t * 0.8 + slow * 3.0 + vec3f(0.0, 2.0943951, 4.1887902)), vec3f(0.0)), vec3f(2.0));
    let tint = (color_a.rgb * w.x + color_b.rgb * w.y + color_c.rgb * w.z) / (w.x + w.y + w.z + 0.0001);
    // a spark going round the ring, its tail fading behind it
    let turn = t * 1.7;
    let spark = exp(-length(s - vec2f(cos(turn), sin(turn))) * 9.0);
    let tail = exp(-fract((turn - around) / 6.2831853) * 9.0) * exp(-abs(d) * 22.0);
    let flare = 1.0 + audio * g.kick;
    var col = background.rgb + tint * (band * (0.45 + 0.8 * quick) + line * 0.8) * glow * flare;
    col += mix(tint, vec3f(1.0), 0.7) * (spark * 0.9 + tail * 0.7) * flare;
    return vec4f(col, 1.0);
}
