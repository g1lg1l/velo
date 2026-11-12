/*{
  "name": "Pixel Blast",
  "category": "generator",
  "description": "Clouds drawn in big dithered pixels, drifting, rings rippling out through them; the bass thickens them, the kick sends a ring out",
  "params": [
    { "id": "pixel", "type": "float", "default": 6.0, "min": 2.0, "max": 32.0 },
    { "id": "scale", "type": "float", "default": 2.0, "min": 0.5, "max": 8.0 },
    { "id": "density", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "ripples", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "shape", "type": "int", "default": 0, "names": ["Square", "Circle", "Diamond"] },
    { "id": "tint", "type": "color", "default": [0.7, 0.55, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.0, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Pixel Blast (its code isn't used: MIT + Commons Clause).

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
    let block = max(floor(pixel * g.resolution.y / 540.0), 2.0);
    let cell = floor(px / block);
    let c = ((cell + 0.5) * block / g.resolution - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    // clouds drifting, rings rippling out from the middle, the kick's own ring
    let r = length(c);
    var v = fbm(c * scale + vec2f(t * 0.4, -t * 0.25));
    v += 0.12 * ripples * sin(r * 24.0 - g.time * 2.0) * exp(-r * 2.0);
    v += 0.35 * audio * g.kick * exp(-abs(r - (1.0 - g.kick) * 0.9) * 10.0);
    v = (v - 0.5) * 2.6 + density + 0.08 * audio * g.bass;
    // dithered: a pixel is lit where the cloud is over its place in a 4 × 4 Bayer pattern
    let b = vec2u(cell) % vec2u(4u);
    let low = ((b.x & 1u) ^ (b.y & 1u)) * 2u + (b.y & 1u);
    let high = ((b.x >> 1u) ^ (b.y >> 1u)) * 2u + (b.y >> 1u);
    let on = step((f32(low * 4u + high) + 0.5) / 16.0, v);
    let f = fract(px / block) - 0.5;
    var d = max(abs(f.x), abs(f.y));
    if shape == 1 {
        d = length(f);
    } else if shape == 2 {
        d = (abs(f.x) + abs(f.y)) * 0.75;
    }
    let lamp = 1.0 - smoothstep(0.42 - min(1.0 / block, 0.2), 0.42, d);
    let glint = 1.0 + audio * g.hat * step(0.9, hash(cell + floor(g.time * 12.0) * 7.0));
    let col = background.rgb + tint.rgb * on * lamp * (0.5 + 0.5 * clamp(v, 0.0, 1.0)) * glint * (1.0 + 0.2 * audio * g.kick);
    return vec4f(col, 1.0);
}
