/*{
  "name": "Life",
  "category": "effect",
  "description": "Conway's Game of Life on a grid of small cells, born wherever the picture is bright and living on by the rules into the dark, each cell lit in the picture's color under it; the kick seeds more.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "cells", "scale": 0.125 }],
  "params": [
    { "id": "threshold", "type": "float", "default": 0.7, "min": 0.0, "max": 1.0 },
    { "id": "rate", "type": "float", "default": 12.0, "min": 1.0, "max": 60.0 },
    { "id": "background", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's life.wgsl (MIT, Zach Banks and Eric Van Albert).

// a cell of the grid (one texel of `cells`, eight pixels of the picture) is alive when its red is 1
fn life_alive(p: vec2i, size: vec2i) -> f32 {
    return textureLoad(cells, (p + size) % size, 0).r;
}

fn buffer_cells(uv: vec2f, px: vec2f) -> vec4f {
    let size = vec2i(textureDimensions(cells));
    let p = vec2i(uv * vec2f(size));
    var alive = life_alive(p, size);
    // a step of the game only on the frames a new one is due, `rate` of them a second
    if floor(g.time * rate) != floor((g.time - g.delta) * rate) {
        var n = 0.0;
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                n += life_alive(p + vec2i(x, y), size);
            }
        }
        n -= alive;
        // two neighbours keep a cell alive, three bring one to life
        alive = select(f32(n == 3.0), f32(n == 2.0 || n == 3.0), alive > 0.5);
    }
    // and wherever the picture is bright enough, a cell is born; the kick lowers the bar
    let c = textureSampleLevel(src, smp, (vec2f(p) + 0.5) / vec2f(size), 0.0).rgb;
    let seed = step(threshold * (1.0 - 0.5 * music * g.kick), dot(c, vec3f(0.2126, 0.7152, 0.0722)));
    return vec4f(max(alive, seed), 0.0, 0.0, 1.0);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let inp = textureSample(src, smp, uv);
    let size = vec2f(textureDimensions(cells));
    let cell = floor(uv * size);
    let alive = textureLoad(cells, vec2i(cell), 0).r;
    // a square in each cell with a thin gap round it, in the picture's color at the cell's middle
    let f = abs(fract(uv * size) - 0.5);
    let square = step(max(f.x, f.y), 0.4);
    let lit = textureSampleLevel(src, smp, (cell + 0.5) / size, 0.0);
    // cells living on into the dark show grey, not black
    let on = alive * square;
    return vec4f(inp.rgb * background * (1.0 - on) + max(lit.rgb, vec3f(0.25)) * on, max(inp.a, on));
}
