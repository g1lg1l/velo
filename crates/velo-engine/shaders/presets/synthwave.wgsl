/*{
  "name": "Synthwave",
  "category": "generator",
  "description": "A retro sunset: a striped sun over a glowing grid rolling toward you",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "grid", "type": "color", "default": [1.0, 0.2, 0.85, 1.0] },
    { "id": "sun_top", "type": "color", "default": [1.0, 0.85, 0.25, 1.0] },
    { "id": "sun_bottom", "type": "color", "default": [1.0, 0.15, 0.5, 1.0] },
    { "id": "sky", "type": "color", "default": [0.1, 0.02, 0.25, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // y up, the horizon a little below the middle
    let s = vec2f((uv.x - 0.5) * g.aspect, 0.5 - uv.y);
    let horizon = -0.08;
    let aa = 1.0 / g.resolution.y;
    var col = vec3f(0.0);
    if s.y > horizon {
        let h = s.y - horizon;
        col = mix(mix(sky.rgb, sun_bottom.rgb, 0.5), sky.rgb, smoothstep(0.0, 0.45, h));
        let c = vec2f(0.0, horizon + 0.2);
        let radius = 0.2 * (1.0 + 0.04 * audio * g.bass);
        let d = length(s - c);
        // bands cut across its lower half, thicker toward the bottom, sliding down
        let below = c.y + 0.02 - s.y;
        let k = fract(below * 16.0 + g.time * speed * 0.3);
        let gap = clamp(below * 3.0, 0.0, 0.7);
        let e = 16.0 * aa * 1.5;
        let open = select(1.0, smoothstep(gap, gap + e, k) * (1.0 - smoothstep(1.0 - e, 1.0, k)), below > 0.0);
        let disc = (1.0 - smoothstep(radius - aa, radius + aa, d)) * open;
        col = mix(col, mix(sun_bottom.rgb, sun_top.rgb, smoothstep(c.y - radius, c.y + radius, s.y)), disc);
        col += sun_bottom.rgb * exp(-max(d - radius, 0.0) * 10.0) * 0.3 * (1.0 + 0.5 * audio * g.bass);
    } else {
        // the floor: depth grows toward the horizon, lines every unit
        let dy = horizon - s.y;
        let z = 1.0 / dy;
        let gx = s.x * z * 3.0;
        let gz = z * 3.0 + g.time * speed * 4.0;
        // how much of the floor one pixel covers, to keep lines a pixel or two wide
        let fx = 3.0 * z * sqrt(1.0 + s.x * s.x * z * z) * aa;
        let fz = 3.0 * z * z * aa;
        let lx = abs(fract(gx + 0.5) - 0.5) / fx;
        let lz = abs(fract(gz + 0.5) - 0.5) / fz;
        let l = min(lx, lz);
        let line = (1.0 - smoothstep(0.5, 1.5, l)) + exp(-l / 4.0) * 0.4;
        // lines crowding toward the horizon turn into what they cover on average, a glow
        let dense = smoothstep(0.12, 0.35, max(fx, fz));
        let shimmer = 1.0 + 0.3 * audio * g.hat * sin(gz * 3.1 + g.time * 20.0) * (1.0 - dense);
        let ground = sky.rgb * 0.15 + grid.rgb * mix(line, min(5.0 * (fx + fz), 0.6), dense) * shimmer * (1.0 + 0.6 * audio * g.kick);
        // far off it goes into the haze the sky has at the horizon
        col = mix(mix(sky.rgb, sun_bottom.rgb, 0.5), ground, exp(-z * 0.12));
    }
    return vec4f(col, 1.0);
}
