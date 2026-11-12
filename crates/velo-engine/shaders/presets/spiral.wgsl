/*{
  "name": "Spiral",
  "category": "generator",
  "description": "A hypnotic spiral turning into the center, its arms fattening with the bass",
  "params": [
    { "id": "arms", "type": "int", "default": 3, "min": 1, "max": 12 },
    { "id": "twist", "type": "float", "default": 4.0, "min": -12.0, "max": 12.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": -2.0, "max": 2.0 },
    { "id": "sharpness", "type": "float", "default": 0.7, "min": 0.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.05, 0.0, 0.15, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.4, 0.2, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = max(length(p), 0.0005);
    let n = f32(arms);
    // the same phase along each logarithmic spiral
    let v = sin(n * atan2(p.y, p.x) + twist * log(r) - g.time * speed * 6.0) + 0.5 * audio * g.bass;
    // soft to hard edges, but never sharper than a pixel where the arms crowd the center
    let crowd = sqrt(n * n + twist * twist) / (r * g.resolution.y);
    let e = max(mix(1.0, 0.03, sharpness), crowd * 1.5);
    let band = smoothstep(-e, e, v);
    var col = mix(color_a.rgb, color_b.rgb, band);
    // each arm rounded like a ribbon: brightest along its middle, and a thin light where the
    // two colors meet
    col *= 0.62 + 0.5 * abs(v);
    col += color_b.rgb * exp(-abs(v) / max(e, 0.04)) * 0.3;
    col *= 0.65 + 0.55 * exp(-r * 2.0);
    col += color_b.rgb * exp(-r * 12.0) * (0.7 + 1.2 * audio * g.kick);
    col *= 1.0 + 0.4 * audio * g.kick + 0.08 * audio * g.hat * sin(r * 80.0 - g.time * 30.0);
    return vec4f(col, 1.0);
}
