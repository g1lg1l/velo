/*{
  "name": "Displace",
  "category": "effect",
  "description": "Moves the picture by a map: its red and green as a vector, or its brightness along an angle",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "params": [
    { "id": "strength", "type": "float", "default": 0.05, "min": 0.0, "max": 0.5 },
    { "id": "mode", "type": "int", "default": 0, "names": ["Vector", "Luma"] },
    { "id": "angle", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(map, smp, uv);
    var m = s.rg - 0.5;
    if mode == 1 {
        let a = angle * 6.2831853;
        m = vec2f(cos(a), sin(a)) * (dot(s.rgb, vec3f(0.2126, 0.7152, 0.0722)) - 0.5);
    }
    // the bass bends it further
    return textureSample(src, smp, uv + m * strength * (1.0 + 2.0 * music * g.bass));
}
