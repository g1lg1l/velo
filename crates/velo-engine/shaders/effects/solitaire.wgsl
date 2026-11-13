/*{
  "name": "Solitaire",
  "category": "effect",
  "description": "The end of a won game of solitaire: the picture made small bounces across the screen, leaving a copy of itself at every step, each with a thin edge, the trail fading as it goes, faster on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "copies" }],
  "params": [
    { "id": "size", "type": "float", "default": 0.3, "min": 0.1, "max": 0.8 },
    { "id": "speed", "type": "float", "default": 1.0, "min": 0.1, "max": 4.0 },
    { "id": "trail", "type": "float", "default": 0.9, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's solitaire.wgsl (MIT, Zach Banks).

// the small picture over `under` where it is now: it moves on in steps, eight a beat, so the
// copies it leaves stand apart like the cards did
fn solitaire_card(uv: vec2f, under: vec4f) -> vec4f {
    let step_at = floor(g.beats * 8.0) / 8.0;
    let phase = step_at * speed / 16.0;
    // back and forth across, and bouncing on the floor, never quite in time with each other
    let across = 1.0 - abs(fract(phase * 2.5) * 2.0 - 1.0);
    let height = abs(sin(phase * 9.0 * 3.1415927));
    let middle = 0.5 + (vec2f(across, 1.0 - height) - 0.5) * (1.0 - size);
    let q = (uv - middle) / size + 0.5;
    if any(q < vec2f(0.0)) || any(q > vec2f(1.0)) {
        return under;
    }
    // a thin grey edge round it, a pixel and a half wide
    let edge = min(min(q.x, 1.0 - q.x) * g.resolution.x, min(q.y, 1.0 - q.y) * g.resolution.y) * size;
    let c = textureSampleLevel(src, smp, q, 0.0);
    return mix(vec4f(0.6), c, smoothstep(0.5, 1.5, edge));
}

// the copies left so far, fading, cleared faster on the kick
fn buffer_copies(uv: vec2f, px: vec2f) -> vec4f {
    let keep = pow(mix(0.95, 0.999, trail), g.delta * 60.0) * (1.0 - 0.05 * music * g.kick);
    // less a little, so the oldest go to nothing rather than to a dust of dark specks
    return solitaire_card(uv, max(textureSampleLevel(copies, smp, uv, 0.0) * keep - 0.002, vec4f(0.0)));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return textureSample(copies, smp, uv);
}
