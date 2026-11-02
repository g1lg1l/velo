/*{
  "name": "Image",
  "category": "generator",
  "description": "A picture from a file: a still, an animated GIF, a folder of frames or a HAP movie",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "fit", "type": "int", "default": 0, "names": ["Fill", "Fit", "Stretch"] },
    { "id": "speed", "type": "float", "default": 1.0, "min": 0.0, "max": 4.0 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.1, "max": 10.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "crop_left", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "crop_right", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "crop_top", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "crop_bottom", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "flip_x", "type": "bool", "default": false },
    { "id": "flip_y", "type": "bool", "default": false },
    { "id": "loop_start", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "loop_end", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// a Hap Q movie's texels hold scaled YCoCg: the engine builds this shader with it true
const YCOCG: bool = false;

// scaled YCoCg (Co, Cg, scale, Y) to RGB, as HAP's reference shader does it
fn ycocg(c: vec4f) -> vec4f {
    let s = c.z * (255.0 / 8.0) + 1.0;
    let co = (c.x - 0.50196078) / s;
    let cg = (c.y - 0.50196078) / s;
    // the compression's error can step just past 0 or 1: kept in range (feedback adds up)
    return vec4f(clamp(vec3f(c.w + co - cg, c.w + cg, c.w - co - cg), vec3f(0.0), vec3f(1.0)), 1.0);
}

// src is the file's frame (the engine binds it, it's not a port); speed is read by the engine:
// how fast a GIF, a folder of frames or a movie plays (1 as made, 0 holds the frame)
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the part of the picture kept, its edges cropped off
    let lo = vec2f(crop_left, crop_top);
    let hi = max(vec2f(1.0 - crop_right, 1.0 - crop_bottom), lo + 0.001);
    let size = vec2f(textureDimensions(src)) * (hi - lo);
    // the picture's aspect over the canvas': fill (cover), fit (contain) or stretch
    let k = (size.x / max(size.y, 1.0)) / g.aspect;
    var scale = vec2f(1.0);
    if fit == 0 {
        scale = select(vec2f(1.0, k), vec2f(1.0 / k, 1.0), k > 1.0);
    } else if fit == 1 {
        scale = select(vec2f(1.0 / k, 1.0), vec2f(1.0, k), k > 1.0);
    }
    // moved, turned (a half turn at 1) and zoomed about the middle, in the canvas' proportions so
    // a turn doesn't skew it
    let a = rotate * 3.14159265;
    let p = (uv - 0.5 - position * 0.5) * vec2f(g.aspect, 1.0);
    var q = (mat2x2f(cos(a), sin(a), -sin(a), cos(a)) * p) / max(zoom, 0.01) / vec2f(g.aspect, 1.0) * scale + 0.5;
    if flip_x {
        q.x = 1.0 - q.x;
    }
    if flip_y {
        q.y = 1.0 - q.y;
    }
    if any(q < vec2f(0.0)) || any(q > vec2f(1.0)) {
        return vec4f(0.0);
    }
    let c = textureSample(src, smp, mix(lo, hi, q));
    if YCOCG {
        return ycocg(c);
    }
    return c;
}
