/*{
  "name": "Traces",
  "category": "generator",
  "description": "Thin glowing lines drawn across the dark, straight or as box outlines, each wavering a while and then redrawn somewhere else; the hats shake them, the kick flares them",
  "params": [
    { "id": "style", "type": "int", "default": 0, "names": ["Lines", "Boxes"] },
    { "id": "lines", "type": "int", "default": 12, "min": 1, "max": 48 },
    { "id": "rate", "type": "float", "default": 0.5, "min": 0.05, "max": 4.0 },
    { "id": "reach", "type": "float", "default": 0.6, "min": 0.1, "max": 1.0 },
    { "id": "wiggle", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.2, "max": 4.0 },
    { "id": "glow", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "tint", "type": "color", "default": [0.45, 0.75, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.01, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

// smooth along x, a different curve for each s
fn noise(x: f32, s: f32) -> f32 {
    let i = floor(x);
    let f = x - i;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash(vec2f(i, s)), hash(vec2f(i + 1.0, s)), u);
}

// -0.5..0.5: big bends and fine kinks
fn wander(x: f32, s: f32) -> f32 {
    return 0.6 * (noise(x, s) - 0.5) + 0.3 * (noise(x * 2.7 + 5.0, s) - 0.5) + 0.2 * (noise(x * 7.1 + 9.0, s) - 0.5);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // x runs 0..aspect, y down 0..1
    let p = uv * vec2f(g.aspect, 1.0);
    let aa = 1.0 / g.resolution.y;
    let w = width * 0.0012;
    let shake = wiggle * (1.0 + 3.0 * audio * g.hat);
    var core = 0.0;
    var halo = 0.0;
    for (var i = 0; i < lines; i++) {
        let f = f32(i);
        // each one lives for a while, then is drawn again somewhere else
        let phase = g.time * rate + hash(vec2f(f, 1.0));
        let n = floor(phase);
        let t = phase - n;
        let r = vec4f(hash(vec2f(f + 11.0, n)), hash(vec2f(f + 23.0, n)), hash(vec2f(f + 37.0, n)), hash(vec2f(f + 51.0, n)));
        let seed = f * 7.0 + n;
        // drawn on over the first third, held, gone at the end
        let grow = smoothstep(0.0, 0.3, t);
        let life = grow * (1.0 - smoothstep(0.7, 1.0, t));
        var d = 1.0;
        var lit = 1.0;
        if style == 0 {
            // a horizontal or a vertical one, growing from its start
            let horizontal = r.w < 0.5;
            let along = select(vec2f(0.0, 1.0), vec2f(1.0, 0.0), horizontal);
            let span = select(1.0, g.aspect, horizontal);
            let across_range = select(g.aspect, 1.0, horizontal);
            let a = dot(p, along);
            let c = dot(p, along.yx);
            let dir = select(-1.0, 1.0, r.z < 0.5);
            let total = reach * span * mix(0.3, 1.0, hash(vec2f(f + 61.0, n)));
            let at = (a - r.y * span) * dir;
            let head = total * grow;
            let bend = shake * 0.04 * wander(a * 5.0 + g.time * 1.5, seed);
            d = abs(c - r.x * across_range - bend);
            lit = smoothstep(0.0, 0.01, at) * (1.0 - smoothstep(head - 0.01, head, at));
            // the tip burns brighter while it's drawn
            lit += 2.0 * (1.0 - grow) * exp(-abs(at - head) * 40.0) * step(0.0, at);
        } else {
            // a box outline, swelling out from its middle
            let centre = vec2f(r.x * g.aspect, r.y);
            let half = reach * 0.3 * vec2f(mix(0.25, 1.0, r.z), mix(0.25, 1.0, r.w)) * mix(0.6, 1.0, grow);
            let q = p - centre + shake * 0.02 * vec2f(wander(p.y * 7.0 + g.time * 1.5, seed), wander(p.x * 7.0 + g.time * 1.5, seed + 3.0));
            let e = abs(q) - half;
            d = abs(length(max(e, vec2f(0.0))) + min(max(e.x, e.y), 0.0));
        }
        core += (1.0 - smoothstep(w, w + aa, d)) * life * lit;
        halo += exp(-d / (w * 10.0)) * life * lit;
    }
    let flare = 1.0 + 1.5 * audio * g.kick;
    let col = background.rgb + (tint.rgb * halo * glow * 0.3 + mix(tint.rgb, vec3f(1.0), 0.6) * core) * flare;
    return vec4f(col, 1.0);
}
