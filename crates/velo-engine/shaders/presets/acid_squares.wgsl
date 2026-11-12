/*{
  "name": "Acid Squares",
  "category": "generator",
  "description": "A field of glossy squares rocking on slow waves under two drifting colored lights, each catching them in turn; the bass rocks them harder, the kick flares the lights",
  "params": [
    { "id": "density", "type": "float", "default": 12.0, "min": 4.0, "max": 40.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "tilt", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "gap", "type": "float", "default": 0.12, "min": 0.0, "max": 0.6 },
    { "id": "gloss", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "color_a", "type": "color", "default": [0.75, 1.0, 0.1, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.15, 0.7, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Acid Squares (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let q = s * density;
    let f = fract(q) - 0.5;
    // the square's middle, in picture heights
    let c = (floor(q) + 0.5) / density;
    let t = g.time * speed * 2.0;
    // which way the waves tip this square: the slope of a few sines, worked out by hand
    let k = tilt * 0.12 * (1.0 + 0.8 * audio * g.bass);
    let slope = vec2f(
        4.0 * cos(c.x * 4.0 + t) + 3.0 * cos((c.x + c.y) * 3.0 + t * 0.7),
        -5.0 * sin(c.y * 5.0 - t * 1.3) + 3.0 * cos((c.x + c.y) * 3.0 + t * 0.7));
    // flat across the square, its edges turned down a little
    let d = max(abs(f.x), abs(f.y));
    let bevel = smoothstep(0.3, 0.5, d) * 1.5;
    let n = normalize(vec3f(-slope * k + f * bevel, 1.0));
    // two lights going round above the field
    let flare = 1.0 + audio * g.kick;
    // (never quite dark: the squares far from both lights still show)
    var col = background.rgb + mix(color_a.rgb, color_b.rgb, 0.5) * 0.05;
    for (var i = 0; i < 2; i++) {
        let fi = f32(i);
        let at = vec2f(0.4 * g.aspect * sin(t * 0.31 + fi * 3.1), 0.35 * cos(t * 0.43 + fi * 1.7));
        let dir = normalize(vec3f(at - c, 0.35));
        let diffuse = max(dot(n, dir), 0.0);
        let shine = pow(max(dot(n, normalize(dir + vec3f(0.0, 0.0, 1.0))), 0.0), 50.0);
        // (fainter far from the light)
        let fall = 1.0 / (1.0 + 2.0 * dot(at - c, at - c));
        col += select(color_b.rgb, color_a.rgb, i == 0) * (0.5 * diffuse * diffuse + 2.5 * shine * gloss) * fall * flare;
    }
    let aa = density / g.resolution.y;
    let half = 0.5 - gap * 0.5;
    return vec4f(mix(background.rgb, col, 1.0 - smoothstep(half - aa, half + aa, d)), 1.0);
}
