/*{
  "name": "Lightning",
  "category": "generator",
  "description": "Electric bolts striking down out of the dark, lighting up again on the kick",
  "params": [
    { "id": "bolts", "type": "int", "default": 2, "min": 1, "max": 6 },
    { "id": "rate", "type": "float", "default": 1.0, "min": 0.1, "max": 6.0 },
    { "id": "jag", "type": "float", "default": 0.12, "min": 0.0, "max": 0.4 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.2, "max": 4.0 },
    { "id": "tint", "type": "color", "default": [0.55, 0.65, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.0, 0.04, 1.0] },
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

// -0.5..0.5, with fine kinks on top of the big bends
fn wander(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 5; i++) {
        sum += amp * (noise(p) - 0.5);
        p = p * 2.1 + vec2f(1.7, 9.2);
        amp *= 0.55;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // 0 at the top, 1 at the bottom
    let y = uv.y;
    let aa = 1.0 / g.resolution.y;
    let w = width * 0.0015;
    var light = 0.0;
    var core = 0.0;
    var sky_flash = 0.0;
    for (var i = 0; i < bolts; i++) {
        let f = f32(i);
        // each strike its own place and path; it flares, then crackles on, faint
        let phase = g.time * rate + f * 0.618 + hash(vec2f(f, 1.0));
        let n = floor(phase);
        let since = fract(phase);
        let seed = hash(vec2f(n, f + 5.0)) * 100.0;
        let x0 = (hash(vec2f(n + 3.0, f)) - 0.5) * g.aspect * 0.8;
        let end = 0.6 + 0.4 * hash(vec2f(f, n + 9.0));
        let crackle = 0.004 * audio * g.hat * sin(y * 300.0 + g.time * 90.0);
        let path = x0 + wander(vec2f(y * 5.0, seed)) * jag * 3.0 * y + crackle;
        let d = abs(s.x - path);
        let along = 1.0 - smoothstep(end - 0.1, end, y);
        let flicker = 0.6 + 0.4 * hash(vec2f(floor(g.time * 24.0), f));
        let strike = (exp(-since * 5.0) + 0.3) * flicker + 0.7 * audio * g.kick;
        core += (1.0 - smoothstep(w, w + aa, d)) * along * strike;
        light += w * 3.0 / (d + w * 3.0) * along * strike;
        sky_flash += strike;
    }
    let col = background.rgb + tint.rgb * (light * 0.6 + sky_flash * 0.04) + vec3f(core);
    return vec4f(col, 1.0);
}
