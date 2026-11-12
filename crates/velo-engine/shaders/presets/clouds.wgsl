/*{
  "name": "Clouds",
  "category": "generator",
  "description": "Fractal clouds drifting across the sky, lit from above",
  "params": [
    { "id": "scale", "type": "float", "default": 2.0, "min": 0.5, "max": 8.0 },
    { "id": "speed", "type": "float", "default": 0.08, "min": 0.0, "max": 1.0 },
    { "id": "cover", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "sky", "type": "color", "default": [0.2, 0.45, 0.85, 1.0] },
    { "id": "cloud", "type": "color", "default": [1.0, 0.98, 0.95, 1.0] },
    { "id": "shadow", "type": "color", "default": [0.45, 0.5, 0.62, 1.0] },
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

// six octaves, each turned a little so no grid shows
fn fbm(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 6; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let t = g.time * speed;
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale + vec2f(t, t * 0.3);
    let n = fbm(p + vec2f(0.0, t * 0.2));
    let c = cover + 0.2 * audio * g.bass;
    let low = 0.62 - 0.4 * c;
    let density = smoothstep(low, low + 0.25, n);
    // thinner toward the light: brighter there
    let toward = fbm(p + vec2f(0.0, t * 0.2) + vec2f(-0.04, -0.07));
    let lit = clamp((n - toward) * 6.0 + 0.6, 0.0, 1.0);
    let shade = mix(shadow.rgb, cloud.rgb, lit) * (1.0 + 0.4 * audio * g.kick);
    let air = mix(sky.rgb * 1.15, sky.rgb * 0.75, uv.y);
    return vec4f(mix(air, shade, density), 1.0);
}
