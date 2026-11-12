/*{
  "name": "Dither Waves",
  "category": "generator",
  "description": "Waves rolling through each other, drawn like an old computer would: a few shades of one color, dithered in big pixels; the bass brightens them, the kick more",
  "params": [
    { "id": "pixel", "type": "float", "default": 3.0, "min": 1.0, "max": 16.0 },
    { "id": "scale", "type": "float", "default": 2.0, "min": 0.5, "max": 8.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "levels", "type": "int", "default": 4, "min": 2, "max": 8 },
    { "id": "color_a", "type": "color", "default": [0.03, 0.02, 0.1, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.55, 0.5, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Dither (its code isn't used: MIT + Commons Clause).

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
    // big pixels: `pixel` of the picture's at 540 lines
    let block = max(floor(pixel * g.resolution.y / 540.0), 1.0);
    let cell = floor(px / block);
    let c = ((cell + 0.5) * block / g.resolution - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    // noise bent by noise, then turned into rolling bands
    let w = fbm(c * scale + vec2f(t * 0.3, t * 0.2));
    let n = fbm(c * scale * 1.3 + vec2f(w * 2.5 - t * 0.4, w * 1.5 + t * 0.1));
    var v = mix(n, 0.5 + 0.5 * sin(n * 10.0 - t * 2.0), 0.6);
    v = clamp(((v - 0.5) * 1.4 + 0.5) * (1.0 + audio * (0.12 * g.bass + 0.25 * g.kick)), 0.0, 1.0);
    // a few shades, the steps between them dithered on a 4 × 4 Bayer pattern
    let b = vec2u(cell) % vec2u(4u);
    let low = ((b.x & 1u) ^ (b.y & 1u)) * 2u + (b.y & 1u);
    let high = ((b.x >> 1u) ^ (b.y >> 1u)) * 2u + (b.y >> 1u);
    let steps = f32(max(levels - 1, 1));
    let shade = floor(v * steps + (f32(low * 4u + high) + 0.5) / 16.0) / steps;
    // a little darker toward the corners, as on an old screen
    return vec4f(mix(color_a.rgb, color_b.rgb, shade) * (1.0 - 0.5 * dot(uv - 0.5, uv - 0.5)), 1.0);
}
