/*{
  "name": "Grid",
  "category": "composite",
  "description": "A picture split into a grid of cells; each cell shows one of the eight inputs (cell_n: input 1 to 8 or 0 for none, fit 0 fill 1 fit 2 stretch, corner radius in pixels)",
  "inputs": [{ "id": "i1", "type": "texture" }, { "id": "i2", "type": "texture" }, { "id": "i3", "type": "texture" }, { "id": "i4", "type": "texture" }, { "id": "i5", "type": "texture" }, { "id": "i6", "type": "texture" }, { "id": "i7", "type": "texture" }, { "id": "i8", "type": "texture" }],
  "params": [
    { "id": "columns", "type": "int", "default": 2, "min": 1, "max": 4 },
    { "id": "rows", "type": "int", "default": 2, "min": 1, "max": 4 },
    { "id": "gap", "type": "float", "default": 0.0, "min": 0.0, "max": 200.0 },
    { "id": "gaps", "type": "int", "default": 0, "names": ["Everywhere", "Inside", "Outside"] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "cell_1", "type": "vec4", "default": [1, 0, 0, 0] },
    { "id": "cell_2", "type": "vec4", "default": [2, 0, 0, 0] },
    { "id": "cell_3", "type": "vec4", "default": [3, 0, 0, 0] },
    { "id": "cell_4", "type": "vec4", "default": [4, 0, 0, 0] },
    { "id": "cell_5", "type": "vec4", "default": [5, 0, 0, 0] },
    { "id": "cell_6", "type": "vec4", "default": [6, 0, 0, 0] },
    { "id": "cell_7", "type": "vec4", "default": [7, 0, 0, 0] },
    { "id": "cell_8", "type": "vec4", "default": [8, 0, 0, 0] },
    { "id": "cell_9", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_10", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_11", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_12", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_13", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_14", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_15", "type": "vec4", "default": [0, 0, 0, 0] },
    { "id": "cell_16", "type": "vec4", "default": [0, 0, 0, 0] }
  ]
}*/

// the cells' settings, in the parameter buffer after the five parameters above
const FIRST_CELL: i32 = 5;

// signed distance from a rounded rectangle of `size` centered on the origin
fn rounded(p: vec2f, size: vec2f, r: f32) -> f32 {
    let q = abs(p) - size * 0.5 + r;
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// one of the inputs (sampled at one level: cells aren't uniform control flow)
fn pick(input: i32, q: vec2f) -> vec4f {
    var c = vec4f(0.0);
    if input == 1 {
        c = textureSampleLevel(i1, smp, q, 0.0);
    } else if input == 2 {
        c = textureSampleLevel(i2, smp, q, 0.0);
    } else if input == 3 {
        c = textureSampleLevel(i3, smp, q, 0.0);
    } else if input == 4 {
        c = textureSampleLevel(i4, smp, q, 0.0);
    } else if input == 5 {
        c = textureSampleLevel(i5, smp, q, 0.0);
    } else if input == 6 {
        c = textureSampleLevel(i6, smp, q, 0.0);
    } else if input == 7 {
        c = textureSampleLevel(i7, smp, q, 0.0);
    } else if input == 8 {
        c = textureSampleLevel(i8, smp, q, 0.0);
    }
    return c;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let bg = vec4f(background.rgb * background.a, background.a);
    let size = g.resolution;
    // the gap around the grid (everywhere, outside) and between its cells (everywhere, inside)
    let outer = select(0.0, gap, gaps != 1);
    let inner = select(0.0, gap, gaps != 2);
    let grid = vec2f(f32(columns), f32(rows));
    let cell = max((size - 2.0 * outer - (grid - 1.0) * inner) / grid, vec2f(1.0));
    let rel = px - outer;
    let at = floor(rel / (cell + inner));
    if any(at < vec2f(0.0)) || any(at >= grid) {
        return bg;
    }
    let within = rel - at * (cell + inner);
    if any(within > cell) {
        return bg;
    }
    let index = i32(at.y) * columns + i32(at.x);
    if index > 15 {
        return bg;
    }
    let s = velo_params.v[FIRST_CELL + index];
    let input = i32(round(s.x));
    let fit = i32(round(s.y));
    let radius = clamp(s.z, 0.0, min(cell.x, cell.y) * 0.5);
    // the rounded corners, antialiased
    let cover = clamp(0.5 - rounded(within - cell * 0.5, cell, radius), 0.0, 1.0);
    if cover <= 0.0 || input < 1 {
        return bg;
    }
    // a picture made for the whole canvas, into a cell of another shape: fill, fit or stretch
    let k = (size.x / size.y) / (cell.x / cell.y);
    var scale = vec2f(1.0);
    if fit == 0 {
        scale = select(vec2f(1.0, k), vec2f(1.0 / k, 1.0), k > 1.0);
    } else if fit == 1 {
        scale = select(vec2f(1.0 / k, 1.0), vec2f(1.0, k), k > 1.0);
    }
    let q = (within / cell - 0.5) * scale + 0.5;
    var shown = bg;
    if all(q >= vec2f(0.0)) && all(q <= vec2f(1.0)) {
        shown = vec4f(pick(input, q).rgb, 1.0);
    }
    return mix(bg, shown, cover);
}
