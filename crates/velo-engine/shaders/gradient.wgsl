/*{
  "name": "Gradient",
  "category": "generator",
  "description": "Two colors along a line, around a center or in a diamond; repeated, mirrored, scrolling",
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.13, 0.83, 0.93, 1.0] },
    { "id": "angle", "type": "float", "default": 0.25, "min": 0.0, "max": 1.0 },
    { "id": "shape", "type": "int", "default": 0, "names": ["Linear", "Radial", "Angular", "Diamond"] },
    { "id": "repeat", "type": "int", "default": 1, "min": 1, "max": 16 },
    { "id": "mirror", "type": "bool", "default": false },
    { "id": "scroll", "type": "float", "default": 0.0, "min": -2.0, "max": 2.0 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5 - center * 0.5) * vec2f(g.aspect, 1.0);
    let a = angle * 6.2831853;
    let along = vec2f(cos(a), sin(a));
    var t = dot(p, along) + 0.5;
    if shape == 1 {
        t = length(p) * 2.0;
    } else if shape == 2 {
        t = fract((atan2(p.y, p.x) - a) / 6.2831853 + 1.0);
    } else if shape == 3 {
        t = (abs(dot(p, along)) + abs(dot(p, vec2f(-along.y, along.x)))) * 2.0;
    }
    // audio: the bass pushes the bands along
    t = t * f32(repeat) + g.time * scroll + 0.25 * audio * g.bass;
    // bands: back and forth when mirrored, else each one starting over
    if mirror {
        t = 1.0 - abs(fract(t * 0.5) * 2.0 - 1.0);
    } else if repeat > 1 || scroll != 0.0 {
        t = fract(t);
    }
    let c = mix(color_a, color_b, clamp(t, 0.0, 1.0));
    // and the kick flashes it
    return vec4f(c.rgb * c.a * (1.0 + 0.5 * audio * g.kick), c.a);
}
