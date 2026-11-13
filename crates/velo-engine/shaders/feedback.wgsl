/*{
  "name": "Feedback",
  "category": "effect",
  "description": "Its own last frame, zoomed, turned and fading, under the input: trails and tunnels",
  "inputs": [{ "id": "src", "type": "texture" }],
  "feedback": true,
  "params": [
    { "id": "decay", "type": "float", "default": 0.92, "min": 0.0, "max": 0.995 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.9, "max": 1.1 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -0.05, "max": 0.05 },
    { "id": "drift", "type": "vec2", "default": [0.0, 0.0], "min": -0.02, "max": 0.02 },
    { "id": "blend", "type": "int", "default": 0, "names": ["Lighten", "Add", "Over"] },
    { "id": "hue", "type": "float", "default": 0.0, "min": -0.05, "max": 0.05 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -0.5, "max": 0.5 },
    { "id": "zoom_curve", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
    { "id": "warp", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "warp_speed", "type": "float", "default": 1.0, "min": 0.0, "max": 4.0 },
    { "id": "warp_scale", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
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
    // the kick throws the trail outward; zoomed and turned about `center`
    let a = rotate * 6.2831853;
    let d = (uv - 0.5 - center) * vec2f(g.aspect, 1.0);
    var z = zoom * (1.0 + 0.02 * music * g.kick);
    // zoom curve (MilkDrop's zoomexp): over 1 the edges zoom more than the middle, under 1 less
    if zoom_curve != 1.0 {
        let rad = length(d) / (0.5 * length(vec2f(g.aspect, 1.0)));
        z = pow(z, pow(zoom_curve, rad * 2.0 - 1.0));
    }
    let p = mat2x2f(cos(a), sin(a), -sin(a), cos(a)) * d / z + drift;
    var q = p / vec2f(g.aspect, 1.0) + 0.5 + center;
    // warp: four slow sine waves pushing the trail about, as MilkDrop's warp does
    if warp > 0.0 {
        let t = g.time * warp_speed;
        let f = vec4f(11.68 + 4.0 * cos(t * 1.413 + 10.0), 8.77 + 3.0 * cos(t * 1.113 + 7.0), 10.54 + 3.0 * cos(t * 1.233 + 3.0), 11.49 + 4.0 * cos(t * 0.933 + 5.0)) / warp_scale;
        let x = (uv.x * 2.0 - 1.0) * g.aspect;
        let y = uv.y * 2.0 - 1.0;
        let w = vec2f(sin(t * 0.333 + x * f.x - y * f.w) + cos(t * 0.753 - x * f.y + y * f.z), cos(t * 0.375 - x * f.z - y * f.y) + sin(t * 0.825 + x * f.x + y * f.w));
        q += warp * 0.0035 * w;
    }
    var back = textureSample(prev, smp, q) * decay;
    // the trail's colors drift a little every frame
    if hue != 0.0 {
        back = vec4f(max(hue_rotate(back.rgb, hue), vec3f(0.0)), back.a);
    }
    let now = textureSample(src, smp, uv);
    if blend == 1 {
        return min(now + back, vec4f(4.0));
    } else if blend == 2 {
        return mix(back, now, now.a);
    }
    return max(now, back);
}
