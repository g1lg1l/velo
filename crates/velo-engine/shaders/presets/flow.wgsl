/*{
  "name": "Flow",
  "category": "generator",
  "description": "Flow lines of a current that swirls around drifting eddies, light running along them",
  "params": [
    { "id": "scale", "type": "float", "default": 1.5, "min": 0.5, "max": 8.0 },
    { "id": "speed", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "lines", "type": "float", "default": 30.0, "min": 4.0, "max": 100.0 },
    { "id": "width", "type": "float", "default": 2.0, "min": 0.5, "max": 10.0 },
    { "id": "color_a", "type": "color", "default": [0.1, 0.55, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.35, 0.75, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.01, 0.04, 1.0] },
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
    for (var i = 0; i < 3; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

// the stream function: a current to the right, bent by noise. Its level lines are the flow
// lines of its curl, so drawing them draws the flow.
fn stream(s: vec2f, t: f32) -> f32 {
    return s.y * 0.4 + fbm(s * scale + vec2f(-t, t * 0.6)) * 0.4;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    let e = 0.002;
    let v = stream(s, t);
    let slope = vec2f(stream(s + vec2f(e, 0.0), t) - v, stream(s + vec2f(0.0, e), t) - v) / e;
    // the distance to the nearest line, in picture heights
    let k = v * lines;
    let level = abs(fract(k + 0.5) - 0.5) / max(length(slope) * lines, 0.0001);
    let w = width * 0.0005 * (1.0 + 0.6 * audio * g.kick);
    let aa = 1.0 / g.resolution.y;
    let line = 1.0 - smoothstep(w, w + aa, level);
    let glow = exp(-level / (w * 4.0 + aa)) * 0.3;
    // each line its own color; pulses of light running downstream
    let id = floor(k + 0.5);
    let tint = mix(color_a.rgb, color_b.rgb, hash(vec2f(id, 3.0)));
    let pulse = 0.35 + 0.65 * pow(0.5 + 0.5 * sin(s.x * 7.0 - g.time * (1.0 + speed * 6.0) + id * 2.4), 3.0);
    let shimmer = 1.0 + audio * g.hat * 0.4 * (hash(vec2f(id, floor(g.time * 15.0))) - 0.5);
    let col = background.rgb + tint * (line + glow) * pulse * shimmer * (0.8 + 0.4 * audio * g.bass);
    return vec4f(col, 1.0);
}
