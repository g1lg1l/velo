/*{
  "name": "Fluid Sphere",
  "category": "generator",
  "description": "A sphere of liquid rippling as if it floated weightless, its skin iridescent like oil on water",
  "params": [
    { "id": "size", "type": "float", "default": 0.6, "min": 0.2, "max": 1.0 },
    { "id": "wobble", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "ripples", "type": "float", "default": 4.0, "min": 1.0, "max": 12.0 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "spin", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "film", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.3, 0.5, 0.9, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.01, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn sdf(p: vec3f) -> f32 {
    let t = g.time * speed;
    let f = ripples;
    let w = sin(p.x * f + t) * sin(p.y * f * 1.3 - t * 0.8) * sin(p.z * f * 0.9 + t * 0.6);
    return length(p) - size - (wobble + 0.4 * audio * g.bass) * 0.12 * w;
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
    let facing = max(dot(-r[1], n), 0.0);
    // a thin film: its color turns round the hue as the angle changes
    let hue = 0.5 + 0.5 * cos(6.2831853 * ((1.0 - facing) * 1.5 + vec3f(0.0, 0.33, 0.67)));
    let body = tint.rgb * (0.25 + 0.75 * facing);
    let rim = pow(1.0 - facing, 3.0);
    let spot = vec3f(pow(max(dot(reflect(r[1], n), normalize(vec3f(0.4, 0.8, -0.5))), 0.0), 60.0));
    return vec4f((mix(body, hue, film * (0.3 + 0.7 * rim)) + spot) * (1.0 + 0.6 * audio * g.kick), 1.0);
}
