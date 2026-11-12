/*{
  "name": "Web Threads",
  "category": "generator",
  "description": "Bundles of glowing threads crossing the dark, each gathered into bright knots along its way and fanning out between them; the bass opens the fans, the kick sends light along the threads",
  "params": [
    { "id": "bundles", "type": "int", "default": 3, "min": 1, "max": 5 },
    { "id": "threads", "type": "int", "default": 10, "min": 2, "max": 24 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 3.0 },
    { "id": "spread", "type": "float", "default": 0.25, "min": 0.0, "max": 0.6 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.3, "max": 6.0 },
    { "id": "glow", "type": "float", "default": 0.6, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.4, 0.6, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.5, 0.8, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Web Threads (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    let aa = 1.0 / g.resolution.y;
    let w = width * 0.0008;
    var col = vec3f(0.0);
    for (var b = 0; b < bundles; b++) {
        let fb = f32(b);
        // each bundle crosses at its own angle, swaying
        let a = fb * 1.0471976 + 0.2 + 0.3 * sin(t * 0.5 + fb * 2.0);
        let p = vec2f(s.x * cos(a) - s.y * sin(a), s.x * sin(a) + s.y * cos(a) + 0.12 * sin(t * 0.7 + fb * 1.3));
        // knots every `gap` along it, sliding slowly; between two the threads fan out
        let gap = 0.55 + 0.1 * fb;
        let along = p.x / gap + t * 0.3 * (0.5 + 0.3 * fb);
        let open = spread * pow(abs(sin(along * 3.1415927)), 1.3) * (1.0 + 0.6 * audio * g.bass);
        for (var k = 0; k < threads; k++) {
            let fk = (f32(k) + 0.5) / f32(threads) - 0.5;
            // its place in the fan, and a wobble of its own
            let d = abs(p.y - open * (fk * 2.0 + 0.3 * sin(along * 2.0 + f32(k) * 1.7 + t * 1.3)));
            let core = 1.0 - smoothstep(w, w + aa, d);
            let halo = exp(-d / (w * 8.0 + 0.002)) * glow * 0.25;
            // the kick's light runs along each thread, not all at the same place
            let run = fract(along * 0.5 + f32(k) * 0.13) - (1.0 - g.kick);
            let pulse = audio * g.kick * exp(-run * run * 36.0);
            col += mix(color_a.rgb, color_b.rgb, fk + 0.5) * (core + halo) * (0.6 + 2.0 * pulse);
        }
        // the knot itself glows
        let knot = length(vec2f((fract(along + 0.5) - 0.5) * gap, p.y));
        col += mix(color_a.rgb, color_b.rgb, 0.5) * exp(-knot * 30.0) * (0.8 + 1.5 * audio * g.kick) + vec3f(1.0) * exp(-knot * 90.0) * 0.8;
    }
    return vec4f(col, 1.0);
}
