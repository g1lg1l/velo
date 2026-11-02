/*{
  "name": "Ridgelines",
  "category": "generator",
  "description": "Stacked lines rising into peaks in the middle, each hiding the ones behind it, like a pulsar's chart on a record sleeve; the bass raises the peaks",
  "params": [
    { "id": "lines", "type": "int", "default": 40, "min": 8, "max": 80 },
    { "id": "height", "type": "float", "default": 0.12, "min": 0.0, "max": 0.4 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "spread", "type": "float", "default": 0.35, "min": 0.1, "max": 1.0 },
    { "id": "width", "type": "float", "default": 1.2, "min": 0.5, "max": 4.0 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

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
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let n = f32(lines);
    let gap = 0.7 / n;
    let aa = 1.0 / g.resolution.y;
    let lift = height * (1.0 + 0.8 * audio * g.bass);
    // only the middle rises
    let middle = exp(-pow(p.x / spread, 2.0) * 3.0);
    var col = background.rgb;
    // back to front: each line's ground covers what's behind and under it
    let peak = middle * lift * 2.55 + width * aa * 2.0;
    for (var i = 0; i < lines; i++) {
        let k = f32(i);
        let base = -0.35 + k * gap;
        // a line rises from its base, never falls below it: one whose peaks can't reach this
        // pixel, nor any after it, ends the loop; one wholly above it only covers it
        if base - peak > p.y {
            break;
        }
        if base < p.y - width * aa * 2.0 {
            col = background.rgb;
            continue;
        }
        let wave = noise(vec2f(p.x * 9.0 + k * 3.1, k * 1.7 + g.time * speed)) * 0.7 + noise(vec2f(p.x * 23.0 - k * 2.3, g.time * speed * 1.7)) * 0.3;
        let y = base - middle * lift * pow(wave, 2.0) * 2.5 - lift * 0.05 * wave;
        let d = p.y - y;
        if d > 0.0 {
            col = background.rgb;
        }
        let line = 1.0 - smoothstep(width * aa * 0.5, width * aa * 0.5 + aa, abs(d));
        col = mix(col, tint.rgb, line);
    }
    return vec4f(col, 1.0);
}
