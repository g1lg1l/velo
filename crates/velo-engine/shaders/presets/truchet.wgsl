/*{
  "name": "Truchet",
  "category": "generator",
  "description": "Quarter circle tiles joined into endless winding paths, light running across them",
  "params": [
    { "id": "scale", "type": "float", "default": 8.0, "min": 2.0, "max": 40.0 },
    { "id": "width", "type": "float", "default": 0.2, "min": 0.02, "max": 0.9 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.02, 0.04, 0.1, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.2, 0.95, 0.75, 1.0] },
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
    let t = g.time * speed;
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale * (1.0 - 0.06 * audio * g.bass) + vec2f(t * 0.5, t * 0.2);
    let cell = floor(p);
    var f = fract(p) - 0.5;
    // half the tiles mirrored: the arcs join up either way
    if hash(cell) > 0.5 {
        f.x = -f.x;
    }
    // two arcs around opposite corners
    let d = min(abs(length(f - 0.5) - 0.5), abs(length(f + 0.5) - 0.5));
    let aa = scale / g.resolution.y;
    let w = width * 0.25 * (1.0 + 0.4 * audio * g.kick);
    let band = 1.0 - smoothstep(w - aa, w + aa, d);
    // rounded like a tube, and a slow wave of light crossing the pattern
    let tube = sqrt(max(1.0 - d * d / (w * w), 0.0));
    var wave = 0.35 + 0.65 * pow(0.5 + 0.5 * sin(dot(p, vec2f(0.35, 0.25)) - t * 4.0), 2.0);
    wave *= 1.0 + 0.5 * audio * g.hat;
    var col = mix(color_a.rgb, color_b.rgb * (0.35 + 0.65 * tube) * wave, band);
    col += color_b.rgb * exp(-max(d - w, 0.0) * 12.0) * 0.15 * wave;
    return vec4f(col, 1.0);
}
