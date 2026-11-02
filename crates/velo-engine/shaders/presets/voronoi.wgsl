/*{
  "name": "Voronoi",
  "category": "generator",
  "description": "Drifting cells with glowing borders, like a living mosaic",
  "params": [
    { "id": "scale", "type": "float", "default": 6.0, "min": 1.0, "max": 30.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "border", "type": "float", "default": 0.04, "min": 0.0, "max": 0.3 },
    { "id": "color_a", "type": "color", "default": [0.05, 0.1, 0.3, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.1, 0.6, 0.8, 1.0] },
    { "id": "edge", "type": "color", "default": [0.8, 1.0, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

// where a cell's point is right now, inside its cell
fn point(c: vec2f, t: f32) -> vec2f {
    let h = vec2f(hash(c), hash(c + vec2f(57.0, 113.0)));
    return 0.5 + 0.4 * sin(t + 6.2831853 * h);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale * (1.0 - 0.08 * audio * g.bass);
    let t = g.time * speed;
    let cell = floor(p);
    let f = fract(p);
    // the nearest point
    var near = vec2f(0.0);
    var id = vec2f(0.0);
    var d1 = 8.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let o = vec2f(f32(i), f32(j));
            let r = o + point(cell + o, t) - f;
            let d = dot(r, r);
            if d < d1 {
                d1 = d;
                near = r;
                id = cell + o;
            }
        }
    }
    // the distance to the border: halfway to each neighbor, along the line between them
    var border_d = 8.0;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let o = vec2f(f32(i), f32(j));
            let r = o + point(cell + o, t) - f;
            let between = r - near;
            if dot(between, between) > 0.00001 {
                border_d = min(border_d, dot(0.5 * (near + r), normalize(between)));
            }
        }
    }
    let h = hash(id);
    var col = mix(color_a.rgb, color_b.rgb, h) * (1.1 - 0.6 * sqrt(d1));
    // a few cells catch the hats
    col += color_b.rgb * audio * g.hat * step(0.85, fract(h * 13.0 + floor(g.time * 4.0) * 0.37)) * 0.6;
    let glow = (1.0 - smoothstep(0.0, border + 0.02, border_d)) + exp(-border_d * 12.0) * 0.25;
    col += edge.rgb * glow * (0.8 + 0.6 * audio * g.kick);
    return vec4f(col, 1.0);
}
