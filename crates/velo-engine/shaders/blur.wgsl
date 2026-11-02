/*{
  "name": "Blur",
  "category": "effect",
  "description": "Round, horizontal, vertical or zoom blur",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "radius", "type": "float", "default": 4.0, "min": 0.0, "max": 32.0 },
    { "id": "kind", "type": "int", "default": 0, "names": ["Round", "Horizontal", "Vertical", "Zoom"] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the kick blurs it more
    let texel = (1.0 + 2.0 * music * g.kick) / g.resolution;
    var sum = vec4f(0.0);
    for (var i = 0; i < 25; i++) {
        let t = (f32(i) + 0.5) / 25.0;
        let a = f32(i) * 2.39996;
        // round: a sunflower of taps; a line either way; or toward the center
        var off = vec2f(cos(a), sin(a)) * sqrt(t) * radius * texel;
        if kind == 1 {
            off = vec2f((t - 0.5) * 2.0 * radius * texel.x, 0.0);
        } else if kind == 2 {
            off = vec2f(0.0, (t - 0.5) * 2.0 * radius * texel.y);
        } else if kind == 3 {
            off = (uv - 0.5) * (t - 0.5) * radius * 0.02;
        }
        sum += textureSampleLevel(src, smp, uv + off, 0.0);
    }
    return sum / 25.0;
}
