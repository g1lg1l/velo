/*{
  "name": "Pixelate",
  "category": "effect",
  "description": "Square or stretched pixels grow gently with the bass.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 12.0, "min": 1.0, "max": 128.0 },
    { "id": "stretch", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
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
    let cell = max(vec2f(1.0), size * vec2f(stretch, 1.0) * (1.0 + 0.5 * music * g.bass));
    let q = (floor(px / cell) + 0.5) * cell / g.resolution;
    return vec4f(cell_color(q, cell), original.a);
}
