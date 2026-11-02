/*{
  "name": "Stripes",
  "category": "generator",
  "description": "Stripes sliding by at an angle, wobbling, pushed along on the kick",
  "params": [
    { "id": "count", "type": "float", "default": 12.0, "min": 2.0, "max": 60.0 },
    { "id": "angle", "type": "float", "default": 0.125, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.2, "min": -2.0, "max": 2.0 },
    { "id": "wobble", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "softness", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.05, 0.05, 0.1, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.95, 0.9, 0.3, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let a = angle * 6.2831853;
    let along = dot(s, vec2f(cos(a), sin(a)));
    let across = dot(s, vec2f(-sin(a), cos(a)));
    let bend = wobble * 0.04 * (1.0 + audio * g.bass) * sin(across * 6.0 + g.time * 1.3);
    let v = (along + bend) * count + g.time * speed * 2.0 + 0.3 * audio * g.kick;
    // 0 in the middle of a stripe, 1 between two
    let tri = abs(fract(v) - 0.5) * 2.0;
    let e = max(softness * 0.5, count / g.resolution.y * 1.5);
    let band = smoothstep(0.5 - e, 0.5 + e, tri);
    let shimmer = 1.0 + 0.25 * audio * g.hat * sin(floor(v) * 7.0 + g.time * 25.0);
    return vec4f(mix(color_a.rgb, color_b.rgb * shimmer, band), 1.0);
}
