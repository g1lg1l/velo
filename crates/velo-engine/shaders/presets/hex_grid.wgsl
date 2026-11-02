/*{
  "name": "Hex Grid",
  "category": "generator",
  "description": "A honeycomb whose cells light up in waves from the center, a ring rippling out on each kick",
  "params": [
    { "id": "scale", "type": "float", "default": 12.0, "min": 2.0, "max": 40.0 },
    { "id": "width", "type": "float", "default": 0.08, "min": 0.01, "max": 0.4 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.1, 0.35, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.3, 0.75, 1.0] },
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

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale;
    // two rectangular grids half a cell apart: the nearer center is the hexagon's
    let s = vec2f(1.0, 1.7320508);
    let a = p - s * floor(p / s) - s * 0.5;
    let shifted = p - s * 0.5;
    let b = shifted - s * floor(shifted / s) - s * 0.5;
    let local = select(b, a, dot(a, a) < dot(b, b));
    let id = p - local;
    // 0 at the center, 0.5 on the edge
    let q = abs(local);
    let to_edge = 0.5 - max(dot(q, vec2f(0.5, 0.8660254)), q.x);
    let aa = scale / g.resolution.y;
    let line = 1.0 - smoothstep(width * 0.5, width * 0.5 + aa, to_edge);
    // how far the cell is from the center, in picture heights
    let r = length(id) / scale;
    let h = hash(id * 2.0 + 0.5);
    var light = pow(0.5 + 0.5 * sin(r * 14.0 - g.time * speed * 4.0 + h), 4.0);
    // the kick's ring moves out as the kick fades
    light += audio * g.kick * exp(-abs(r - (1.0 - g.kick) * 0.9) * 18.0);
    light += audio * g.hat * step(0.93, hash(id * 2.0 + 0.5 + floor(g.time * 10.0) * 7.0)) * 0.7;
    let tint = mix(color_a.rgb, color_b.rgb, clamp(r * 1.4 + 0.25 * h, 0.0, 1.0));
    var col = background.rgb + tint * light * (0.25 + 0.75 * smoothstep(0.0, 0.4, to_edge));
    col = mix(col, tint * (0.5 + light), line);
    return vec4f(col, 1.0);
}
