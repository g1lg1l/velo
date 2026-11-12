/*{
  "name": "Reaction",
  "category": "generator",
  "description": "Two chemicals feeding and eating each other: spots, stripes and coral grow and divide by themselves, growing back wherever a drop falls in and clears a patch, a big one on the kick",
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

// what a drop does where it lands: x clears the patch (all u, no v), y puts v at its heart, so
// the pattern grows back into it and never comes to rest. One place a drop, the frame it
// falls; each kick's somewhere new, bigger
fn drip(uv: vec2f) -> vec2f {
    let round = vec2f(g.aspect, 1.0);
    var d = vec2f(0.0);
    let tick = floor(g.time * drops);
    if drops > 0.0 && tick != floor((g.time - g.delta) * drops) {
        let place = 0.1 + 0.8 * vec2f(hash(tick), hash(tick + 17.0));
        let far = length((uv - place) * round);
        d = vec2f(1.0 - smoothstep(0.1, 0.14, far), 1.0 - smoothstep(0.0, 0.03, far));
    }
    // (the kick stays high a few frames: the same place for all of them)
    let beat = floor(g.time * 4.0);
    let far = length((uv - 0.15 - 0.7 * vec2f(hash(beat + 3.0), hash(beat + 41.0))) * round);
    let hit = step(0.01, audio) * step(0.9, g.kick);
    let reach = 0.1 + 0.05 * audio;
    return max(d, hit * vec2f(1.0 - smoothstep(reach, reach + 0.04, far), 1.0 - smoothstep(0.0, 0.035, far)));
}

fn buffer_s0(uv: vec2f, px: vec2f) -> vec4f {
    var s = react(s3, uv);
    // nothing drawn yet (a buffer starts as zeros): spots of v all over, so the pattern is
    // there within seconds, not only where drops have fallen
    if textureSampleLevel(s3, smp, uv, 0.0).a < 0.5 {
        let q = uv * vec2f(g.aspect, 1.0) * 9.0;
        let cell = floor(q);
        let id = cell.x * 7.3 + cell.y * 31.7;
        // one in each cell of a coarse grid, each anywhere in its cell and its own size
        let spot = 0.25 + 0.5 * vec2f(hash(id + 1.0), hash(id + 2.0));
        s = vec2f(0.0, 1.0 - smoothstep(0.04, 0.07 + 0.08 * hash(id + 3.0), length(fract(q) - spot)));
    }
    let d = drip(uv);
    s = mix(s, vec2f(0.0), d.x);
    return vec4f(s.x, max(s.y, d.y), 0.0, 1.0);
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
    let c = at(s3, uv);
    let v = c.y;
    // lit from the top left: the pattern as a relief
    let slope = (at(s3, uv + vec2f(e.x, 0.0)).y - at(s3, uv - vec2f(e.x, 0.0)).y) + (at(s3, uv + vec2f(0.0, e.y)).y - at(s3, uv - vec2f(0.0, e.y)).y);
    let body = smoothstep(0.04, 0.3, v);
    let light = 0.4 + relief * max(-slope * 5.0, 0.0) + 0.5 * v;
    // where it has eaten the ground bare, a faint glow of the tint's opposite color
    let ground = background.rgb + tint.bgr * 0.2 * smoothstep(0.1, 0.6, c.x) * (1.0 - body);
    let lit = 1.0 + 0.5 * audio * g.kick + 0.3 * audio * g.bass;
    return vec4f(mix(ground, tint.rgb * light * lit, body), 1.0);
}
