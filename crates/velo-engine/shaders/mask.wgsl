/*{
  "name": "Mask",
  "category": "composite",
  "description": "The picture only where the mask is (keep), cut out there (cut), or multiplied by the mask",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "mask", "type": "texture" }],
  "params": [
    { "id": "mode", "type": "int", "default": 0, "names": ["Keep", "Cut", "Multiply"] },
    { "id": "source", "type": "int", "default": 0, "names": ["Luma", "Alpha"] },
    { "id": "threshold", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "softness", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "invert", "type": "bool", "default": false }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = textureSample(src, smp, uv);
    let m = textureSample(mask, smp, uv);
    // multiply: the mask's own colors over the picture
    if mode == 2 {
        return vec4f(c.rgb * m.rgb, c.a);
    }
    // how much the mask covers here: its brightness or its alpha, through the threshold
    var v = select(dot(m.rgb, vec3f(0.2126, 0.7152, 0.0722)), m.a, source == 1);
    let s = softness * 0.5 + 0.0001;
    v = smoothstep(threshold - s, threshold + s, v);
    if invert {
        v = 1.0 - v;
    }
    if mode == 1 {
        v = 1.0 - v;
    }
    // what's left out is clear: a Layer Stack or a Blend shows what's under it
    return c * v;
}
