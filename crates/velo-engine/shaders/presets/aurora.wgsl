/*{
  "name": "Aurora",
  "category": "generator",
  "description": "Northern lights: curtains of light folding over a starry sky",
  "params": [
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "height", "type": "float", "default": 0.3, "min": 0.0, "max": 0.8 },
    { "id": "color_a", "type": "color", "default": [0.1, 1.0, 0.55, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.6, 0.2, 1.0, 1.0] },
    { "id": "sky", "type": "color", "default": [0.0, 0.03, 0.1, 1.0] },
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
    // y up, 0 at the bottom
    let p = vec2f((uv.x - 0.5) * g.aspect, 1.0 - uv.y);
    let t = g.time * speed;
    var col = sky.rgb * (1.4 - p.y);
    // stars, twinkling with the hats
    let cell = p * 90.0;
    let star = hash(cell);
    let spot = length(fract(cell) - 0.5 - (vec2f(hash(cell + 41.0), hash(cell + 73.0)) - 0.5) * 0.6);
    let twinkle = 0.6 + 0.4 * sin(g.time * 3.0 + star * 60.0) + audio * g.hat * 0.8;
    col += step(0.97, star) * (1.0 - smoothstep(0.0, 0.12, spot)) * twinkle * smoothstep(0.1, 0.6, p.y) * 0.8;
    // three curtains, each hanging from a folding line, rays rising from it
    var light = vec3f(0.0);
    let reach = 0.22 * (1.0 + 0.4 * audio * g.kick);
    for (var i = 0; i < 3; i++) {
        let f = f32(i);
        let x = p.x * (1.0 - 0.2 * f) + f * 5.3;
        let foot = height + 0.1 * f + 0.4 * (noise(vec2f(x * 1.6 + t * 0.4, f * 3.0)) - 0.5) + 0.07 * sin(x * 3.0 + t * 0.7 + f);
        let d = p.y - foot;
        let rays = noise(vec2f(x * 16.0 + t * 0.8 + noise(vec2f(x * 2.0, t * 0.3)) * 6.0, f * 7.0));
        let shimmer = 1.0 + audio * g.hat * (noise(vec2f(x * 40.0, g.time * 12.0)) - 0.5);
        let curtain = pow(smoothstep(-0.1, 0.01, d), 2.0) * exp(-max(d, 0.0) / reach) * (0.2 + rays * rays * 1.3) * shimmer;
        light += mix(color_a.rgb, color_b.rgb, smoothstep(0.0, reach * 1.5, d)) * curtain * (1.0 - 0.25 * f);
    }
    col += light * 0.7 * (1.0 + 0.5 * audio * g.bass);
    return vec4f(col, 1.0);
}
