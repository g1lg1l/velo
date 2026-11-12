/*{
  "name": "Starfield",
  "category": "generator",
  "description": "Flying through the stars, pushed forward on the kick",
  "params": [
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 3.0 },
    { "id": "density", "type": "float", "default": 0.5, "min": 0.05, "max": 1.0 },
    { "id": "size", "type": "float", "default": 1.0, "min": 0.2, "max": 3.0 },
    { "id": "tint", "type": "color", "default": [0.7, 0.85, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.03, 1.0] },
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
    let t = g.time * speed * 0.2 + 0.02 * audio * g.kick;
    var col = background.rgb;
    // layers of stars at different depths; each comes closer, then starts over far away
    for (var i = 0; i < 6; i++) {
        let layer = f32(i);
        let z = fract(layer / 6.0 + t);
        let q = p * mix(24.0, 1.5, z) + layer * vec2f(13.7, 7.1);
        let cell = floor(q);
        let h = hash(cell + layer * 31.0);
        let at = (vec2f(h, hash(cell + vec2f(17.0, 59.0))) - 0.5) * 0.6;
        let d = length(fract(q) - 0.5 - at);
        // far stars small and faint, near ones bright; gone before they get too close
        let fade = z * (1.0 - smoothstep(0.85, 1.0, z));
        let r = size * 0.06 * (0.4 + h) * (1.0 + audio * g.kick);
        let star = (1.0 - smoothstep(0.0, r, d)) + exp(-d / r) * 0.5 * (1.0 - smoothstep(0.25, 0.5, d));
        let twinkle = 1.0 + audio * g.hat * (hash(cell + floor(g.time * 20.0)) - 0.5);
        let on = step(1.0 - density, fract(h * 7.31));
        col += mix(tint.rgb, vec3f(1.0), h) * star * fade * on * twinkle * 1.4;
    }
    return vec4f(col * (1.0 + 0.6 * audio * g.bass + 0.4 * audio * g.kick), 1.0);
}
