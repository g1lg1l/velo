/*{
  "name": "Lasers",
  "category": "generator",
  "description": "Club laser beams fanning out through haze and sweeping, from the bottom, the corners or the top; spread wider with the bass, flashed by the kick",
  "params": [
    { "id": "beams", "type": "int", "default": 8, "min": 1, "max": 24 },
    { "id": "spread", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "sweep", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.2, "max": 4.0 },
    { "id": "haze", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "origin", "type": "int", "default": 0, "names": ["Bottom", "Corners", "Top"] },
    { "id": "tint", "type": "color", "default": [0.2, 1.0, 0.35, 1.0] },
    { "id": "hue_step", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// a color turned round the grey axis
fn hue_turn(c: vec3f, turns: f32) -> vec3f {
    let k = vec3f(0.57735027);
    let a = turns * 6.2831853;
    return c * cos(a) + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - cos(a));
}

// the light of a beam from `o` along angle `a` at p: a thin core and a wide glow, fading out far
fn beam(p: vec2f, o: vec2f, a: f32) -> vec2f {
    let dir = vec2f(cos(a), sin(a));
    let v = p - o;
    let along = dot(v, dir);
    let across = abs(v.x * dir.y - v.y * dir.x);
    let ahead = smoothstep(0.0, 0.05, along) * exp(-along * 0.6);
    let w = width * 0.0015 * (1.0 + max(along, 0.0) * 2.0);
    return vec2f(exp(-across / w), exp(-across / (w * 25.0))) * ahead;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // y down, as uv
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    let fan = spread * (1.0 + 0.5 * audio * g.bass) * 2.4;
    let n = max(beams, 1);
    var light = vec3f(0.0);
    for (var i = 0; i < n; i++) {
        let k = select(f32(i) / f32(n - 1) - 0.5, 0.0, n == 1);
        // each beam's place in the fan, swaying back and forth on its own phase
        let sway = sweep * 0.6 * sin(t + f32(i) * 0.7);
        var o = vec2f(0.0, 0.55);
        var a = -1.5707963 + k * fan + sway;
        if origin == 1 {
            // from both bottom corners, crossing
            let side = select(-1.0, 1.0, i % 2 == 0);
            o = vec2f(side * 0.5 * g.aspect, 0.55);
            a = -1.5707963 - side * (0.5 + 0.5 * (k + 0.5) * fan) + sway;
        } else if origin == 2 {
            o = vec2f(0.0, -0.55);
            a = 1.5707963 + k * fan + sway;
        }
        let b = beam(p, o, a);
        light += hue_turn(tint.rgb, hue_step * f32(i)) * (b.x * 1.5 + b.y * haze * 0.4);
    }
    // the kick flashes them
    return vec4f(light * (1.0 + 1.2 * audio * g.kick), 1.0);
}
