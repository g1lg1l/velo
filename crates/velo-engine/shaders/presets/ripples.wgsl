/*{
  "name": "Ripples",
  "category": "generator",
  "description": "Drops falling on still water, the rings crossing each other",
  "params": [
    { "id": "drops", "type": "int", "default": 4, "min": 1, "max": 8 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "frequency", "type": "float", "default": 40.0, "min": 10.0, "max": 120.0 },
    { "id": "color_a", "type": "color", "default": [0.0, 0.08, 0.2, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.3, 0.75, 0.95, 1.0] },
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

// the water's height: each drop's ring of waves spreads and fades, then it falls again elsewhere
fn water(p: vec2f, t: f32) -> f32 {
    var h = 0.0;
    for (var i = 0; i < drops; i++) {
        let f = f32(i);
        let life = t + f / f32(drops);
        let n = floor(life);
        let k = fract(life);
        let at = (vec2f(hash(vec2f(n, f)), hash(vec2f(f + 19.0, n))) - 0.5) * vec2f(g.aspect, 1.0) * 0.8;
        let x = length(p - at) - k * 0.9;
        h += sin(x * frequency) * exp(-x * x * 40.0) * (1.0 - k) * (1.0 - k);
    }
    return h;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed * 0.3;
    let e = 0.002;
    let h = water(p, t);
    let slope = vec2f(water(p + vec2f(e, 0.0), t) - h, water(p + vec2f(0.0, e), t) - h) / e;
    // the waves grow on the kick
    let lift = 0.012 * (1.0 + audio * g.kick);
    let normal = normalize(vec3f(-slope * lift, 1.0));
    let sun = normalize(vec3f(-0.4, -0.5, 1.0));
    let lit = max(dot(normal, sun), 0.0);
    let shine = pow(max(dot(reflect(-sun, normal), vec3f(0.0, 0.0, 1.0)), 0.0), 60.0 - 30.0 * audio * g.hat);
    var col = mix(color_a.rgb, color_b.rgb, smoothstep(0.85, 1.0, lit) * 0.8 + 0.2 * (1.0 - uv.y));
    col += vec3f(shine) * 0.8;
    return vec4f(col * (1.0 + 0.15 * audio * g.bass), 1.0);
}
