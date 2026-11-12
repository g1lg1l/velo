/*{
  "name": "Fractal World",
  "category": "generator",
  "description": "A camera flying without end through a fractal world of folded shells and beads, its colors drawn from the folds; the bass reshapes it and the kick lights it",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "shape", "type": "float", "default": 1.2, "min": 1.0, "max": 1.5 },
    { "id": "detail", "type": "int", "default": 8, "min": 3, "max": 11 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.4, "max": 2.5 },
    { "id": "roll", "type": "float", "default": 0.2, "min": 0.0, "max": 1.0 },
    { "id": "fog", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.25, 0.75, 0.5, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.45, 0.2, 0.6, 1.0] },
    { "id": "color_c", "type": "color", "default": [1.0, 0.75, 0.15, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// the world: space folded into itself over and over (an Apollonian packing). The distance to
// it, and how close each fold came to the axes and the middle: what it's colored by
fn world(start: vec3f, s: f32) -> vec4f {
    var p = start;
    var scale = 1.0;
    var trap = vec3f(1000.0);
    for (var i = 0; i < detail; i++) {
        p = -1.0 + 2.0 * fract(0.5 * p + 0.5);
        let r2 = dot(p, p);
        trap = min(trap, vec3f(abs(p.x), abs(p.y), r2));
        let k = s / r2;
        p *= k;
        scale *= k;
    }
    return vec4f(0.25 * abs(p.y) / scale, trap);
}

fn world_normal(p: vec3f, s: f32, e: f32) -> vec3f {
    let h = vec2f(e, 0.0);
    return normalize(vec3f(world(p + h.xyy, s).x - world(p - h.xyy, s).x, world(p + h.yxy, s).x - world(p - h.yxy, s).x, world(p + h.yyx, s).x - world(p - h.yyx, s).x));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s2 = (uv - 0.5) * vec2f(g.aspect, -1.0);
    let t = g.time * speed * 0.4;
    // the bass breathes the folds wider
    let s = shape + 0.08 * sin(t * 0.7) + 0.06 * audio * g.bass;
    // the camera: wandering round the middle on three unrelated turns, looking at a point that wanders too
    let ro = vec3f(2.8 * cos(0.1 + 0.33 * t), 0.4 + 0.3 * cos(0.37 * t), 2.8 * cos(0.5 + 0.35 * t));
    let ta = vec3f(1.9 * cos(1.2 + 0.41 * t), 0.4 + 0.1 * cos(0.27 * t), 1.9 * cos(2.0 + 0.38 * t));
    let lean = roll * cos(0.1 * t);
    let cw = normalize(ta - ro);
    let cu = normalize(cross(cw, vec3f(sin(lean), cos(lean), 0.0)));
    let cv = cross(cu, cw);
    let rd = normalize(s2.x * cu + s2.y * cv + 2.0 * zoom * cw);
    // marched, the step and the hit finer near the eye than far off
    var d = 0.01;
    var hit = false;
    var trap = vec3f(0.0);
    for (var i = 0; i < 110; i++) {
        let w = world(ro + rd * d, s);
        trap = w.yzw;
        if (w.x < 0.0008 * d) {
            hit = true;
            break;
        }
        d += w.x;
        if (d > 30.0) {
            break;
        }
    }
    let haze = mix(color_a.rgb * 0.35, color_b.rgb * 0.25, 0.5 + 0.5 * rd.y);
    var col = haze;
    if (hit) {
        let p = ro + rd * d;
        let n = world_normal(p, s, 0.0006 * d);
        // the folds' colors: the first where a fold lay flat, the second in its creases, the
        // third in the beads at its middle
        var paint = mix(color_a.rgb, color_b.rgb, clamp(6.0 * trap.y, 0.0, 1.0));
        paint = mix(paint, color_c.rgb, pow(clamp(1.0 - 2.0 * trap.z, 0.0, 1.0), 8.0));
        let key = normalize(vec3f(0.6, 0.7, 0.4));
        let lit = clamp(dot(n, key), 0.0, 1.0);
        let sky = 0.5 + 0.5 * n.y;
        let rim = pow(1.0 - clamp(dot(n, -rd), 0.0, 1.0), 3.0);
        // darker deep in the creases
        let deep = clamp(pow(trap.z * 2.0, 1.2), 0.0, 1.0);
        col = paint * (0.25 * sky + 1.1 * lit) * (0.35 + 0.65 * deep) + color_c.rgb * rim * (0.15 + 0.6 * audio * g.kick);
        col = mix(col, haze, 1.0 - exp(-fog * 0.08 * d * d));
    }
    col *= 1.0 + 0.3 * audio * g.kick;
    return vec4f(pow(col, vec3f(0.8)), 1.0);
}
