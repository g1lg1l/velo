/*{
  "name": "Waves",
  "category": "generator",
  "description": "Glowing sine lines like a waveform, swelling with the bass",
  "params": [
    { "id": "lines", "type": "int", "default": 8, "min": 1, "max": 24 },
    { "id": "amplitude", "type": "float", "default": 0.15, "min": 0.0, "max": 0.5 },
    { "id": "frequency", "type": "float", "default": 3.0, "min": 0.5, "max": 20.0 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "width", "type": "float", "default": 2.0, "min": 0.5, "max": 10.0 },
    { "id": "color_a", "type": "color", "default": [0.2, 0.6, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.3, 0.6, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// one line's height across the picture: three sines, each line a little out of step
fn wave(x: f32, i: f32, t: f32) -> f32 {
    return sin(x * frequency + t + i * 0.7) * 0.6 + sin(x * frequency * 2.3 - t * 1.3 + i * 1.3) * 0.3 + sin(x * frequency * 5.1 + t * 2.1 + i) * 0.1;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed * 3.0;
    let amp = amplitude * (1.0 + 0.8 * audio * g.bass);
    // quieter toward the sides
    let edge = s.x / (g.aspect * 0.5);
    let taper = 1.0 - edge * edge * 0.7;
    let w = width * 0.0005;
    let aa = 1.0 / g.resolution.y;
    var col = vec3f(0.0);
    for (var i = 0; i < lines; i++) {
        let f = f32(i);
        let k = (f + 0.5) / f32(lines);
        let jitter = 0.01 * audio * g.hat * sin(s.x * 90.0 + g.time * 40.0 + f);
        let y = (k - 0.5) * 0.12 + amp * taper * wave(s.x, f, t) + jitter;
        let dy = (amp * taper * wave(s.x + 0.001, f, t) - amp * taper * wave(s.x, f, t)) / 0.001;
        let d = abs(s.y - y) / sqrt(1.0 + dy * dy);
        let core = 1.0 - smoothstep(w, w + aa, d);
        col += mix(color_a.rgb, color_b.rgb, k) * (core + exp(-d / (w * 6.0 + 0.004)) * 0.35);
    }
    return vec4f(col * (1.0 + 0.3 * audio * g.kick), 1.0);
}
