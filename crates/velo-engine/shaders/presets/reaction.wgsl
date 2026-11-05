/*{
  "name": "Reaction",
  "category": "generator",
  "description": "Two chemicals feeding and eating each other: spots, stripes and coral grow and divide by themselves from the drops that fall in, a big one on the kick",
  "buffers": [{ "id": "s0", "scale": 0.5 }, { "id": "s1", "scale": 0.5 }, { "id": "s2", "scale": 0.5 }, { "id": "s3", "scale": 0.5 }],
  "params": [
    { "id": "feed", "type": "float", "default": 0.055, "min": 0.01, "max": 0.1 },
    { "id": "kill", "type": "float", "default": 0.062, "min": 0.045, "max": 0.07 },
    { "id": "drops", "type": "float", "default": 1.0, "min": 0.0, "max": 8.0 },
    { "id": "relief", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "tint", "type": "color", "default": [0.8, 0.9, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.015, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Gray-Scott: u (fed in) turns to v (which eats it) where v is already. Four buffers make four
// steps a frame: s0 steps what s3 held last frame, s1 steps s0, and so on. A buffer holds
// (1 - u, v), so one nothing has drawn in yet is all u and no v.

fn hash(n: f32) -> f32 {
    return fract(sin(n * 127.1) * 43758.5453);
}

fn at(t: texture_2d<f32>, uv: vec2f) -> vec2f {
    return textureSampleLevel(t, smp, uv, 0.0).xy;
}

fn react(t: texture_2d<f32>, uv: vec2f) -> vec2f {
    let e = 1.0 / vec2f(textureDimensions(t));
    let c = at(t, uv);
    // the 3 × 3 laplacian: sides 0.2, corners 0.05, the middle -1
    var l = -c;
    l += 0.2 * (at(t, uv + vec2f(e.x, 0.0)) + at(t, uv - vec2f(e.x, 0.0)) + at(t, uv + vec2f(0.0, e.y)) + at(t, uv - vec2f(0.0, e.y)));
    l += 0.05 * (at(t, uv + e) + at(t, uv - e) + at(t, uv + vec2f(e.x, -e.y)) + at(t, uv + vec2f(-e.x, e.y)));
    let u = 1.0 - c.x;
    let v = c.y;
    let eaten = u * v * v;
    // u's laplacian is minus the stored one's
    let next_u = u - l.x - eaten + feed * (1.0 - u);
    let next_v = v + 0.5 * l.y + eaten - (feed + kill) * v;
    return vec2f(1.0 - clamp(next_u, 0.0, 1.0), clamp(next_v, 0.0, 1.0));
}

// v dropped in: one place a drop, the frame it falls, and the kick's in the middle
fn drip(uv: vec2f) -> f32 {
    let round = vec2f(g.aspect, 1.0);
    var v = 0.0;
    let tick = floor(g.time * drops);
    if drops > 0.0 && tick != floor((g.time - g.delta) * drops) {
        let place = 0.1 + 0.8 * vec2f(hash(tick), hash(tick + 17.0));
        v = 1.0 - smoothstep(0.0, 0.08, length((uv - place) * round));
    }
    return max(v, audio * g.kick * (1.0 - smoothstep(0.0, 0.1, length((uv - 0.5) * round))));
}

fn buffer_s0(uv: vec2f, px: vec2f) -> vec4f {
    let s = react(s3, uv);
    return vec4f(s.x, max(s.y, drip(uv)), 0.0, 1.0);
}

fn buffer_s1(uv: vec2f, px: vec2f) -> vec4f {
    return vec4f(react(s0, uv), 0.0, 1.0);
}

fn buffer_s2(uv: vec2f, px: vec2f) -> vec4f {
    return vec4f(react(s1, uv), 0.0, 1.0);
}

fn buffer_s3(uv: vec2f, px: vec2f) -> vec4f {
    return vec4f(react(s2, uv), 0.0, 1.0);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let e = 1.0 / vec2f(textureDimensions(s3));
    let v = at(s3, uv).y;
    // lit from the top left: the pattern as a relief
    let slope = (at(s3, uv + vec2f(e.x, 0.0)).y - at(s3, uv - vec2f(e.x, 0.0)).y) + (at(s3, uv + vec2f(0.0, e.y)).y - at(s3, uv - vec2f(0.0, e.y)).y);
    let body = smoothstep(0.04, 0.3, v);
    let light = 0.35 + relief * max(-slope * 5.0, 0.0) + 0.4 * v;
    return vec4f(mix(background.rgb, tint.rgb * light, body), 1.0);
}
