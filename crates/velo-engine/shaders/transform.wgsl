/*{
  "name": "Transform",
  "category": "effect",
  "description": "Zoom, rotate, offset, mirror or tile its input",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.1, "max": 8.0 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "offset", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "mirror", "type": "bool", "default": false },
    { "id": "tile", "type": "bool", "default": false },
    { "id": "spin", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "scroll", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // rotate: half-turns either way; spin: turns a second; scroll: screens a second
    let a = rotate * 3.14159265 + g.time * spin * 6.2831853;
    // the kick punches in
    let p = mat2x2f(cos(a), sin(a), -sin(a), cos(a)) * ((uv - 0.5) * vec2f(g.aspect, 1.0)) / (zoom * (1.0 + 0.15 * music * g.kick)) - offset - scroll * g.time;
    var q = p / vec2f(g.aspect, 1.0) + 0.5;
    if mirror {
        q = 1.0 - abs(fract(q * 0.5) * 2.0 - 1.0);
    }
    if tile {
        q = fract(q);
    }
    return textureSample(src, smp, q);
}
