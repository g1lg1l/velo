/*{
  "name": "Fractal",
  "category": "generator",
  "description": "A Julia set whose shape keeps changing as its seed goes round, colored in bands that flow outward",
  "params": [
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.5, "max": 8.0 },
    { "id": "speed", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "flow", "type": "float", "default": 0.2, "min": 0.0, "max": 2.0 },
    { "id": "detail", "type": "int", "default": 64, "min": 16, "max": 200 },
    { "id": "color_a", "type": "color", "default": [0.1, 0.05, 0.4, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.95, 0.3, 0.6, 1.0] },
    { "id": "color_c", "type": "color", "default": [1.0, 0.85, 0.4, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * 3.0 / zoom;
    // the seed goes round just inside the Mandelbrot set's edge (its main cardioid), where the
    // Julia sets are whole and most alive; the bass nudges it along
    let a = g.time * speed + 0.12 * audio * g.bass;
    let c = 0.98 * (0.5 * vec2f(cos(a), sin(a)) - 0.25 * vec2f(cos(2.0 * a), sin(2.0 * a)));
    var z = p;
    var n = 0.0;
    for (var i = 0; i < detail; i++) {
        z = vec2f(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        if dot(z, z) > 256.0 {
            break;
        }
        n += 1.0;
    }
    if n >= f32(detail) {
        return vec4f(color_a.rgb * 0.15, 1.0);
    }
    // a smooth count, so the bands have no steps
    let s = n - log2(log2(dot(z, z))) + 4.0;
    let t = sqrt(s) * 0.35 - g.time * flow;
    // three colors taking turns along it
    let w = pow(0.5 + 0.5 * cos(t * 6.2831853 + vec3f(0.0, 2.0943951, 4.1887902)), vec3f(2.0));
    let col = (color_a.rgb * w.x + color_b.rgb * w.y + color_c.rgb * w.z) / (w.x + w.y + w.z + 0.0001);
    // far out it fades into the dark, near the set it glows; the kick lights it
    let glow = smoothstep(0.0, 8.0, s) * (1.0 + 0.6 * audio * g.kick);
    return vec4f(col * glow, 1.0);
}
