/*{
  "name": "LED Wall",
  "category": "effect",
  "description": "The picture on a wall of LEDs: each a round or square light of its cell's color, dark gaps between, brighter on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 14.0, "min": 4.0, "max": 64.0 },
    { "id": "gap", "type": "float", "default": 0.25, "min": 0.0, "max": 0.8 },
    { "id": "glow", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "round", "type": "bool", "default": true },
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
    let cell = vec2f(size);
    let at = (floor(px / cell) + 0.5) * cell / g.resolution;
    let color = cell_color(at, cell);
    // where in its cell: 0 at the middle, 0.5 at the edge
    let q = fract(px / cell) - 0.5;
    let d = select(max(abs(q.x), abs(q.y)), length(q), round);
    let r = 0.5 * (1.0 - gap);
    let aa = 1.0 / size;
    let lamp = 1.0 - smoothstep(r - aa, r + aa, d);
    let halo = exp(-max(d - r, 0.0) * 14.0) * glow * 0.5;
    return vec4f(color * (lamp + halo) * (1.0 + 0.6 * music * g.kick), original.a);
}
