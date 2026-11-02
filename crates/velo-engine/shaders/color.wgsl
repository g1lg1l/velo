/*{
  "name": "Color",
  "category": "effect",
  "description": "Brightness, contrast, saturation, hue, invert",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "brightness", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "contrast", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "saturation", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "hue", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "gamma", "type": "float", "default": 1.0, "min": 0.2, "max": 3.0 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "posterize", "type": "int", "default": 0, "min": 0, "max": 16 },
    { "id": "invert", "type": "bool", "default": false },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// a turn around the grey axis
fn hue_rotate(c: vec3f, turns: f32) -> vec3f {
    let a = turns * 6.2831853;
    let k = vec3f(0.57735);
    return c * cos(a) + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - cos(a));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    var c = hue_rotate(s.rgb, hue);
    // the music lifts the colors and the kick flashes them
    c = mix(vec3f(dot(c, vec3f(0.2126, 0.7152, 0.0722))), c, saturation * (1.0 + 0.5 * music * g.level));
    c = (c - 0.5) * contrast + 0.5 + brightness + 0.25 * music * g.kick;
    // the tint's alpha is how much it tints
    c = pow(max(c, vec3f(0.0)), vec3f(1.0 / gamma)) * mix(vec3f(1.0), tint.rgb, tint.a);
    // a few flat levels per channel (0: off)
    if posterize > 1 {
        let n = f32(posterize);
        c = floor(c * n) / (n - 1.0);
    }
    if invert {
        c = 1.0 - c;
    }
    return vec4f(max(c, vec3f(0.0)), s.a);
}
