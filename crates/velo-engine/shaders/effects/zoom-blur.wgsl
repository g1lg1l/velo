/*{
  "name": "Zoom Blur",
  "category": "effect",
  "description": "The picture streaks out from a point as if rushing at you, harder with the bass with music up.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "strength", "type": "float", "default": 0.15, "min": 0.0, "max": 0.6 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = 0.5 + center * 0.5;
    let k = strength * (1.0 + 2.0 * music * g.bass);
    // 24 copies, each a little nearer the point, averaged
    // each pixel starts at its own fraction of a step: the copies melt into one smear
    let jitter = fract(sin(dot(px, vec2f(12.9898, 78.233))) * 43758.5453);
    var sum = vec4f(0.0);
    for (var i = 0; i < 24; i++) {
        let s = 1.0 - k * (f32(i) + jitter) / 24.0;
        sum += textureSampleLevel(src, smp, c + (uv - c) * s, 0.0);
    }
    return sum / 24.0;
}
