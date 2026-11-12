/*{
  "name": "Gradient Blinds",
  "category": "generator",
  "description": "Satin blinds shading a gradient of two colors, swaying, under a spotlight that wanders over them; the bass turns the slats, the kick flares the light",
  "params": [
    { "id": "count", "type": "float", "default": 16.0, "min": 4.0, "max": 60.0 },
    { "id": "angle", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "tilt", "type": "float", "default": 0.7, "min": 0.0, "max": 1.0 },
    { "id": "spot", "type": "float", "default": 0.7, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [1.0, 0.62, 0.99, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.32, 0.15, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Gradient Blinds (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let a = angle * 6.2831853;
    let p = vec2f(s.x * cos(a) - s.y * sin(a), s.x * sin(a) + s.y * cos(a));
    let t = g.time * speed;
    let x = p.x * count;
    let id = floor(x);
    let f = fract(x);
    // a spotlight wandering over them
    let d = s - vec2f(0.4 * g.aspect * sin(t * 0.7), 0.3 * cos(t * 0.9));
    let lit = exp(-dot(d, d) * 3.0) * spot * (1.0 + 0.8 * audio * g.kick);
    // each slat turned a little, swaying on its own: dark at one edge, bright at the other
    let turn = clamp(tilt * (0.6 + 0.4 * sin(t * 1.3 + id * 0.35)) * (1.0 + 0.5 * audio * g.bass), 0.0, 1.0);
    let satin = pow(mix(1.0, f, turn), 1.5);
    // the gradient runs across them all, breathing a little
    let k = clamp(p.x / g.aspect + 0.5 + 0.2 * sin(t * 0.4 + p.y * 1.5), 0.0, 1.0);
    var col = mix(color_a.rgb, color_b.rgb, k) * (0.1 + 0.9 * satin) * (0.7 + 1.3 * lit);
    // a thin shine on each slat's bright edge, a pixel of shadow where the next begins
    let aa = count / g.resolution.y;
    col += mix(color_a.rgb, vec3f(1.0), 0.5) * smoothstep(0.93, 1.0, f) * 0.3 * (0.3 + lit);
    col *= smoothstep(0.0, aa * 1.5, f) * (1.0 - smoothstep(1.0 - aa, 1.0, f) * 0.5);
    col += (hash(px) - 0.5) * 0.03;
    return vec4f(col, 1.0);
}
