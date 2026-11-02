/*{
  "name": "My Shader",
  "category": "generator",
  "description": "A tunnel; turn audio up and it breathes with the bass and flashes on the kick. Change anything: it runs as you type",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 4.0 },
    { "id": "rings", "type": "float", "default": 8.0, "min": 1.0, "max": 32.0 },
    { "id": "tint", "type": "color", "default": [0.55, 0.35, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// uv runs 0..1 across the picture, px counts pixels. `g` holds time, aspect, bpm, beat_phase,
// bar_phase and the audio, 0..1: level, bass, mid, high, kick, snare, hat, melody. Each param above is
// a variable by its id. For an effect on another picture, start from a Shader FX node.
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = max(length(p), 0.001);
    // the bass pulls the walls in; the rings spiral round as they come
    let depth = 0.2 / r * (1.0 - 0.15 * audio * g.bass) + g.time * speed;
    let bands = 0.5 + 0.5 * sin(depth * rings + atan2(p.y, p.x) * 3.0);
    // bright edges on a dark wall, lit up on the kick; the far end fades into the dark
    let glow = (0.12 + 1.1 * pow(bands, 4.0)) * (1.0 + audio * g.kick);
    return vec4f(tint.rgb * glow * smoothstep(0.02, 0.35, r), 1.0);
}
