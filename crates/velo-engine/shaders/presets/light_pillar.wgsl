/*{
  "name": "Light Pillar",
  "category": "generator",
  "description": "A pillar of light with fire winding up around it, one color at its head and another at its foot; the bass swells it, the kick makes it surge",
  "params": [
    { "id": "width", "type": "float", "default": 0.18, "min": 0.03, "max": 0.6 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "swirl", "type": "float", "default": 1.0, "min": -3.0, "max": 3.0 },
    { "id": "glow", "type": "float", "default": 0.6, "min": 0.0, "max": 2.0 },
    { "id": "tilt", "type": "float", "default": 0.0, "min": -0.25, "max": 0.25 },
    { "id": "color_a", "type": "color", "default": [0.32, 0.62, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.62, 0.99, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Light Pillar (its code isn't used: MIT + Commons Clause).

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

fn fbm(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s0 = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let a = tilt * 6.2831853;
    let s = vec2f(s0.x * cos(a) - s0.y * sin(a), s0.x * sin(a) + s0.y * cos(a));
    let t = g.time * speed;
    let w = width * (1.0 + 0.35 * audio * g.bass + 0.2 * audio * g.kick);
    // the pillar is round: across it is a way round it, its middle nearest the eye
    let x = s.x / w;
    let in_x = clamp(x, -1.0, 1.0);
    let z = sqrt(1.0 - in_x * in_x);
    let around = asin(in_x);
    // light wound round it and climbing: what's on the near side, and the far side through it
    let wind = s.y * swirl * 3.0 - t * 0.7;
    let front = fbm(vec2f(around * 1.5 + wind, s.y * 3.0 + t * 1.5));
    let back = fbm(vec2f(4.0 - around * 1.5 + wind, s.y * 3.0 + t * 1.5 + 7.0));
    let core = smoothstep(1.0, 0.85, abs(x));
    let body = core * (0.2 + 2.4 * smoothstep(0.3, 0.8, front) + 0.8 * smoothstep(0.35, 0.8, back)) * (0.4 + 0.6 * z);
    let rim = core * pow(1.0 - z, 2.0) * 0.6;
    let halo = exp(-max(abs(x) - 1.0, 0.0) * 2.5 / (0.3 + glow)) * glow * 0.5 * (1.0 - core);
    // rolled off, so a surge fills the pillar without burning it flat
    let light = 1.9 * (1.0 - exp(-(body + rim + halo) * (1.0 + 0.4 * audio * g.kick) * 0.6));
    // its head one color, its foot another, white where it's hottest
    let tint = mix(color_a.rgb, color_b.rgb, smoothstep(-0.5, 0.5, s.y));
    return vec4f(tint * light + vec3f(1.0) * max(light - 1.4, 0.0) * 0.3, 1.0);
}
