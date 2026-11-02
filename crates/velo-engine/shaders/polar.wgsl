/*{
  "name": "Polar",
  "category": "effect",
  "description": "Wraps the picture round its middle (its bottom edge at the center, a tiny planet) or unrolls a round one into a strip; turned, twisted into a spiral, repeated round",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "mode", "type": "int", "default": 0, "names": ["Wrap round", "Unroll"] },
    { "id": "turn", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "twist", "type": "float", "default": 0.0, "min": -2.0, "max": 2.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
    { "id": "repeat", "type": "int", "default": 1, "min": 1, "max": 8 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the kick twists it a little more
    let spiral = twist + 0.3 * music * g.kick;
    if mode == 1 {
        // across is the way round, down is out from the middle
        let r = (1.0 - uv.y) * 0.5 / zoom;
        let a = (uv.x / f32(repeat) + turn + spiral * r) * 6.2831853;
        let q = 0.5 + vec2f(sin(a), -cos(a)) * r / vec2f(g.aspect, 1.0);
        return textureSampleLevel(src, smp, q, 0.0);
    }
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // 0 in the middle, 1 at the top and bottom edges
    let r = length(p) * 2.0;
    let a = atan2(p.x, -p.y) / 6.2831853 + 0.5 + turn + spiral * r;
    let q = vec2f(fract(a * f32(repeat)), 1.0 - r / zoom);
    return textureSampleLevel(src, smp, clamp(q, vec2f(0.0), vec2f(1.0)), 0.0);
}
