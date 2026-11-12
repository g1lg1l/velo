/*{
  "name": "Glitch",
  "category": "effect",
  "description": "A signal breaking up: bands and blocks torn sideways in bursts that never come twice the same, colors split every which way, static, and now and then a yellow or blue smear left behind. Harder on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "feedback": true,
  "params": [
    { "id": "shift", "type": "float", "default": 0.04, "min": 0.0, "max": 0.3 },
    { "id": "blocks", "type": "float", "default": 24.0, "min": 2.0, "max": 100.0 },
    { "id": "split", "type": "float", "default": 3.0, "min": 0.0, "max": 24.0 },
    { "id": "rate", "type": "float", "default": 12.0, "min": 1.0, "max": 60.0 },
    { "id": "chaos", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "trails", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn glitch_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(43.17, 91.73))) * 17431.37);
}

fn glitch_draw(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let pulse = music * g.kick;
    let slot = floor(g.time * rate);
    // a slower clock deals the bursts: stretches of calm, stretches gone wild
    let spell = floor(g.time * rate * 0.17);
    let wild = clamp(smoothstep(0.55 - 0.5 * chaos, 0.95, glitch_draw(spell * 1.31 + 4.0)) + pulse, 0.0, 1.0);
    // bands of a height that changes with every slot: the same stripes never come back
    let rows = blocks * (0.35 + 1.3 * glitch_draw(slot * 0.71));
    let band = floor(uv.y * rows);
    let seed = vec2f(band, slot);
    let torn = step(0.78 - 0.5 * wild, glitch_hash(seed));
    var offset = (glitch_hash(seed + 7.0) - 0.5) * shift * (1.0 + 3.0 * pulse) * torn * (0.4 + 2.2 * glitch_hash(seed + 3.3));
    // fine lines tear on top of them, each its own way
    let fine = floor(uv.y * rows * 7.0);
    offset += (glitch_hash(vec2f(fine, slot * 1.7)) - 0.5) * shift * 0.35 * step(0.9 - 0.35 * wild * chaos, glitch_hash(vec2f(fine + 13.0, slot)));
    // blocks lifted from somewhere else in the picture
    let cell = floor(uv * vec2f(g.aspect, 1.0) * rows * 0.6 * (0.5 + glitch_draw(slot * 2.3)));
    let lifted = step(0.93 - 0.3 * wild * chaos, glitch_hash(cell + slot * 0.37));
    let jump = (vec2f(glitch_hash(cell + 1.7 + slot), glitch_hash(cell + 9.1 + slot)) - 0.5) * shift * 3.0 * lifted;
    // and, rarely, the whole picture rolls for a slot
    let roll = step(0.97 - 0.1 * wild * chaos, glitch_draw(slot * 3.7)) * (glitch_draw(slot * 5.1) - 0.5) * 0.3 * chaos;
    let q = fract(vec2f(uv.x + offset, uv.y + roll) + jump);
    // the colors split a different way and distance in every band
    let turn = glitch_hash(seed + 21.0) * 6.2831853 * chaos;
    let chroma = vec2f(cos(turn), sin(turn) * g.aspect) * split * (1.0 + 2.0 * pulse) * (0.5 + 1.5 * glitch_hash(seed + 5.0) * torn) / g.resolution.x;
    var c = vec3f(textureSample(src, smp, q + chroma).r, textureSample(src, smp, q).g, textureSample(src, smp, q - chroma).b);
    // static in some torn bands; one dropped to black or burnt out now and then
    let grit = glitch_hash(px + slot * 13.0);
    c = mix(c, vec3f(grit), 0.35 * torn * step(0.8, glitch_hash(seed + 11.0)) * chaos);
    c *= 1.0 - step(0.985, glitch_hash(seed + 31.0)) * torn * chaos;
    c += step(0.99, glitch_hash(seed + 41.0)) * torn * 0.6 * chaos;
    // the smear: what a tear left behind slides on and fades, yellow in one spell and blue in
    // another, long while it's wild and short once it calms
    let back = textureSample(prev, smp, fract(uv + vec2f((glitch_hash(seed + 17.0) - 0.5) * 0.02 * wild, 0.0))).rgb;
    let left = max(back - c, vec3f(0.0));
    let glow = max(left.r, max(left.g, left.b));
    let tint = mix(vec3f(1.0, 0.85, 0.1), vec3f(0.15, 0.45, 1.0), step(0.5, glitch_draw(spell * 7.3 + band * 0.13)));
    c += mix(left, tint * glow, 0.75 * wild) * trails * mix(0.5, 0.94, wild);
    return vec4f(c, original.a);
}
