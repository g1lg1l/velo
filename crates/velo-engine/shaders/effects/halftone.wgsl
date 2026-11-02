/*{
  "name": "Halftone",
  "category": "effect",
  "description": "Ink dots on paper breathe with the level of the music.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "size", "type": "float", "default": 9.0, "min": 3.0, "max": 48.0 },
    { "id": "contrast", "type": "float", "default": 1.2, "min": 0.25, "max": 3.0 },
    { "id": "ink", "type": "color", "default": [0.05, 0.04, 0.08, 1.0] },
    { "id": "paper", "type": "color", "default": [0.95, 0.91, 0.8, 1.0] },
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
    let q = (floor(px / size) + 0.5) * size / g.resolution;
    let sample_color = cell_color(q, vec2f(size));
    let brightness = clamp((dot(sample_color, vec3f(0.2126, 0.7152, 0.0722)) - 0.5) * contrast + 0.5, 0.0, 1.0);
    let radius = sqrt(1.0 - brightness) * 0.71 * (1.0 + 0.12 * music * g.level);
    let distance = length(fract(px / size) - 0.5);
    let dot_mask = 1.0 - smoothstep(radius - 0.5 / size, radius + 0.5 / size, distance);
    return vec4f(mix(paper.rgb, ink.rgb, dot_mask), original.a);
}
