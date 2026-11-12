/*{
  "name": "Micro Slats",
  "category": "generator",
  "description": "A floor of tiny slats stretching to the horizon, a slow sea rolling through them: slats on a crest catch the light and glint; the bass raises the swell, the kick sends a wave in",
  "params": [
    { "id": "density", "type": "float", "default": 14.0, "min": 4.0, "max": 40.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "swell", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "gap", "type": "float", "default": 0.25, "min": 0.05, "max": 0.7 },
    { "id": "horizon", "type": "float", "default": 0.3, "min": -0.3, "max": 0.5 },
    { "id": "tint", "type": "color", "default": [0.5, 0.3, 1.0, 1.0] },
    { "id": "glint", "type": "color", "default": [1.0, 0.85, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.0, 0.04, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Micro Slats (its code isn't used: MIT + Commons Clause).

// the sea's height at a place on the floor, -1..1
fn sea(w: vec2f, t: f32) -> f32 {
    return 0.5 * sin(w.y * 0.7 + t + 0.8 * sin(w.x * 0.3 + t * 0.4)) + 0.3 * sin(w.y * 1.3 - w.x * 0.5 + t * 1.3) + 0.2 * sin(w.x * 0.9 + t * 0.7);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // y up
    let s = vec2f((uv.x - 0.5) * g.aspect, 0.5 - uv.y);
    let down = horizon - s.y;
    if down <= 0.0 {
        return vec4f(background.rgb + tint.rgb * 0.25 * exp(down * 9.0), 1.0);
    }
    // the floor: z grows toward the horizon; slats twice as deep as wide
    let z = 0.35 / down;
    let q = vec2f(s.x * z, z * 0.5) * density;
    let id = floor(q);
    let f = abs(fract(q) - 0.5);
    let t = g.time * speed * 2.0;
    // each slat stands as high as the sea is at its middle; the kick's wave comes in from afar
    let at = (id + 0.5) / density * 5.0;
    let wave = audio * g.kick * exp(-abs(at.y * 0.25 - g.kick * 3.0) * 2.0);
    let h = sea(at, t) * swell * (1.0 + 0.5 * audio * g.bass) + 2.0 * wave;
    // how much of a slat one pixel covers, across and in depth: far ones melt into a haze
    let foot = vec2f(density * z, density * 0.5 * z * z / 0.35) / g.resolution.y;
    let half = 0.5 - gap * 0.5;
    let slat = (1.0 - smoothstep(half - foot.x, half + foot.x, f.x)) * (1.0 - smoothstep(half - foot.y, half + foot.y, f.y));
    let far = smoothstep(0.3, 1.0, max(foot.x, foot.y));
    let cover = mix(slat, (1.0 - gap) * (1.0 - gap), far);
    // lit by its height, and a glint on the crests
    let lit = 0.08 + 0.7 * pow(clamp(h * 0.5 + 0.5, 0.0, 1.0), 2.0);
    let crest = pow(clamp(h * 1.3, 0.0, 1.1), 3.0);
    var col = tint.rgb * lit * 1.3 + glint.rgb * crest * 0.6;
    col = background.rgb + col * cover * exp(-z * 0.22);
    return vec4f(col + tint.rgb * 0.25 * exp(-down * 9.0), 1.0);
}
