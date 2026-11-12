/*{
  "name": "Galaxy",
  "category": "generator",
  "description": "A spiral galaxy turning slowly, its core glowing with the bass",
  "params": [
    { "id": "arms", "type": "int", "default": 2, "min": 1, "max": 6 },
    { "id": "twist", "type": "float", "default": 3.0, "min": 0.5, "max": 8.0 },
    { "id": "speed", "type": "float", "default": 0.05, "min": -1.0, "max": 1.0 },
    { "id": "tilt", "type": "float", "default": 0.35, "min": 0.0, "max": 0.9 },
    { "id": "color_a", "type": "color", "default": [0.35, 0.55, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.75, 0.45, 1.0] },
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

fn fbm(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 5; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn turn(p: vec2f, a: f32) -> vec2f {
    return vec2f(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // sized by the narrower side, so a tall picture holds it too
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0) / min(g.aspect, 1.0);
    // seen at an angle: the disc squashed into an ellipse
    var q = turn(s, 0.4);
    q.y /= 1.0 - tilt;
    let r = length(q);
    let spin = g.time * speed;
    let a = atan2(q.y, q.x) - spin;
    // arms along logarithmic spirals, dust turning with them
    let arm = pow(0.5 + 0.5 * cos(f32(arms) * a - twist * log(r + 0.001)), 3.0);
    let dust = fbm(turn(q, -spin) * 6.0);
    let lanes = 0.55 + 0.45 * smoothstep(0.3, 0.7, fbm(turn(q, -spin) * 11.0 + 3.0));
    // the bass swells the whole disc, the kick flares the core
    let swell = 1.0 + 0.5 * audio * g.bass;
    let density = exp(-r * 2.3) * (0.12 + arm * (0.5 + 1.2 * dust)) * lanes * swell;
    let core = (exp(-r * 13.0) + 0.25 * exp(-r * 4.5)) * (1.0 + 0.6 * audio * g.bass + 1.2 * audio * g.kick);
    var col = mix(color_b.rgb, color_a.rgb, smoothstep(0.0, 0.3, r)) * density * 3.2 + color_b.rgb * core * 1.6;
    // knots of young stars strung along the arms, in the two colors' blend
    col += mix(color_a.rgb, color_b.rgb, 0.5).brg * pow(arm * dust * 1.6, 4.0) * exp(-r * 2.0) * 0.9 * swell;
    // stars, in the arms and all around
    let cell = s * 160.0;
    let star = step(0.975 - arm * density * 0.15, hash(cell)) * (1.0 - smoothstep(0.05, 0.22, length(fract(cell) - 0.5)));
    let warmth = hash(cell + 7.0);
    col += mix(color_a.rgb, color_b.rgb, warmth) * 0.5 * star + star * (0.3 + 0.5 * warmth) * (1.0 + 1.5 * audio * g.hat * (hash(cell + floor(g.time * 12.0)) - 0.3));
    return vec4f(1.0 - exp(-col * 1.2), 1.0);
}
