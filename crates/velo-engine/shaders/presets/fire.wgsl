/*{
  "name": "Fire",
  "category": "generator",
  "description": "Flames licking up from the bottom, rising higher with the bass and flaring on the kick",
  "params": [
    { "id": "height", "type": "float", "default": 0.55, "min": 0.1, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "turbulence", "type": "float", "default": 1.0, "min": 0.0, "max": 2.0 },
    { "id": "scale", "type": "float", "default": 3.0, "min": 1.0, "max": 10.0 },
    { "id": "color_a", "type": "color", "default": [0.9, 0.12, 0.02, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.7, 0.15, 1.0] },
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
        p = p * 2.03 + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // y up from the bottom edge
    let p = vec2f((uv.x - 0.5) * g.aspect, 1.0 - uv.y);
    let t = g.time * speed;
    // noise rising, bent sideways by more noise; finer licks on top, rising faster
    let q = p * scale;
    let bend = fbm(q * 0.5 + vec2f(0.0, -t * 0.6)) - 0.5;
    let x = q.x + bend * turbulence * 2.0;
    let big = fbm(vec2f(x, q.y * 1.4 - t * 2.2));
    let fine = fbm(vec2f(x * 2.6 + 7.0, q.y * 2.2 - t * 3.6));
    let n = big * (0.55 + 0.9 * fine);
    // tongues: some places along the bottom reach higher than others, and they move
    let tongues = 0.45 + noise(vec2f(q.x * 1.3, t * 0.5)) * 1.1;
    let reach = height * tongues * (1.0 + 0.5 * audio * g.bass);
    // hottest at the bottom, thinning out toward the tips
    let heat = clamp(n * 2.0 - p.y / max(reach, 0.01) - 0.05, 0.0, 1.3) * (1.0 + 0.5 * audio * g.kick);
    // dark red at the tips, through the two colors, to white at the heart
    var col = color_a.rgb * smoothstep(0.0, 0.45, heat) * (0.35 + 0.9 * heat);
    col = mix(col, color_b.rgb, smoothstep(0.4, 0.9, heat));
    col += vec3f(1.0, 0.95, 0.85) * smoothstep(0.9, 1.3, heat) * 0.9;
    // the glow of the bed, and sparks carried up, more of them with the hats
    col += color_a.rgb * exp(-p.y * 7.0) * 0.35;
    let s = vec2f(p.x * 26.0 + 2.0 * sin(p.y * 5.0 + t), p.y * 14.0 - t * 5.0);
    let cell = floor(s);
    let h = hash(cell);
    let at = vec2f(hash(cell + 31.0), hash(cell + 57.0)) * 0.6 + 0.2;
    let spark = smoothstep(0.16, 0.0, length((fract(s) - at) * vec2f(1.0, 0.55))) * step(0.93 - 0.1 * audio * g.hat, h);
    col += color_b.rgb * spark * smoothstep(1.6, 0.2, p.y / max(height, 0.01)) * (0.5 + 0.5 * sin(g.time * 9.0 + h * 40.0)) * 1.5;
    return vec4f(col, 1.0);
}
