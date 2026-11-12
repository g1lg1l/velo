/*{
  "name": "Silk",
  "category": "generator",
  "description": "Satin folds flowing slowly, their glossy highlights sliding as the cloth moves; the bass deepens the folds, the kick flashes the sheen",
  "params": [
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "scale", "type": "float", "default": 1.6, "min": 0.4, "max": 6.0 },
    { "id": "angle", "type": "float", "default": 0.08, "min": 0.0, "max": 1.0 },
    { "id": "depth", "type": "float", "default": 1.0, "min": 0.0, "max": 2.5 },
    { "id": "sheen", "type": "float", "default": 0.7, "min": 0.0, "max": 1.5 },
    { "id": "tint", "type": "color", "default": [0.48, 0.42, 0.62, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Silk (its code isn't used: MIT + Commons Clause).

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

// the cloth's height: long folds running one way, bent by slow noise, finer creases across
fn cloth(p: vec2f, t: f32) -> f32 {
    let w = noise(p * 0.7 + vec2f(t * 0.15, -t * 0.1));
    let a = p.x * 2.2 + w * 4.0 + 1.3 * sin(p.y * 0.9 + t * 0.6);
    return 0.6 * sin(a) + 0.25 * sin(a * 2.3 + p.y * 1.7 - t * 0.8) + 0.15 * sin(p.y * 4.0 + w * 6.0 + t);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let a = angle * 6.2831853;
    let p = vec2f(s.x * cos(a) - s.y * sin(a), s.x * sin(a) + s.y * cos(a)) * scale;
    let t = g.time * speed;
    let e = 0.01;
    let h = cloth(p, t);
    let lift = depth * 0.35 * (1.0 + 0.6 * audio * g.bass);
    let n = normalize(vec3f(-(cloth(p + vec2f(e, 0.0), t) - h) / e * lift, -(cloth(p + vec2f(0.0, e), t) - h) / e * lift, 1.0));
    let light = normalize(vec3f(-0.4, -0.5, 0.75));
    let diffuse = max(dot(n, light), 0.0);
    // satin: a broad soft sheen with a tight highlight on top
    let facing = max(dot(n, normalize(light + vec3f(0.0, 0.0, 1.0))), 0.0);
    let gloss = pow(facing, 12.0) * 0.6 + pow(facing, 90.0);
    var col = tint.rgb * (0.1 + 0.95 * diffuse * diffuse) * (0.85 + 0.15 * h);
    col += mix(tint.rgb, vec3f(1.0), 0.6) * gloss * sheen * (1.0 + 1.2 * audio * g.kick);
    // the weave, as a fine grain
    col += (hash(px) - 0.5) * 0.025;
    return vec4f(col, 1.0);
}
