/*{
  "name": "Ferrofluid",
  "category": "generator",
  "description": "A glossy black drop of ferrofluid rising into spikes, as if a magnet came near: map a signal onto spikes",
  "params": [
    { "id": "size", "type": "float", "default": 0.55, "min": 0.2, "max": 1.0 },
    { "id": "spikes", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "count", "type": "float", "default": 9.0, "min": 3.0, "max": 24.0 },
    { "id": "sharpness", "type": "float", "default": 6.0, "min": 1.0, "max": 16.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "spin", "type": "float", "default": 0.05, "min": -1.0, "max": 1.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "color", "type": "color", "default": [0.02, 0.02, 0.03, 1.0] },
    { "id": "shine", "type": "color", "default": [0.7, 0.8, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// the sphere pushed out where a moving pattern of its directions peaks: a spike at each peak
fn sdf(p: vec3f) -> f32 {
    let n = normalize(p + vec3f(1e-5));
    let t = g.time * speed;
    let s = sin(n.x * count + t) * sin(n.y * count - t * 0.7) * sin(n.z * count + t * 0.5);
    return length(p) - size * (1.0 + min(spikes + 0.5 * audio * g.bass, 1.5) * 0.6 * pow(max(s, 0.0), sharpness));
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
    var t = 0.0;
    for (var i = 0; i < 110; i++) {
        let d = sdf(ro + rd * t);
        if d < 0.001 {
            return t;
        }
        t += d * 0.6;
        if t > 6.0 {
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
    // black glass: what it reflects, a soft sky above and a hard highlight
    let fresnel = pow(1.0 - max(dot(-r[1], n), 0.0), 3.0);
    let sky = shine.rgb * smoothstep(-0.1, 0.9, m.y) * (0.15 + 0.85 * fresnel);
    let spot = shine.rgb * pow(max(dot(m, normalize(vec3f(0.5, 0.8, -0.4))), 0.0), 80.0) * 2.0;
    return vec4f(color.rgb + (sky + spot) * (1.0 + 0.8 * audio * g.kick), 1.0);
}
