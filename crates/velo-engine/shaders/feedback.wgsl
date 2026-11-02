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
    // the kick throws the trail outward
    let a = rotate * 6.2831853;
    let p = mat2x2f(cos(a), sin(a), -sin(a), cos(a)) * ((uv - 0.5) * vec2f(g.aspect, 1.0)) / (zoom * (1.0 + 0.02 * music * g.kick)) + drift;
    var back = textureSample(prev, smp, p / vec2f(g.aspect, 1.0) + 0.5) * decay;
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
