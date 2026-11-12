/*{
  "name": "Pixel Snow",
  "category": "generator",
  "description": "Pixel-art snowflakes drifting down in layers, the near ones little crosses, the far ones single pixels; the kick gusts them sideways, the hats make them glint",
  "params": [
    { "id": "density", "type": "float", "default": 0.5, "min": 0.05, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "size", "type": "float", "default": 4.0, "min": 1.0, "max": 16.0 },
    { "id": "wind", "type": "float", "default": 0.3, "min": -2.0, "max": 2.0 },
    { "id": "tint", "type": "color", "default": [0.9, 0.95, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.03, 0.08, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Pixel Snow (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // everything in big pixels (`size` of them at 540 lines), counted whole so it stays crisp
    let block = max(floor(size * g.resolution.y / 540.0), 1.0);
    let at = floor(px / block);
    var col = background.rgb * (0.7 + 0.6 * uv.y);
    for (var layer = 0; layer < 4; layer++) {
        let l = f32(layer);
        // 1 the nearest layer: the fastest, the brightest
        let near = 1.0 - l * 0.22;
        let grid = 12.0 + 8.0 * l;
        let fall = floor(g.time * speed * 50.0 * near);
        let drift = floor((g.time * wind * 12.0 + 3.0 * sin(g.time * 0.5 + l) + 8.0 * audio * g.kick) * near);
        let q = at - vec2f(drift, fall) + l * 37.0;
        let cell = floor(q / grid);
        let h = hash(cell + l * 91.0);
        // one flake a cell at most, somewhere inside it
        let middle = floor(vec2f(hash(cell + 13.0), hash(cell + 29.0)) * (grid - 4.0)) + 2.0;
        let o = abs(q - cell * grid - middle);
        // the two near layers' flakes are crosses of five pixels, the others one pixel
        let flake = step(o.x + o.y, select(0.5, 1.5, layer < 2));
        let glint = 0.75 + 0.25 * sin(g.time * 3.0 + h * 50.0) + audio * g.hat * step(0.7, fract(h * 7.3));
        col += tint.rgb * flake * step(h, density) * near * glint;
    }
    return vec4f(col, 1.0);
}
