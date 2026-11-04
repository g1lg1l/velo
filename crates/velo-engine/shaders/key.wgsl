/*{
  "name": "Key",
  "category": "effect",
  "description": "Cuts a color out of the picture (a green screen), or its dark or its bright parts, so what's under it shows through; spill takes the color's tint off the edges left",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "mode", "type": "int", "default": 0, "names": ["Color", "Dark", "Bright"] },
    { "id": "color", "type": "color", "default": [0.0, 1.0, 0.0, 1.0] },
    { "id": "tolerance", "type": "float", "default": 0.25, "min": 0.0, "max": 1.0 },
    { "id": "softness", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "spill", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "invert", "type": "bool", "default": false }
  ]
}*/

// a color's blue and red difference (YCbCr's Cb, Cr): how it's tinted, whatever its brightness
fn chroma(c: vec3f) -> vec2f {
    let y = dot(c, vec3f(0.2126, 0.7152, 0.0722));
    return vec2f((c.b - y) / 1.8556, (c.r - y) / 1.5748);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = textureSample(src, smp, uv);
    // the picture's color unpremultiplied, to compare it
    let rgb = c.rgb / max(c.a, 0.0001);
    let luma = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
    // how far from what's cut out: the key color's tint (its hue, so a screen in shadow or
    // washed out still counts; grays never do), black or white
    let pc = chroma(rgb);
    let kc = chroma(color.rgb);
    var far = 2.0;
    if length(pc) > 0.0001 && length(kc) > 0.0001 {
        let hue = acos(clamp(dot(normalize(pc), normalize(kc)), -1.0, 1.0)) / 3.14159265;
        let tinted = length(pc) / max(luma, 0.05);
        far = hue * 2.0 + max(0.25 - tinted, 0.0) * 4.0;
    }
    if mode == 1 {
        far = luma;
    } else if mode == 2 {
        far = 1.0 - luma;
    }
    var keep = smoothstep(tolerance, tolerance + softness + 0.0001, far);
    if invert {
        keep = 1.0 - keep;
    }
    // near the key color, its tint taken off toward gray
    var tint = rgb;
    if mode == 0 {
        let near = 1.0 - smoothstep(tolerance, tolerance + softness + 0.25, far);
        tint = mix(rgb, vec3f(luma), near * spill);
    }
    // what's cut out is clear: a Layer Stack or a Blend shows what's under it
    let a = c.a * keep;
    return vec4f(tint * a, a);
}
