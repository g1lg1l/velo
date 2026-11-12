/*{
  "name": "Prismatic Burst",
  "category": "generator",
  "description": "Rays of light bursting from a point in all the colors of the spectrum, slowly turning; the bass brightens them, the kick sends a wave of light out",
  "params": [
    { "id": "rays", "type": "float", "default": 24.0, "min": 4.0, "max": 80.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "spin", "type": "float", "default": 0.02, "min": -0.5, "max": 0.5 },
    { "id": "reach", "type": "float", "default": 0.9, "min": 0.2, "max": 3.0 },
    { "id": "spread", "type": "int", "default": 1, "min": 1, "max": 4 },
    { "id": "saturation", "type": "float", "default": 0.85, "min": 0.0, "max": 1.0 },
    { "id": "intensity", "type": "float", "default": 1.0, "min": 0.0, "max": 2.0 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Prismatic Burst (its code isn't used: MIT + Commons Clause).

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

fn spectrum(h: f32) -> vec3f {
    return 0.5 + 0.5 * cos(6.2831853 * (h + vec3f(0.0, 0.33, 0.67)));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5 - center * 0.5) * vec2f(g.aspect, 1.0);
    let r = length(s);
    let a = g.time * spin * 6.2831853;
    let d0 = s / max(r, 0.0001);
    let d = vec2f(d0.x * cos(a) - d0.y * sin(a), d0.x * sin(a) + d0.y * cos(a));
    let t = g.time * speed;
    // noise read round a circle: rays with no seam, broad ones and fine ones
    let broad = noise(d * rays * 0.35 + vec2f(t, -t * 0.6));
    let fine = noise(d * rays + vec2f(7.0 - t * 1.4, t * 0.9));
    let ray = broad * broad * 0.7 + pow(fine, 4.0) * 1.6;
    let fall = exp(-r * 2.2 / (reach * (1.0 + 0.5 * audio * g.kick)));
    // the colors go round the burst (spread times) and shift along each ray
    let hue = atan2(d.y, d.x) / 6.2831853 * f32(spread) + r * 0.6 - t * 0.15 + broad * 0.25;
    let c = mix(vec3f(1.0), spectrum(hue), saturation);
    var col = c * ray * fall * 2.2 * (1.0 + 0.6 * audio * g.bass);
    col += vec3f(1.0) * exp(-r * 14.0) * 1.2 + c * exp(-r * 5.0) * 0.4;
    // the kick's wave of light moves out as the kick fades
    col += c * audio * g.kick * exp(-abs(r - (1.0 - g.kick) * 0.9) * 12.0) * 0.8;
    return vec4f(col * intensity, 1.0);
}
