/*{
  "name": "Light Rays",
  "category": "generator",
  "description": "Shafts of light pouring down from above, slowly shifting",
  "params": [
    { "id": "speed", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "rays", "type": "float", "default": 6.0, "min": 1.0, "max": 20.0 },
    { "id": "reach", "type": "float", "default": 0.8, "min": 0.1, "max": 2.0 },
    { "id": "intensity", "type": "float", "default": 1.0, "min": 0.0, "max": 2.0 },
    { "id": "tint", "type": "color", "default": [1.0, 0.85, 0.6, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.02, 0.06, 1.0] },
    { "id": "center", "type": "vec2", "default": [0.0, -1.0], "min": -1.0, "max": 1.0 },
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
    let s = (uv - 0.5 - center * 0.5) * vec2f(g.aspect, 1.0);
    let r = length(s);
    let dir = s / max(r, 0.0001);
    let t = g.time * speed;
    // noise read around a circle: the rays go all the way round without a seam
    let n = noise(dir * rays + vec2f(t, -t * 0.7)) * 0.6 + noise(dir * rays * 2.3 + vec2f(17.0 - t * 1.3, t * 0.5)) * 0.28 + noise(dir * rays * 6.1 + vec2f(t * 1.7, 31.0 + t)) * 0.12;
    // a few strong shafts with dark between them, not an even wash
    let beam = pow(smoothstep(0.25, 0.85, n), 2.0) * 3.2;
    let fall = exp(-r / (reach * (1.0 + 0.4 * audio * g.kick)));
    let shimmer = 1.0 + audio * g.hat * 0.6 * (noise(dir * rays * 9.0 + g.time * 8.0) - 0.5);
    // dust drifting through the shafts, glinting where they're bright
    let m = (s + vec2f(0.03 * sin(g.time * 0.2), g.time * 0.012)) * 55.0;
    let cell = floor(m);
    let h = hash(cell);
    let mote = smoothstep(0.2, 0.0, length(fract(m) - 0.2 - 0.6 * vec2f(hash(cell + 13.0), hash(cell + 29.0)))) * step(0.72, h) * pow(0.5 + 0.5 * sin(g.time * (1.0 + 3.0 * h) + h * 50.0), 4.0);
    let raw = (beam * fall * shimmer * (1.0 + 1.6 * mote) + exp(-r * 7.0) * 0.9 + exp(-r * 30.0) * 1.5) * intensity * (1.0 + 0.6 * audio * g.bass);
    // rolled off, so loud music fills the shafts without burning them flat
    let light = 1.7 * (1.0 - exp(-raw * 0.75));
    // white where it's strongest, the tint where it thins
    let col = background.rgb * (1.0 + 0.6 * fall) + tint.rgb * light + vec3f(1.0) * max(light - 1.0, 0.0) * 0.4;
    return vec4f(col, 1.0);
}
