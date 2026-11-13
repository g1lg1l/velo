/*{
  "name": "Pixel Sort",
  "category": "effect",
  "description": "Runs of bright pixels along each row or column sorted by brightness, light to dark, into streaks up to length long; the kick lowers the threshold, so more of the picture smears.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "rank" }],
  "params": [
    { "id": "threshold", "type": "float", "default": 0.2, "min": 0.0, "max": 1.0 },
    { "id": "length", "type": "int", "default": 120, "min": 4, "max": 160 },
    { "id": "direction", "type": "int", "default": 0, "names": ["Rows", "Columns"] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After the idea of cables' PixelSort_v2 (MIT): no texture moves pixels, so a first pass works
// out where in its run each pixel goes, and the picture fetches, for each place, the pixel that
// goes there. A run is a stretch of pixels over the threshold, cut into blocks of `length` that
// start at a different place on every line, so the streaks don't line up.
// shortcut: `rank` is read at the input's pixels, as the canvas's; an input of another size would
// need its own rank buffer.

const SORT_MAX: i32 = 160;

fn sort_value(p: vec2i) -> f32 {
    return dot(textureLoad(src, p, 0).rgb, vec3f(0.2126, 0.7152, 0.0722));
}

fn sort_dir() -> vec2i {
    return select(vec2i(1, 0), vec2i(0, 1), direction == 1);
}

// the run the pixel at `p` is in, as its first and last index along the line; (1, 0) when it
// isn't in one
fn sort_run(p: vec2i, size: vec2i) -> vec2i {
    let dir = sort_dir();
    let index = dot(p, dir);
    let line = dot(p, vec2i(1, 1) - dir);
    let count = dot(size, dir);
    let start = p - dir * index;
    let bar = threshold * (1.0 - 0.6 * music * g.kick);
    if sort_value(p) < bar {
        return vec2i(1, 0);
    }
    let block = clamp(length, 1, SORT_MAX);
    let shift = i32(fract(sin(f32(line) * 12.9898) * 43758.5453) * f32(block));
    let block_start = max(((index + shift) / block) * block - shift, 0);
    let block_end = min(((index + shift) / block) * block - shift + block, count) - 1;
    var first = index;
    for (var i = 0; i < SORT_MAX; i++) {
        if first <= block_start || sort_value(start + dir * (first - 1)) < bar {
            break;
        }
        first--;
    }
    var last = index;
    for (var i = 0; i < SORT_MAX; i++) {
        if last >= block_end || sort_value(start + dir * (last + 1)) < bar {
            break;
        }
        last++;
    }
    return vec2i(first, last);
}

// how many pixels of its run sort before this one: the brighter first, ties by place
fn buffer_rank(uv: vec2f, px: vec2f) -> vec4f {
    let size = vec2i(textureDimensions(src));
    let p = min(vec2i(uv * vec2f(size)), size - 1);
    let run = sort_run(p, size);
    if run.x > run.y {
        return vec4f(-1.0, 0.0, 0.0, 1.0);
    }
    let dir = sort_dir();
    let index = dot(p, dir);
    let start = p - dir * index;
    let value = sort_value(p);
    var rank = 0;
    for (var j = run.x; j <= run.y; j++) {
        let v = sort_value(start + dir * j);
        if j != index && (v > value || (v == value && j < index)) {
            rank++;
        }
    }
    return vec4f(f32(rank), 0.0, 0.0, 1.0);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let size = vec2i(textureDimensions(src));
    let p = min(vec2i(uv * vec2f(size)), size - 1);
    let run = sort_run(p, size);
    if run.x > run.y {
        return textureLoad(src, p, 0);
    }
    let dir = sort_dir();
    let start = p - dir * dot(p, dir);
    let place = dot(p, dir) - run.x;
    for (var j = run.x; j <= run.y; j++) {
        if i32(textureLoad(rank, start + dir * j, 0).r + 0.5) == place {
            return textureLoad(src, start + dir * j, 0);
        }
    }
    return textureLoad(src, p, 0);
}
