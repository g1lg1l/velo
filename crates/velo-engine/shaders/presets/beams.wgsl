/*{
  "name": "Beams",
  "category": "generator",
  "description": "Glossy ribbons side by side rippling in the dark, a light sliding along each as it bends; the bass deepens the ripples, the kick flashes the gloss",
  "params": [
    { "id": "count", "type": "float", "default": 9.0, "min": 2.0, "max": 40.0 },
    { "id": "angle", "type": "float", "default": 0.08, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "ripple", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "width", "type": "float", "default": 0.85, "min": 0.2, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.75, 0.8, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Beams (its code isn't used: MIT + Commons Clause).

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

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let a = angle * 6.2831853;
    let p = vec2f(s.x * cos(a) - s.y * sin(a), s.x * sin(a) + s.y * cos(a));
    let x = p.x * count;
    let id = floor(x);
    // across the ribbon, -0.5..0.5
    let f = fract(x) - 0.5;
    // each ribbon ripples along its length on its own noise, at its own pace
    let amp = ripple * (1.0 + 0.8 * audio * g.bass);
    let run = g.time * speed * (0.6 + 0.4 * hash(vec2f(id, 3.0)));
    let e = 0.02;
    let h = noise(vec2f(id * 7.3, p.y * 3.0 - run));
    let slope = (noise(vec2f(id * 7.3, (p.y + e) * 3.0 - run)) - h) / e * amp * 0.2;
    // a little rounded across, tilting with its ripples
    let half = width * 0.5;
    let aa = count / g.resolution.y;
    let inside = 1.0 - smoothstep(half - aa, half + aa, abs(f));
    let n = normalize(vec3f(f / max(half, 0.01) * 0.6, slope, 1.0));
    let light = normalize(vec3f(0.3, -0.6, 0.7));
    let diffuse = max(dot(n, light), 0.0);
    let gloss = pow(max(dot(n, normalize(light + vec3f(0.0, 0.0, 1.0))), 0.0), 14.0);
    var col = tint.rgb * (0.05 + 0.4 * diffuse * diffuse + 1.3 * gloss * (1.0 + audio * g.kick));
    // no two alike, all fading toward their ends
    col *= (0.6 + 0.4 * hash(vec2f(id, 9.0))) * smoothstep(1.1, 0.4, abs(p.y));
    return vec4f(background.rgb + col * inside, 1.0);
}
