/*{
  "name": "Fractal",
  "category": "generator",
  "description": "A Julia set whose shape keeps changing as its seed goes round, colored in bands that flow outward",
  "params": [
    { "id": "zoom", "type": "float", "default": 1.25, "min": 0.5, "max": 8.0 },
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
    // turning slowly, so it never sits still even when the seed is slow
    let spin = g.time * speed * 0.3;
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0) * 3.0 / zoom;
    let p = vec2f(s.x * cos(spin) - s.y * sin(spin), s.x * sin(spin) + s.y * cos(spin));
    // the seed goes round just inside the Mandelbrot set's edge (its main cardioid), where the
    // Julia sets are whole and most alive; the bass nudges it along
    let a = 2.2 + g.time * speed + 0.12 * audio * g.bass;
    let c = 0.98 * (0.5 * vec2f(cos(a), sin(a)) - 0.25 * vec2f(cos(2.0 * a), sin(2.0 * a)));
    var z = p;
    var n = 0.0;
    // the nearest the orbit comes to the middle: it colors the inside
    var trap = 1e9;
    for (var i = 0; i < detail; i++) {
        z = vec2f(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y) + c;
        let d = dot(z, z);
        if d > 256.0 {
            break;
        }
        trap = min(trap, d);
        n += 1.0;
    }
    let lit = 1.0 + 0.6 * audio * g.kick;
    if n >= f32(detail) {
        // inside: folds of the two darker colors, breathing with the flow
        let k = 0.5 + 0.5 * cos(sqrt(trap) * 9.0 - g.time * flow * 3.0);
        return vec4f(mix(color_a.rgb * 0.25, color_b.rgb * 0.5, k * k) * lit, 1.0);
    }
    // a smooth count, so the bands have no steps
    let sm = n - log2(log2(dot(z, z))) + 4.0;
    let t = sqrt(sm) * 0.35 - g.time * flow;
    // three colors taking turns along it
    let w = pow(0.5 + 0.5 * cos(t * 6.2831853 + vec3f(0.0, 2.0943951, 4.1887902)), vec3f(2.0));
    var col = (color_a.rgb * w.x + color_b.rgb * w.y + color_c.rgb * w.z) / (w.x + w.y + w.z + 0.0001);
    // far out it fades into the dark, near the set it glows, hottest right at its edge
    col = col * smoothstep(0.0, 7.0, sm) * 1.15 + color_c.rgb * smoothstep(0.35, 1.0, sm / f32(detail)) * 0.7;
    return vec4f(col * lit, 1.0);
}
