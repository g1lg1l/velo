/*{
  "name": "Threshold",
  "category": "effect",
  "description": "Two colors: one where the picture is over a level, the other where it's under; by its brightness or a channel, with a soft edge",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "level", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "softness", "type": "float", "default": 0.02, "min": 0.0, "max": 0.5 },
    { "id": "measure", "type": "int", "default": 0, "names": ["Brightness", "Red", "Green", "Blue", "Alpha"] },
    { "id": "under", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "over", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    var v = dot(s.rgb, vec3f(0.2126, 0.7152, 0.0722));
    switch measure {
        case 1: { v = s.r; }
        case 2: { v = s.g; }
        case 3: { v = s.b; }
        case 4: { v = s.a; }
        default: {}
    }
    // the bass lowers the bar: more of the picture gets over it
    let l = level - 0.2 * music * g.bass;
    let c = mix(under, over, smoothstep(l - softness - 0.0001, l + softness, v));
    return vec4f(c.rgb * c.a, c.a);
}
