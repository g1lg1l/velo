/*{
  "name": "Iridescence",
  "category": "generator",
  "description": "The shifting rainbow of a soap film or an oil slick",
  "params": [
    { "id": "scale", "type": "float", "default": 1.5, "min": 0.3, "max": 6.0 },
    { "id": "speed", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "thickness", "type": "float", "default": 0.7, "min": 0.2, "max": 3.0 },
    { "id": "shine", "type": "float", "default": 0.85, "min": 0.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.05, 0.04, 0.12, 1.0] },
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
    for (var i = 0; i < 3; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let p = s * scale;
    let t = g.time * speed;
    // the film's thickness, in micrometers: smooth noise, bent by itself
    let w = vec2f(fbm(p + vec2f(t, 0.0)), fbm(p + vec2f(4.1, 1.7 - t)));
    let f = fbm(p + w * 2.5 + vec2f(0.0, t * 0.5));
    let shimmer = 0.012 * audio * g.hat * (noise(p * 25.0 + g.time * 6.0) - 0.5);
    // thicker toward the bottom, where the film drains
    let d = thickness * (0.2 + f * 0.3 + uv.y * 0.15) + 0.05 * audio * g.bass + shimmer;
    // light off the film's two faces interferes: each wavelength (red, green, blue, in
    // micrometers) comes out bright at its own thicknesses
    let film = 0.5 + 0.5 * cos(6.2831853 * 2.0 * 1.33 * d / vec3f(0.65, 0.53, 0.45));
    // our eyes blend neighboring wavelengths: softer than pure red, green and blue
    let soft = mix(vec3f((film.r + film.g + film.b) / 3.0), film, 0.7);
    let col = tint.rgb + soft * shine * (0.5 + 0.7 * f);
    return vec4f(col * (1.0 + 0.2 * audio * g.kick), 1.0);
}
