/*{
  "name": "Output",
  "category": "output",
  "description": "The last grade of what goes out: exposure, contrast, saturation, hue, colorize, invert, vignette, fade to black",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "exposure", "type": "float", "default": 0.0, "min": -3.0, "max": 3.0 },
    { "id": "contrast", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "saturation", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "hue", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "colorize", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "color", "type": "color", "default": [1.0, 0.45, 0.15, 1.0] },
    { "id": "invert", "type": "bool", "default": false },
    { "id": "vignette", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// a turn around the grey axis
fn hue_rotate(c: vec3f, turns: f32) -> vec3f {
    let a = turns * 6.2831853;
    let k = vec3f(0.57735);
    return c * cos(a) + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - cos(a));
}

// every step leaves the picture as it is at its default
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    // exposure in stops: black stays black
    var c = s.rgb * exp2(exposure);
    c = mix(vec3f(0.5), c, contrast);
    c = mix(vec3f(dot(c, vec3f(0.2126, 0.7152, 0.0722))), c, saturation);
    // skipped at 0: the GPU's sine and cosine are near, not exact
    if hue != 0.0 {
        c = hue_rotate(c, hue);
    }
    // colorize: the brightness from black through the color to white
    let y = dot(c, vec3f(0.2126, 0.7152, 0.0722));
    c = mix(c, select(mix(color.rgb, vec3f(1.0), 2.0 * y - 1.0), color.rgb * 2.0 * y, y < 0.5), colorize);
    if invert {
        c = 1.0 - c;
    }
    // darker toward the edges, then all of it toward black
    let e = uv * (1.0 - uv);
    c *= mix(1.0, pow(e.x * e.y * 16.0, 0.25), vignette) * (1.0 - fade);
    return vec4f(c, s.a);
}
