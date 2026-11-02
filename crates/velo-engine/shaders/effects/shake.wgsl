/*{
  "name": "Shake",
  "category": "effect",
  "description": "A handheld camera: the picture jitters and tilts, and with music up every kick jolts it.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "shake", "type": "float", "default": 0.01, "min": 0.0, "max": 0.1 },
    { "id": "tilt", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 6.0, "min": 0.5, "max": 30.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// smooth wandering -1..1, a sum of unrelated sines
fn wander(t: f32, seed: f32) -> f32 {
    return (sin(t * 1.0 + seed) + 0.6 * sin(t * 2.3 + seed * 3.1) + 0.3 * sin(t * 5.7 + seed * 7.7)) / 1.9;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let t = g.time * speed;
    let amount_now = shake + 0.05 * music * g.kick;
    let move_by = vec2f(wander(t, 1.0), wander(t, 4.0)) * amount_now;
    let a = wander(t, 9.0) * amount_now * tilt * 3.0;
    // zoomed in a little so the edges never show
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) / (1.0 + amount_now * 2.0);
    let q = vec2f(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a)) + move_by;
    return textureSampleLevel(src, smp, q / vec2f(g.aspect, 1.0) + 0.5, 0.0);
}
