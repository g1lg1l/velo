/*{
  "name": "Kaleido",
  "category": "effect",
  "description": "Folds its input into mirrored wedges",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "segments", "type": "int", "default": 6, "min": 2, "max": 24 },
    { "id": "spin", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
    { "id": "angle", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -0.5, "max": 0.5 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = 0.5 + center;
    let p = (uv - c) * vec2f(g.aspect, 1.0);
    let sector = 6.2831853 / f32(segments);
    // the kick turns the wedges a little, the bass draws them in
    var a = atan2(p.y, p.x) + g.time * spin + (angle + 0.03 * music * g.kick) * 6.2831853;
    // fold every wedge onto the first one, mirrored
    a = abs(fract(a / sector + 0.5) - 0.5) * sector;
    let q = vec2f(cos(a), sin(a)) * length(p) / (zoom * (1.0 + 0.2 * music * g.bass)) / vec2f(g.aspect, 1.0) + c;
    return textureSample(src, smp, q);
}
