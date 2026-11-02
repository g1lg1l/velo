/*{
  "name": "Probe",
  "category": "effect",
  "description": "A Probe node's picture side: its region drawn into 32 × 16 cells, each the average of 4 × 4 taps over its part, read back and averaged as the node's signal",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "size", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 }
  ],
  "amount": "none"
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the region: `size` of the picture each way round `position` (Shapes' units: -1 to 1)
    let lo = 0.5 + position * 0.5 - size * 0.5;
    // this cell's corner, in the region (uv is its middle in the 32 × 16 target)
    let cell = uv - vec2f(0.5 / 32.0, 0.5 / 16.0);
    var sum = vec4f(0.0);
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let at = cell + (vec2f(f32(x), f32(y)) + 0.5) / vec2f(128.0, 64.0);
            sum += textureSampleLevel(src, smp, lo + at * size, 0.0);
        }
    }
    return sum / 16.0;
}
