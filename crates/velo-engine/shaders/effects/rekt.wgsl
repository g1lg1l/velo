/*{
  "name": "Rekt",
  "category": "effect",
  "description": "Rectangles thrown over the picture one by one, each the picture as a grey negative inside a thin line, gone after a while as new ones land on top: one a beat, or with music up one on that share of the kicks.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "deal", "scale": 0.0625 }, { "id": "boxes" }],
  "params": [
    { "id": "size", "type": "float", "default": 0.5, "min": 0.05, "max": 0.9 },
    { "id": "linger", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's rekt.wgsl (MIT, Zach Banks).

fn rekt_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(43.17, 91.73))) * 17431.37);
}

// the deal (red, counting up), the kick of the last frame (green) and whether a box lands this
// frame (blue), the same in every texel
fn buffer_deal(uv: vec2f, px: vec2f) -> vec4f {
    let last = textureLoad(deal, vec2i(0), 0);
    let beat = floor(g.beats) != floor(g.beats - g.delta * g.bpm / 60.0);
    // the kick as it lands, and only that share of the kicks
    let hit = g.kick >= 0.5 && last.g < 0.5 && rekt_hash(vec2f(last.r, 7.0)) < music;
    let dealt = f32(select(beat, hit, music > 0.0));
    // the count wraps well before a half float stops counting whole numbers
    return vec4f((last.r + dealt) % 1024.0, g.kick, dealt, 1.0);
}

// how much of a box is left at each place, running down each frame and full again where a new
// one lands
fn buffer_boxes(uv: vec2f, px: vec2f) -> vec4f {
    let d = textureLoad(deal, vec2i(0), 0);
    let keep = pow(mix(0.9, 0.995, linger), g.delta * 60.0);
    // a box somewhere on the picture, wide or tall, its corners never off it
    let wh = (vec2f(rekt_hash(vec2f(d.r, 2.0)), rekt_hash(vec2f(d.r, 3.0))) * 0.75 + 0.25) * size;
    let at = vec2f(rekt_hash(vec2f(d.r, 0.0)), rekt_hash(vec2f(d.r, 1.0))) * (1.0 - wh);
    let inside = all(uv >= at) && all(uv < at + wh);
    // a pixel in from its border (green) is its edge, kept until another box lands over it
    let from_edge = min(uv - at, at + wh - uv) * g.resolution;
    let edge = f32(min(from_edge.x, from_edge.y) < 1.5);
    let last = textureSampleLevel(boxes, smp, uv, 0.0);
    let fresh = inside && d.b > 0.5;
    return vec4f(select(last.r * keep, 1.0, fresh), select(last.g, edge, fresh), 0.0, 1.0);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let inp = textureSample(src, smp, uv);
    // a box stays whole most of its life and goes in a moment at the end
    let b = textureLoad(boxes, vec2i(uv * vec2f(textureDimensions(boxes))), 0);
    let a = smoothstep(0.02, 0.1, b.r);
    let negative = vec3f(inp.a - dot(inp.rgb, vec3f(0.2126, 0.7152, 0.0722)));
    let c = mix(mix(inp.rgb, negative, a), vec3f(0.85) * inp.a, b.g * a);
    return vec4f(c, inp.a);
}
