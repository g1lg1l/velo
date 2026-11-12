/*{
  "name": "Liquid Chrome",
  "category": "generator",
  "description": "Drops of liquid metal drifting, merging and pulling apart, mirroring a striped sky",
  "params": [
    { "id": "size", "type": "float", "default": 0.35, "min": 0.1, "max": 0.8 },
    { "id": "count", "type": "int", "default": 4, "min": 2, "max": 6 },
    { "id": "spread", "type": "float", "default": 0.6, "min": 0.0, "max": 1.5 },
    { "id": "merge", "type": "float", "default": 0.35, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "spin", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.85, 0.9, 1.0, 1.0] },
    { "id": "sky", "type": "color", "default": [0.25, 0.35, 0.9, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.01, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// the smooth union: within k of each other, two shapes melt together
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / max(k, 1e-4), 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn sdf(p: vec3f) -> f32 {
    let t = g.time * speed;
    var d = 1e9;
    for (var i = 0; i < count; i++) {
        let f = f32(i);
        let c = vec3f(sin(t * (0.7 + 0.13 * f) + f * 2.1), cos(t * (0.5 + 0.17 * f) + f * 1.3), sin(t * (0.3 + 0.11 * f) + f * 3.7) * 0.6) * spread;
        d = smin(d, length(p - c) - size * (1.0 + 0.2 * audio * g.bass) * (0.75 + 0.25 * sin(f * 1.7)), merge * 0.8);
    }
    return d;
}

// a ray from a camera in front of the middle, y up, the scene turned `turn` turns about y
fn ray(uv: vec2f, turn: f32) -> array<vec3f, 2> {
    let q = (uv - 0.5 - position * 0.5) * vec2f(g.aspect, -1.0);
    let a = turn * 6.2831853;
    let r = mat3x3f(cos(a), 0.0, -sin(a), 0.0, 1.0, 0.0, sin(a), 0.0, cos(a));
    return array<vec3f, 2>(r * vec3f(0.0, 0.0, -2.4), r * normalize(vec3f(q, 1.6)));
}

fn normal(p: vec3f) -> vec3f {
    let e = vec2f(0.001, 0.0);
    return normalize(vec3f(sdf(p + e.xyy) - sdf(p - e.xyy), sdf(p + e.yxy) - sdf(p - e.yxy), sdf(p + e.yyx) - sdf(p - e.yyx)));
}

// how far along the ray the surface is, or -1; steps shortened, the fields aren't exact
fn march(ro: vec3f, rd: vec3f) -> f32 {
    // only through the sphere round every blob: a ray that misses it costs nothing
    let reach = 1.55 * spread + size * 1.4 + merge;
    let b = dot(ro, rd);
    let h = b * b - dot(ro, ro) + reach * reach;
    if h < 0.0 {
        return -1.0;
    }
    var t = max(-b - sqrt(h), 0.0);
    let far = -b + sqrt(h);
    for (var i = 0; i < 110; i++) {
        let d = sdf(ro + rd * t);
        if d < 0.001 {
            return t;
        }
        t += d * 0.6;
        if t > far {
            break;
        }
    }
    return -1.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let r = ray(uv, g.time * spin);
    let t = march(r[0], r[1]);
    if t < 0.0 {
        return vec4f(background.rgb, 1.0);
    }
    let n = normal(r[0] + r[1] * t);
    let m = reflect(r[1], n);
    // chrome: a sky of bands overhead, a floor in the sky's opposite color below the horizon,
    // a bright line where they meet, a white sun
    let bands = 0.5 + 0.5 * sin(m.y * 9.0 + m.x * 2.0 + g.time * speed * 0.5);
    let above = sky.rgb * (0.35 + 0.65 * bands);
    let below = sky.bgr * (0.1 + 0.3 * smoothstep(0.0, -0.9, m.y)) * (0.6 + 0.4 * sin(m.x * 11.0));
    var env = mix(below, above, smoothstep(-0.06, 0.06, m.y)) + vec3f(0.8) * exp(-abs(m.y) * 22.0);
    // edges mirror more than what faces us
    let fresnel = pow(1.0 - max(dot(-r[1], n), 0.0), 4.0);
    env += sky.rgb * fresnel * 0.6;
    let flash = 1.0 + audio * g.kick;
    let sun = vec3f(pow(max(dot(m, normalize(vec3f(-0.4, 0.7, -0.5))), 0.0), 90.0)) * 3.0 * flash;
    return vec4f(tint.rgb * env * (1.0 + 0.4 * audio * g.kick) + sun, 1.0);
}
