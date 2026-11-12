/*{
  "name": "Topography",
  "category": "generator",
  "description": "A landscape drawn in glowing contour lines, seen from above at an angle, slowly drifting; the bass raises the hills, the kick sends a pulse of light up through the heights",
  "params": [
    { "id": "levels", "type": "float", "default": 14.0, "min": 3.0, "max": 40.0 },
    { "id": "scale", "type": "float", "default": 1.0, "min": 0.3, "max": 4.0 },
    { "id": "speed", "type": "float", "default": 0.2, "min": 0.0, "max": 2.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.5, "max": 4.0 },
    { "id": "tilt", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.15, 0.4, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.9, 1.0, 0.6, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.01, 0.04, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Topography (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x), mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}

fn fbm(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // the ground leaning away at the top: further there, so smaller
    let z = 1.0 / max(1.0 - tilt * 0.9 * (0.5 - uv.y) * 1.6, 0.15);
    let w = vec2f(s.x * z, (uv.y - 0.5) * z * (1.0 + tilt)) * scale * 1.6;
    let t = g.time * speed;
    let land = fbm(w + vec2f(t * 0.3, -t * 0.5)) * (1.0 + 0.25 * audio * g.bass);
    // a line at every level, a pixel or two wide whatever the slope
    let v = land * levels;
    let per_pixel = max(fwidth(v), 0.0001);
    let d = abs(fract(v + 0.5) - 0.5) / per_pixel;
    let line = (1.0 - smoothstep(0.5 * width, 0.5 * width + 1.0, d)) * (1.0 - smoothstep(0.3, 0.8, per_pixel));
    let halo = exp(-d / (3.0 * width)) * 0.25 * (1.0 - smoothstep(0.2, 0.6, per_pixel));
    // every fifth line stronger, as on a map
    let level = floor(v + 0.5);
    let index = 0.6 + 0.6 * step(4.5, level - 5.0 * floor(level / 5.0));
    // the kick's pulse climbs from the valleys to the peaks as the kick fades
    let pulse = audio * g.kick * exp(-abs(land - (1.0 - g.kick) * 0.9) * 14.0) * 2.5;
    let tint = mix(color_a.rgb, color_b.rgb, smoothstep(0.25, 0.75, land));
    var col = background.rgb + tint * (line * index + halo) * (1.0 + pulse) + tint * 0.07 * land;
    // far off it sinks into the dark
    return vec4f(col * mix(1.0, exp(-(z - 1.0) * 0.5), tilt), 1.0);
}
