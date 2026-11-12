/*{
  "name": "Sunburst",
  "category": "generator",
  "description": "Retro rays turning around a glowing center",
  "params": [
    { "id": "rays", "type": "int", "default": 16, "min": 4, "max": 64 },
    { "id": "speed", "type": "float", "default": 0.05, "min": -1.0, "max": 1.0 },
    { "id": "glow", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.95, 0.35, 0.1, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.75, 0.25, 1.0] },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5 - center * 0.5) * vec2f(g.aspect, 1.0);
    let r = max(length(p), 0.0005);
    let n = f32(rays);
    let a = atan2(p.y, p.x) + g.time * speed;
    // never sharper than a pixel where the rays meet
    let e = max(0.04, n / (r * g.resolution.y) * 1.5);
    let band = smoothstep(-e, e, cos(a * n));
    // each ray flickers on its own with the hats
    let ray = floor(a * n / 6.2831853 + 0.5);
    let flicker = 1.0 + audio * g.hat * 0.3 * sin(ray * 12.9898 + floor(g.time * 12.0) * 4.1);
    var col = mix(color_a.rgb, color_b.rgb * flicker, band);
    // every other ray a shade deeper, all of them paler toward the middle
    col *= 0.9 + 0.1 * cos(ray * 3.1415927);
    col *= 0.72 + 0.45 * exp(-r * 2.0);
    col += color_b.rgb * glow * (exp(-r * 6.0 / (1.0 + 0.8 * audio * g.bass)) + 0.6 * exp(-r * 22.0));
    // the kick sends a ring of light out along the rays
    col += color_b.rgb * audio * g.kick * exp(-abs(r - (1.0 - g.kick) * 0.9) * 14.0) * 0.5;
    return vec4f(col * (1.0 + 0.3 * audio * g.kick), 1.0);
}
