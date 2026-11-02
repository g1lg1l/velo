/*{
  "name": "V Beams",
  "category": "generator",
  "description": "Velo's mark: two beams of light rising from one point into a V through billowing smoke, thin rays fanning round them and dust glinting in the light; the bass opens and brightens it, the kick flashes the root and sends a shockwave along the floor, the hats and snare flicker the rays",
  "params": [
    { "id": "spread", "type": "float", "default": 0.42, "min": 0.1, "max": 0.9 },
    { "id": "width", "type": "float", "default": 0.08, "min": 0.02, "max": 0.3 },
    { "id": "intensity", "type": "float", "default": 1.0, "min": 0.0, "max": 2.0 },
    { "id": "smoke", "type": "float", "default": 0.8, "min": 0.0, "max": 1.5 },
    { "id": "rays", "type": "float", "default": 0.7, "min": 0.0, "max": 1.5 },
    { "id": "grain", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [1.0, 0.88, 0.74, 1.0] },
    { "id": "root", "type": "vec2", "default": [0.0, 0.64], "min": -1.0, "max": 1.0 },
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

// smoke: noise over a few octaves
fn fbm(p: vec2f) -> f32 {
    var v = 0.0;
    var a = 0.5;
    var q = p;
    for (var i = 0; i < 4; i = i + 1) {
        v = v + a * noise(q);
        q = q * 2.03 + vec2f(1.7, 9.2);
        a = a * 0.5;
    }
    return v;
}

// a beam from `o` along `dir`, opening as it goes and fading far from its root: its core and a
// wide halo round it
fn beam(p: vec2f, o: vec2f, dir: vec2f, w: f32) -> vec2f {
    let d = p - o;
    let along = dot(d, dir);
    let x = abs(dot(d, vec2f(-dir.y, dir.x))) / (w * (0.12 + max(along, 0.0)));
    let fade = exp(-max(along, 0.0) * 1.1) * smoothstep(-0.01, 0.03, along);
    return vec2f(exp(-x * x * 2.5), exp(-x * x * 0.2)) * fade;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let o = root * 0.5 * vec2f(g.aspect, 1.0);
    let d = p - o;
    let r = length(d);
    // 0 straight up from the root
    let ang = atan2(d.x, -d.y);
    let bass = audio * g.bass;
    let kick = audio * g.kick;
    let t = g.time;
    // the smoke churns faster as the music gets louder
    let ts = t * 0.5 + audio * g.level_time * 0.5;
    // a breath on every beat while something plays
    let breath = 1.0 + 0.3 * audio * smoothstep(0.02, 0.25, g.level) * exp(-g.beat_phase * 4.0);
    let lift = intensity * breath * (1.0 + 0.9 * bass);

    // the V opens and its beams widen with the bass
    let a = spread * (1.0 + 0.25 * bass);
    let w = width * (1.0 + 0.6 * bass);
    let bl = beam(p, o, vec2f(-sin(a), -cos(a)), w);
    let br = beam(p, o, vec2f(sin(a), -cos(a)), w);
    let core = bl.x + br.x;
    let halo = bl.y + br.y;

    // smoke rising in two layers, folded by a slow warp so it billows
    let q = p * 2.4 + vec2f(0.0, ts * 0.22);
    let warp = vec2f(noise(q * 0.6 + vec2f(ts * 0.07, 3.1)), noise(q * 0.6 + vec2f(7.4, -ts * 0.05))) - 0.5;
    let near = fbm(q + warp * 1.8);
    let far = fbm(p * 1.2 + vec2f(-ts * 0.04, ts * 0.09) + warp * 0.9);
    // (thinner high up)
    let dens = smoke * (0.3 + 0.7 * smoothstep(-0.5, 0.35, p.y)) * smoothstep(0.45, 0.72, near * 0.65 + far * 0.55);
    // and a bank of it rolling along the floor, thickest round the root
    let up = -d.y;
    let bank = smoke * smoothstep(0.38, 0.66, fbm(vec2f(p.x * 1.6 + ts * 0.06, p.y * 3.5 + ts * 0.1) + warp * 1.4))
        * exp(-max(up, 0.0) * 2.5) * exp(-max(-up, 0.0) * 40.0) * exp(-abs(d.x) * 0.6);

    // shafts in the light, radiating from the root
    let shafts = smoothstep(0.2, 0.8, noise(vec2f(ang * 26.0, r * 1.2 - ts * 0.35)) * 0.6 + noise(vec2f(ang * 61.0 + 4.0, r * 2.0 - ts * 0.5)) * 0.4);
    // dusty grain in the light
    let grit = mix(1.0, 0.55 + 0.9 * hash(px * 0.7 + vec2f(floor(t * 24.0) * 13.0, 0.0)), grain);
    // the beams: solid where the smoke is thick, streaked by the shafts
    var light = (core * (0.4 + 1.3 * dens) * (0.5 + 0.9 * shafts) + halo * 0.1 * (0.2 + 1.5 * dens)) * grit * lift;

    // thin rays fanning round the V, drifting, sweeping with the highs, flickered by the hats and snare
    var fan = 0.0;
    let step16 = floor(g.beats * 4.0);
    for (var i = 0; i < 9; i = i + 1) {
        let k = f32(i);
        let h = hash(vec2f(k, 17.0));
        let at = (k - 4.0) * 0.24 + 0.08 * sin(t * (0.2 + 0.2 * h) + h * 6.28) + 0.06 * sin(audio * g.high_time * 0.9 + k * 2.1);
        let off = abs(sin(ang - at)) * r;
        let lit = exp(-off / (0.0015 + r * 0.006)) + 0.15 * exp(-off / (0.01 + r * 0.04));
        let on = hash(vec2f(k, step16));
        fan = fan + lit * (0.12 + 0.08 * sin(t * 1.7 + k * 2.3) + audio * (g.hat * step(0.4, on) + 0.6 * g.snare * on));
    }
    light = light + fan * rays * (0.15 + 1.2 * dens) * exp(-r * 1.6) * smoothstep(0.0, 0.06, r) * smoothstep(-0.01, 0.03, up) * grit * lift;

    // the root: a hot point, a bloom round it and a flare along the floor, flashing on the kick
    let flash = 1.0 + 1.6 * kick;
    let glow = exp(-r * 6.0);
    let hot = exp(-r * 30.0);
    let flare = exp(-abs(d.y) * 160.0) * exp(-abs(d.x) * 2.5 / (1.0 + kick)) + 0.2 * exp(-abs(d.y) * 30.0) * exp(-abs(d.x) * 2.0);
    light = light + (hot * 1.4 + glow * 0.15 + flare * 0.8) * flash * intensity * breath;
    // the smoke above the floor lit by it and by what spills from the beams, in shafts
    // (darker between the beams, so the V keeps its shape)
    let spill = (exp(-r * 2.0) * 0.45 * (0.3 + 0.7 * smoothstep(0.0, a, abs(ang))) + halo * 0.25 + glow * 0.3) * (1.0 - smoothstep(-0.01, 0.03, d.y));
    light = light + (dens * (0.5 + shafts * shafts) + bank * 2.0) * spill * (1.0 + 0.6 * kick) * intensity * (1.0 + 0.5 * bass);
    light = light + bank * (core * 0.5 + flare * 0.5 + 0.05) * flash * intensity;

    // dust glinting where the light is, more with the highs
    var glints = 0.0;
    for (var l = 0; l < 2; l = l + 1) {
        let fl = f32(l);
        let s = (p + vec2f(0.02 * sin(t * 0.3 + fl), ts * (0.03 + 0.02 * fl))) * (70.0 + 50.0 * fl);
        let cell = floor(s) + fl * 57.0;
        let h = hash(cell);
        let spot = vec2f(hash(cell + vec2f(113.0, 0.0)), hash(cell + vec2f(0.0, 211.0))) * 0.6 + 0.2;
        let twinkle = pow(0.5 + 0.5 * sin(t * (2.0 + 4.0 * h) + h * 40.0), 6.0) * (0.6 + 1.5 * audio * max(g.hat, g.high));
        glints = glints + smoothstep(0.18, 0.0, length(fract(s) - spot)) * step(0.55, h) * twinkle;
    }
    light = light + glints * (core * 1.5 + halo * 0.3 + glow * 0.6) * intensity;

    // a shockwave of light running out along the floor on the kick
    let e = length(vec2f(d.x, d.y * 6.0));
    let rad = (1.0 - g.kick) * 1.2;
    let x = (e - rad) / (0.02 + 0.08 * rad);
    let wisp = noise(vec2f(d.x * 7.0, d.y * 40.0) - ts);
    light = light + exp(-x * x) * sqrt(g.kick) * audio * (0.25 + 0.75 * wisp) * (0.5 + 1.8 * bank) * (0.6 + 0.4 * step(0.0, d.y)) * intensity;

    // the floor: a faint, rippled reflection of the V below the root
    if (d.y > 0.0) {
        let pm = vec2f(p.x + (noise(vec2f(p.x * 8.0, d.y * 90.0 - t * 2.0)) - 0.5) * 0.02, o.y - d.y);
        let rl = beam(pm, o, vec2f(-sin(a), -cos(a)), w * 1.4).x + beam(pm, o, vec2f(sin(a), -cos(a)), w * 1.4).x;
        light = light + rl * 0.3 * exp(-d.y * 10.0) * lift;
    }

    // warm light, white where it's hottest, the unlit smoke a cold deep blue
    var c = tint.rgb * light + vec3f(1.0) * (hot * hot * (0.6 + kick) + core * core * 0.12 * (1.0 + bass)) + vec3f(0.3, 0.38, 0.55) * (dens * 0.1 + bank * 0.1);
    c = c * (1.0 - 0.6 * dot(uv - 0.5, uv - 0.5));
    return vec4f(1.0 - exp(-c * 1.25), 1.0);
}
