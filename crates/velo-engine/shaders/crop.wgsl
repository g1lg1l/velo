/*{
  "name": "Crop",
  "category": "effect",
  "description": "Cuts the picture's edges away: left where it was, stretched to fill the picture, or fitted in whole; see-through outside, with a soft edge",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "left", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "right", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "top", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "bottom", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "fit", "type": "int", "default": 0, "names": ["In place", "Fill", "Fit"] },
    { "id": "feather", "type": "float", "default": 0.0, "min": 0.0, "max": 0.2 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let lo = vec2f(left, top);
    let hi = max(vec2f(1.0 - right, 1.0 - bottom), lo + 0.001);
    let size = hi - lo;
    // filling, what's kept covers the whole picture: no edge to cut
    if fit == 1 {
        return textureSampleLevel(src, smp, lo + uv * size, 0.0);
    }
    var q = uv;
    if fit == 2 {
        // as big as fits, its shape kept
        q = (lo + hi) * 0.5 + (uv - 0.5) * max(size.x, size.y);
    }
    // a feather the same width across and down, and half a pixel at least
    let half = 0.5 / g.resolution;
    let e = feather * vec2f(1.0 / g.aspect, 1.0) + half;
    let inside = smoothstep(lo - half, lo + e, q) * (1.0 - smoothstep(hi - e, hi + half, q));
    return textureSampleLevel(src, smp, q, 0.0) * inside.x * inside.y;
}
