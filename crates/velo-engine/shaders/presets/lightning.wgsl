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

// how lit a pixel `d` from a bolt of half width w is: x its white core, y the glow round it
fn bolt(d: f32, w: f32, aa: f32) -> vec2f {
    return vec2f(1.0 - smoothstep(w, w + aa, d), w * 5.0 / (d + w * 5.0) + 0.5 * exp(-d * 18.0));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // 0 at the top, 1 at the bottom
    let y = uv.y;
    let aa = 1.0 / g.resolution.y;
    let w = width * 0.0018;
    var light = 0.0;
    var core = 0.0;
    var sky_flash = 0.0;
    var sky_at = 0.0;
    for (var i = 0; i < bolts; i++) {
        let f = f32(i);
        // each strike its own place and path: it flares, strikes again a moment later, then
        // crackles on faintly until the next
        let phase = g.time * rate + f * 0.618 + hash(vec2f(f, 1.0));
        let n = floor(phase);
        let since = fract(phase);
        let seed = hash(vec2f(n, f + 5.0)) * 100.0;
        let x0 = (hash(vec2f(n + 3.0, f)) - 0.5) * g.aspect * 0.8;
        let end = 0.75 + 0.3 * hash(vec2f(f, n + 9.0));
        let crackle = 0.004 * audio * g.hat * sin(y * 300.0 + g.time * 90.0);
        let path = x0 + wander(vec2f(y * 5.0, seed)) * jag * 3.0 * (0.25 + y) + crackle;
        let flicker = 0.6 + 0.4 * hash(vec2f(floor(g.time * 24.0), f));
        let strike = (exp(-since * 4.5) + 0.6 * exp(-abs(since - 0.2) * 25.0) + 0.22) * flicker + audio * g.kick;
        // thinner toward its tip
        let b = bolt(abs(s.x - path), w * (1.4 - 0.7 * y), aa) * (1.0 - smoothstep(end - 0.12, end, y));
        core += b.x * strike;
        light += b.y * strike;
        // two forks leaving it part way down, each to its own side, fainter and shorter
        for (var k = 0; k < 2; k++) {
            let fk = f32(k);
            let start = 0.15 + 0.5 * hash(vec2f(n + fk * 7.0, f + 21.0));
            let side = select(-1.0, 1.0, hash(vec2f(n + fk * 3.0, f + 33.0)) < 0.5);
            let reach = 0.18 + 0.2 * hash(vec2f(n + fk, f + 47.0));
            let down = y - start;
            let root = x0 + wander(vec2f(start * 5.0, seed)) * jag * 3.0 * (0.25 + start);
            let fork = root + side * down * (0.5 + 0.6 * fk) + wander(vec2f(y * 9.0, seed + 40.0 + fk * 13.0)) * jag * 1.5 * smoothstep(0.0, 0.1, down) + crackle;
            let fb = bolt(abs(s.x - fork), w * 0.6, aa) * step(0.0, down) * (1.0 - smoothstep(reach * 0.6, reach, down));
            core += fb.x * strike * 0.6;
            light += fb.y * strike * 0.3;
        }
        sky_flash += strike;
        sky_at += strike * exp(-abs(s.x - x0) * 1.5);
    }
    // the clouds the strikes light from inside, brightest above each
    let clouds = noise(vec2f(s.x * 2.0 + g.time * 0.03, y * 5.0)) * 0.6 + noise(vec2f(s.x * 5.0 - g.time * 0.05, y * 11.0 + 3.0)) * 0.4;
    let overcast = clouds * exp(-y * 3.5) * (0.12 * sky_flash + 0.6 * sky_at);
    let col = background.rgb * (1.0 + 1.5 * sky_flash) + tint.rgb * (light * 0.55 + overcast + sky_flash * 0.03) + vec3f(core) * 1.2;
    return vec4f(col, 1.0);
}
