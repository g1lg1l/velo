/*{
  "name": "ASCII",
  "category": "effect",
  "description": "Brightness becomes dots, bars, crosses and hash glyphs that brighten with the mids.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 12.0, "min": 6.0, "max": 48.0 },
    { "id": "weight", "type": "float", "default": 0.07, "min": 0.03, "max": 0.14 },
    { "id": "colored", "type": "bool", "default": true },
    { "id": "tint", "type": "color", "default": [0.35, 1.0, 0.55, 1.0] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// the average color over a cell (its size in pixels, `at` its center as uv): nine taps, each
// a bilinear blend of four pixels
fn cell_color(at: vec2f, cell: vec2f) -> vec3f {
    var sum = vec3f(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            sum += textureSampleLevel(src, smp, at + vec2f(f32(x), f32(y)) * cell / (3.0 * g.resolution), 0.0).rgb;
        }
    }
    return sum / 9.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let cell = vec2f(size, size * 1.4);
    let q = (floor(px / cell) + 0.5) * cell / g.resolution;
    let color = cell_color(q, cell);
    // the mids lift every cell a step or so, the kick a few cells at random more
    let cell_id = floor(px / cell);
    let jolt = step(0.8, fract(sin(dot(cell_id + floor(g.time * 12.0), vec2f(12.9898, 78.233))) * 43758.5453)) * g.kick;
    let brightness = clamp(dot(color, vec3f(0.2126, 0.7152, 0.0722)) + music * (0.2 * g.mid + 0.3 * jolt), 0.0, 1.0);
    let p = abs(fract(px / cell) - 0.5);
    let aa = 0.5 / size;
    let dot_shape = 1.0 - smoothstep(weight - aa, weight + aa, length(p));
    let bar = 1.0 - smoothstep(weight - aa, weight + aa, p.y);
    let cross_shape = 1.0 - smoothstep(weight - aa, weight + aa, min(p.x, p.y));
    let hash_shape = 1.0 - smoothstep(weight - aa, weight + aa, min(abs(p.x - 0.18), abs(p.y - 0.18)));
    let frame_shape = 1.0 - smoothstep(weight - aa, weight + aa, abs(max(p.x, p.y) - 0.28));
    var glyph = dot_shape * step(0.08, brightness);
    glyph = select(glyph, bar, brightness >= 0.25);
    glyph = select(glyph, cross_shape, brightness >= 0.45);
    glyph = select(glyph, hash_shape, brightness >= 0.65);
    glyph = select(glyph, max(hash_shape, frame_shape), brightness >= 0.85);
    let margin = 1.0 - smoothstep(0.36, 0.4, max(p.x, p.y));
    // a cell's own color, lifted so a thin glyph still reads
    let ink = select(tint.rgb * (0.35 + 0.65 * brightness), color * (1.0 + 0.8 * (1.0 - brightness)), colored);
    return vec4f(ink * glyph * margin * (1.0 + 0.5 * music * g.kick), original.a);
}
