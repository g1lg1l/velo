/*{
  "name": "Scramble",
  "category": "effect",
  "description": "The picture cut into a grid of blocks, some of them swapped for blocks from elsewhere, dealt anew on every beat; with music up the kick deals instead, that share of the kicks.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "deal", "scale": 0.0625 }],
  "params": [
    { "id": "blocks", "type": "float", "default": 8.0, "min": 2.0, "max": 40.0 },
    { "id": "shuffle", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's scramble.wgsl (MIT, Zach Banks).

fn scramble_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(43.17, 91.73))) * 17431.37);
}

// the deal (red, counting up) and the kick of the last frame (green), the same in every texel
fn buffer_deal(uv: vec2f, px: vec2f) -> vec4f {
    let last = textureLoad(deal, vec2i(0), 0);
    let beat = floor(g.beats) != floor(g.beats - g.delta * g.bpm / 60.0);
    // the kick as it lands, and only that share of the kicks
    let hit = g.kick >= 0.5 && last.g < 0.5 && scramble_hash(vec2f(last.r, 7.0)) < music;
    let dealt = select(beat, hit, music > 0.0);
    // the count wraps well before a half float stops counting whole numbers
    return vec4f((last.r + f32(dealt)) % 1024.0, g.kick, 0.0, 1.0);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let seed = textureLoad(deal, vec2i(0), 0).r;
    // square blocks, `blocks` of them down the picture
    let grid = max(round(vec2f(blocks * g.aspect, blocks)), vec2f(1.0));
    let cell = floor(uv * grid);
    let moved = step(scramble_hash(cell + seed * 0.37), shuffle);
    let to = floor(vec2f(scramble_hash(cell + seed + 1.7), scramble_hash(cell + seed + 9.1)) * grid);
    let taken = mix(cell, to, moved);
    return textureSampleLevel(src, smp, (taken + fract(uv * grid)) / grid, 0.0);
}
