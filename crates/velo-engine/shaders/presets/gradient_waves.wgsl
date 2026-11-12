/*{
  "name": "Gradient Waves",
  "category": "generator",
  "description": "A sea of satin waves rolling in from a hazy horizon, catching the light on their backs; the bass raises them, the kick brightens the swell",
  "params": [
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "height", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "scale", "type": "float", "default": 1.8, "min": 0.3, "max": 4.0 },
    { "id": "horizon", "type": "float", "default": 0.25, "min": -0.4, "max": 0.5 },
    { "id": "color_a", "type": "color", "default": [0.3, 0.15, 0.85, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.45, 0.7, 1.0] },
    { "id": "haze", "type": "color", "default": [0.55, 0.5, 0.85, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Gradient Waves (its code isn't used: MIT + Commons Clause).

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

// the sea's height at a place on it (x across, y into the distance)
fn sea(w: vec2f, t: f32) -> f32 {
    return 0.5 * sin(w.y * 1.2 + t + 0.6 * sin(w.x * 0.5 + t * 0.3)) + 0.3 * sin(w.y * 2.3 - w.x * 0.8 + t * 1.4) + 0.2 * noise(w * 0.9 + vec2f(0.0, t * 0.5)) + 0.12 * sin(w.x * 2.1 + w.y * 3.7 - t * 1.8);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // y up
    let s = vec2f((uv.x - 0.5) * g.aspect, 0.5 - uv.y);
    let down = horizon - s.y;
    if down <= 0.0 {
        // the sky: haze at the horizon, the sea's own color higher up
        return vec4f(mix(haze.rgb, color_a.rgb * 0.3, smoothstep(0.0, 0.6, -down)), 1.0);
    }
    let z = 0.35 / down;
    let w = vec2f(s.x * z, z) * scale * 2.0;
    let t = g.time * speed * 2.0;
    // the slope, measured over more of the sea the further off: far waves blur, not shimmer
    let e = 0.02 * z;
    let h = sea(w, t);
    let lift = height * 0.9 * (1.0 + 0.6 * audio * g.bass);
    let n = normalize(vec3f(-(sea(w + vec2f(e, 0.0), t) - h) / e * lift, 1.0, -(sea(w + vec2f(0.0, e), t) - h) / e * lift));
    // a low light from beyond the horizon, the eye above the near shore
    let light = normalize(vec3f(0.3, 0.6, 0.7));
    let eye = normalize(vec3f(-s.x, down, -1.0));
    let diffuse = max(dot(n, light), 0.0);
    let gloss = pow(max(dot(n, normalize(light + eye)), 0.0), 60.0);
    var col = mix(color_a.rgb, color_b.rgb, 0.5 + 0.5 * h) * (0.08 + 1.15 * diffuse * diffuse) * (1.0 + 0.4 * audio * g.kick);
    col += mix(color_b.rgb, vec3f(1.0), 0.6) * gloss * 1.1;
    return vec4f(mix(haze.rgb, col, exp(-z * 0.2)), 1.0);
}
