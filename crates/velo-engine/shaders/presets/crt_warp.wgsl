/*{
  "name": "CRT Warp",
  "category": "generator",
  "description": "A blocky plasma glowing on a curved old tube, with scanlines and the red, green and blue stripes of its phosphor; the bass breathes the plasma, the kick makes the tube surge",
  "params": [
    { "id": "pixel", "type": "float", "default": 14.0, "min": 4.0, "max": 48.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "scale", "type": "float", "default": 3.0, "min": 0.5, "max": 12.0 },
    { "id": "curvature", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "scanlines", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "mask", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.1, 0.0, 0.45, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.2, 0.55, 1.0] },
    { "id": "color_c", "type": "color", "default": [0.2, 0.9, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' CRT Warp (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the tube's curve
    let c = uv * 2.0 - 1.0;
    let q = c * (1.0 + curvature * 0.3 * c.yx * c.yx) * 0.5 + 0.5;
    let inside = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    // blocks `pixel` pixels across at 540 lines, each the plasma's color at its middle
    let n = vec2f(g.aspect, 1.0) * 540.0 / pixel;
    let cell = floor(q * n);
    let p = ((cell + 0.5) / n - 0.5) * vec2f(g.aspect, 1.0) * scale * (1.0 - 0.1 * audio * g.bass);
    let t = g.time * speed;
    var v = sin(p.x + t) + sin((p.y + t) * 0.7) + sin((p.x + p.y) * 0.6 + t * 1.3);
    v += sin(length(p + vec2f(sin(t * 0.5), cos(t * 0.4)) * scale * 0.4) * 1.2 - t);
    let w = pow(max(0.5 + 0.5 * cos(v * 1.5707963 + vec3f(0.0, 2.0943951, 4.1887902)), vec3f(0.0)), vec3f(2.0));
    var col = (color_a.rgb * w.x + color_b.rgb * w.y + color_c.rgb * w.z) / (w.x + w.y + w.z + 0.0001);
    // each block a little dimmer at its edges, as a fat pixel glows
    let f = abs(fract(q * n) - 0.5);
    col *= 1.0 - 0.5 * smoothstep(0.3, 0.5, max(f.x, f.y));
    // scanlines, then the phosphor: stripes of red, green and blue three pixels wide
    col *= 1.0 - scanlines * (0.5 + 0.5 * sin(px.y * 3.1415927)) * 0.6;
    let stripe = i32(px.x) % 3;
    col *= mix(vec3f(1.0), 0.5 + 1.5 * vec3f(f32(stripe == 0), f32(stripe == 1), f32(stripe == 2)), mask * 0.6);
    // dark corners; the kick surges the whole tube
    col *= (1.0 - 0.4 * dot(c, c)) * (1.0 + 0.5 * audio * g.kick);
    return vec4f(col * inside, 1.0);
}
