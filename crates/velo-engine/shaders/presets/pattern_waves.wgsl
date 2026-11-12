/*{
  "name": "Pattern Waves",
  "category": "generator",
  "description": "A sheet of dots or lines, each as big as the light on it, moving like draped silk; the bass deepens the folds, the kick brightens the light",
  "params": [
    { "id": "pattern", "type": "int", "default": 0, "names": ["Dots", "Lines"] },
    { "id": "size", "type": "float", "default": 9.0, "min": 3.0, "max": 40.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "scale", "type": "float", "default": 1.4, "min": 0.4, "max": 6.0 },
    { "id": "depth", "type": "float", "default": 1.4, "min": 0.0, "max": 2.5 },
    { "id": "tint", "type": "color", "default": [0.75, 0.7, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.01, 0.06, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Pattern Waves (its code isn't used: MIT + Commons Clause).

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
    // cells `size` pixels across at 540 lines; each lit as the cloth is at its middle
    let cell = max(floor(size * g.resolution.y / 540.0), 3.0);
    let c = ((floor(px / cell) + 0.5) * cell / g.resolution - 0.5) * vec2f(g.aspect, 1.0) * scale;
    let t = g.time * speed;
    let e = 0.01;
    let h = cloth(c, t);
    let lift = depth * 0.35 * (1.0 + 0.6 * audio * g.bass);
    let n = normalize(vec3f(-(cloth(c + vec2f(e, 0.0), t) - h) / e * lift, -(cloth(c + vec2f(0.0, e), t) - h) / e * lift, 1.0));
    let light = normalize(vec3f(-0.4, -0.5, 0.75));
    let diffuse = max(dot(n, light), 0.0);
    let gloss = pow(max(dot(n, normalize(light + vec3f(0.0, 0.0, 1.0))), 0.0), 40.0);
    let shade = clamp((0.015 + 0.95 * pow(diffuse, 4.0) + gloss) * (1.0 + 0.5 * audio * g.kick), 0.0, 1.0);
    // the brighter, the bigger the dot or the thicker the line
    let f = fract(px / cell) - 0.5;
    let aa = 1.0 / cell;
    var mark = 1.0 - smoothstep(sqrt(shade) * 0.56 - aa, sqrt(shade) * 0.56 + aa, length(f));
    if pattern == 1 {
        mark = 1.0 - smoothstep(shade * 0.5 - aa, shade * 0.5 + aa, abs(f.y));
    }
    // white where the light is strongest
    return vec4f(mix(background.rgb, mix(tint.rgb * (0.55 + 0.6 * shade), vec3f(1.0), smoothstep(0.75, 1.0, shade) * 0.6), mark), 1.0);
}
