/*{
  "name": "Moiré",
  "category": "generator",
  "description": "Two patterns of rings or lines laid over each other, their interference rippling as they drift apart with the bass",
  "params": [
    { "id": "pattern", "type": "int", "default": 0, "names": ["Rings", "Lines"] },
    { "id": "density", "type": "float", "default": 40.0, "min": 5.0, "max": 120.0 },
    { "id": "apart", "type": "float", "default": 0.12, "min": 0.0, "max": 0.5 },
    { "id": "speed", "type": "float", "default": 0.2, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [0.02, 0.02, 0.05, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.95, 0.95, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// bands of a value, smoothed over about a pixel
fn bands(v: f32) -> f32 {
    let w = fwidth(v);
    let f = abs(fract(v) - 0.5);
    return smoothstep(0.25 - w, 0.25 + w, f);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    let off = apart * (1.0 + audio * g.bass);
    // two centers going round each other, or two sets of lines turning against each other
    let c = off * vec2f(cos(t), sin(t * 0.7));
    var a = 0.0;
    var b = 0.0;
    if pattern == 1 {
        let turn = off * 1.5 + 0.05 * sin(t);
        a = p.y * density;
        b = (p.y * cos(turn) + p.x * sin(turn)) * density;
    } else {
        a = length(p - c) * density;
        b = length(p + c) * density;
    }
    // where the two disagree it's lit: the interference
    let v = abs(bands(a) - bands(b));
    let col = mix(color_a.rgb, color_b.rgb, v) * (1.0 + 0.4 * audio * g.kick);
    return vec4f(col, 1.0);
}
