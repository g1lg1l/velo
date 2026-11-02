/*{
  "name": "Wave Warp",
  "category": "effect",
  "description": "The picture rippling in waves across, down, both ways or out from the middle; the bass makes them deeper.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "depth", "type": "float", "default": 0.015, "min": 0.0, "max": 0.2 },
    { "id": "waves", "type": "float", "default": 6.0, "min": 0.5, "max": 40.0 },
    { "id": "speed", "type": "float", "default": 1.0, "min": -4.0, "max": 4.0 },
    { "id": "direction", "type": "int", "default": 0, "names": ["Across", "Down", "Both", "Out"] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let d = depth * (1.0 + 2.0 * music * g.bass);
    let t = g.time * speed;
    let k = waves * 6.2831853;
    var o = vec2f(0.0);
    if direction == 0 {
        o.x = sin(uv.y * k + t);
    } else if direction == 1 {
        o.y = sin(uv.x * k + t);
    } else if direction == 2 {
        o = vec2f(sin(uv.y * k + t), sin(uv.x * k * 0.8 - t * 1.3));
    } else {
        let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
        o = normalize(p + 0.0001) / vec2f(g.aspect, 1.0) * sin(length(p) * k - t);
    }
    return textureSampleLevel(src, smp, uv + o * d, 0.0);
}
