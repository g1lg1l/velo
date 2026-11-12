/*{
  "name": "Bokeh",
  "category": "generator",
  "description": "Soft out-of-focus lights drifting in the dark, each a disc with a brighter rim, swelling on the kick",
  "params": [
    { "id": "count", "type": "int", "default": 28, "min": 4, "max": 64 },
    { "id": "size", "type": "float", "default": 0.07, "min": 0.01, "max": 0.25 },
    { "id": "speed", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "softness", "type": "float", "default": 0.3, "min": 0.02, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [1.0, 0.55, 0.2, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.35, 0.5, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.01, 0.04, 1.0] },
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

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    let aa = 1.5 / g.resolution.y;
    // the lights wrap round a field a little larger than the picture, so none pops in or out
    let span = vec2f(g.aspect, 1.0) + size * 3.2;
    var light = vec3f(0.0);
    for (var i = 0; i < count; i++) {
        let k = f32(i);
        let h = vec3f(hash(vec2f(k, 1.0)), hash(vec2f(k, 2.0)), hash(vec2f(k, 3.0)));
        // 0 far (small, sharp, bright) to 1 near (big, faint, rising faster)
        let depth = hash(vec2f(k, 4.0));
        var at = h.xy * span + vec2f(0.12 * sin(t * (0.5 + h.z) + k), 0.1 * cos(t * (0.4 + h.x) + k * 1.3) - t * (0.15 + 0.35 * depth));
        at = (fract(at / span) - 0.5) * span;
        let r = size * (0.4 + 1.1 * depth) * (1.0 + 0.35 * audio * g.kick * (0.4 + h.z));
        let d = length(p - at);
        // (a light this pixel is outside of adds nothing)
        if d >= r {
            continue;
        }
        let soft = max(r * softness, aa);
        let disc = 1.0 - smoothstep(r - soft, r, d);
        // a brighter rim just inside the edge, as a lens draws it
        let rim = disc * smoothstep(r - soft - r * 0.3, r - soft, d);
        let twinkle = 0.8 + 0.2 * sin(g.time * (0.7 + 2.0 * h.y) + k * 4.0) + 0.7 * audio * g.hat * step(0.6, h.y);
        // most lights one color or the other, few in between
        light += mix(color_a.rgb, color_b.rgb, smoothstep(0.3, 0.7, h.x)) * (disc * 0.45 + rim * 0.55) * mix(1.2, 0.45, depth) * twinkle;
    }
    light *= 1.0 + 0.6 * audio * g.bass;
    // where lights overlap they add up, rolling off before they clip
    return vec4f(background.rgb + 1.0 - exp(-light * 2.2), 1.0);
}
