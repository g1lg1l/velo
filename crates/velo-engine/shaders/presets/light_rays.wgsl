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
    let n = noise(dir * rays + vec2f(t, -t * 0.7)) * 0.65 + noise(dir * rays * 2.3 + vec2f(17.0 - t * 1.3, t * 0.5)) * 0.35;
    let beam = pow(n, 3.0) * 2.5;
    let fall = exp(-r / (reach * (1.0 + 0.3 * audio * g.kick)));
    let shimmer = 1.0 + audio * g.hat * 0.5 * (noise(dir * rays * 9.0 + g.time * 8.0) - 0.5);
    let light = (beam * fall * shimmer + exp(-r * 8.0) * 0.6) * intensity * (1.0 + 0.3 * audio * g.bass);
    return vec4f(background.rgb + tint.rgb * light, 1.0);
}
