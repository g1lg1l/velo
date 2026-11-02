/*{
  "name": "Levels",
  "category": "effect",
  "description": "Where black and white are, the midtones' gamma, an S-curve and the range out: on every channel or one",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "black", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "white", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "gamma", "type": "float", "default": 1.0, "min": 0.1, "max": 4.0 },
    { "id": "curve", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "out_black", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "out_white", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "channel", "type": "int", "default": 0, "names": ["All", "Red", "Green", "Blue"] }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    let rgb = s.rgb / max(s.a, 0.0001);
    var c = clamp((rgb - black) / max(white - black, 0.0001), vec3f(0.0), vec3f(1.0));
    c = pow(c, vec3f(1.0 / gamma));
    // up: an S, more contrast in the middle; down: its opposite, flatter
    let s_curve = c * c * (3.0 - 2.0 * c);
    c = mix(c, select(2.0 * c - s_curve, s_curve, curve > 0.0), abs(curve));
    c = mix(vec3f(out_black), vec3f(out_white), c);
    let only = select(vec3f(f32(channel == 1), f32(channel == 2), f32(channel == 3)), vec3f(1.0), channel == 0);
    return vec4f(mix(rgb, c, only) * s.a, s.a);
}
