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
    var light = vec3f(0.0);
    for (var i = 0; i < count; i++) {
        let k = f32(i);
        let h = vec3f(hash(vec2f(k, 1.0)), hash(vec2f(k, 2.0)), hash(vec2f(k, 3.0)));
        // drifting on slow loops of their own, nearer ones bigger and fainter
        let at = (h.xy - 0.5) * vec2f(g.aspect, 1.0) * 1.2 + 0.15 * vec2f(sin(t * (0.5 + h.z) + k), cos(t * (0.4 + h.x) + k * 1.3));
        let r = size * (0.5 + h.z) * (1.0 + 0.3 * audio * g.kick);
        let d = length(p - at);
        let disc = 1.0 - smoothstep(r * (1.0 - softness), r, d);
        let rim = smoothstep(r * 0.6, r, d) * disc;
        light += mix(color_a.rgb, color_b.rgb, h.x) * (disc * 0.4 + rim * 0.3) * (1.2 - h.z * 0.6);
    }
    return vec4f(background.rgb + light * (1.0 + 0.4 * audio * g.bass), 1.0);
}
