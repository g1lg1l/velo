/*{
  "name": "Strands",
  "category": "generator",
  "description": "Fine glowing threads weaving round each other like a braid, swelling with the bass",
  "params": [
    { "id": "strands", "type": "int", "default": 28, "min": 2, "max": 64 },
    { "id": "spread", "type": "float", "default": 0.22, "min": 0.0, "max": 0.5 },
    { "id": "twist", "type": "float", "default": 2.2, "min": 0.0, "max": 8.0 },
    { "id": "amplitude", "type": "float", "default": 0.08, "min": 0.0, "max": 0.4 },
    { "id": "frequency", "type": "float", "default": 2.4, "min": 0.3, "max": 12.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.3, "max": 6.0 },
    { "id": "glow", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.35, 0.55, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.35, 0.75, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// a thread's height at x: where it sits in the braid (turning round its middle as x and time
// go), then its own slow wave on top, each thread a little out of step with the next
fn strand(x: f32, i: f32, k: f32, t: f32, amp: f32) -> f32 {
    let turn = x * twist + t * 0.6 + i * 2.399963;
    let braid = spread * (k - 0.5) * cos(turn) + spread * 0.18 * sin(turn * 0.5 + i);
    let wave = sin(x * frequency + t + i * 0.37) * 0.6 + sin(x * frequency * 2.1 - t * 1.4 + i * 1.7) * 0.3 + sin(x * frequency * 4.3 + t * 2.2 + i * 0.9) * 0.1;
    return braid + amp * wave;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed * 3.0;
    let amp = amplitude * (1.0 + 0.9 * audio * g.bass);
    let w = width * 0.00035;
    let aa = 1.0 / g.resolution.y;
    var col = vec3f(0.0);
    for (var n = 0; n < strands; n++) {
        let i = f32(n);
        let k = (i + 0.5) / f32(strands);
        let y = strand(s.x, i, k, t, amp);
        // the distance to the thread, across it (not straight down): the same width on a slope
        let slope = (strand(s.x + 0.001, i, k, t, amp) - y) / 0.001;
        let d = abs(s.y - y) / sqrt(1.0 + slope * slope);
        // nearer threads of the braid brighter: the ones turning toward the eye
        let depth = 0.55 + 0.45 * sin(s.x * twist + t * 0.6 + i * 2.399963);
        let core = 1.0 - smoothstep(w, w + aa, d);
        let halo = exp(-d / (w * 8.0 + 0.003)) * glow * 0.3;
        let shimmer = 1.0 + 0.5 * audio * g.hat * sin(s.x * 60.0 + g.time * 30.0 + i);
        col += mix(color_a.rgb, color_b.rgb, k) * (core + halo) * depth * shimmer;
    }
    return vec4f(col * (1.0 + 0.35 * audio * g.kick), 1.0);
}
