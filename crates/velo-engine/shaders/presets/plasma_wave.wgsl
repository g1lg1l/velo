/*{
  "name": "Plasma Wave",
  "category": "generator",
  "description": "Two ribbons of plasma curling through each other, twisting as they go, with a soft glow and white-hot cores; the bass swings them wider, the kick fattens them",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "amplitude", "type": "float", "default": 0.22, "min": 0.0, "max": 0.45 },
    { "id": "frequency", "type": "float", "default": 2.2, "min": 0.5, "max": 10.0 },
    { "id": "thickness", "type": "float", "default": 1.0, "min": 0.2, "max": 4.0 },
    { "id": "glow", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "color_a", "type": "color", "default": [0.65, 0.3, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.2, 0.75, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Plasma Wave (its code isn't used: MIT + Commons Clause).

// a ribbon's height at x: two sines that never line up twice the same way
fn ribbon(x: f32, t: f32, phase: f32) -> f32 {
    return sin(x * frequency + t + phase) * 0.7 + sin(x * frequency * 0.53 - t * 0.7 + phase * 1.7) * 0.3;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed * 2.0;
    let amp = amplitude * (1.0 + 0.6 * audio * g.bass);
    var col = background.rgb;
    for (var i = 0; i < 2; i++) {
        let phase = f32(i) * 3.1415927;
        let y = amp * ribbon(s.x, t, phase);
        let slope = (amp * ribbon(s.x + 0.002, t, phase) - y) / 0.002;
        // the distance across the ribbon, not straight down: the same width on a slope
        let d = abs(s.y - y) / sqrt(1.0 + slope * slope);
        // seen edge on it thins, face on it widens: it twists as it goes
        let face = 0.25 + 0.75 * abs(cos(s.x * frequency * 0.5 + t * 0.5 + phase * 0.5));
        let w = thickness * 0.035 * face * (1.0 + 0.5 * audio * g.kick);
        let body = exp(-d * d / (w * w));
        let core = exp(-d * d / (w * w * 0.04));
        let halo = exp(-d / (w * 6.0 + 0.02)) * 0.35 * glow;
        let shimmer = 1.0 + 0.3 * audio * g.hat * sin(g.time * 40.0 + phase);
        col += select(color_b.rgb, color_a.rgb, i == 0) * (body * 0.9 + halo) * shimmer + vec3f(1.0) * core * 0.9;
    }
    return vec4f(col * (1.0 + 0.4 * audio * g.kick), 1.0);
}
