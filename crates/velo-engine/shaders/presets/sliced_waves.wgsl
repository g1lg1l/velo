/*{
  "name": "Sliced Waves",
  "category": "generator",
  "description": "Waves of colored light cut into glowing bars, layer behind layer, the far ones out of focus; the bass raises the waves, the hats flash single bars, the kick lights them all",
  "params": [
    { "id": "bars", "type": "float", "default": 48.0, "min": 8.0, "max": 160.0 },
    { "id": "layers", "type": "int", "default": 3, "min": 1, "max": 5 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "amplitude", "type": "float", "default": 0.2, "min": 0.0, "max": 0.45 },
    { "id": "height", "type": "float", "default": 0.22, "min": 0.02, "max": 0.6 },
    { "id": "color_a", "type": "color", "default": [0.2, 0.6, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.3, 0.7, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Sliced Waves (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    var col = background.rgb;
    for (var i = 0; i < layers; i++) {
        // 0 the nearest layer, 1 the farthest: smaller, dimmer, out of focus
        let k = f32(i) / max(f32(layers - 1), 1.0);
        let zoom = 1.0 + 0.6 * k;
        let p = s * zoom + vec2f(k * 0.37, 0.0);
        let x = p.x * bars / g.aspect;
        let id = floor(x);
        let f = fract(x) - 0.5;
        // the wave where this bar's middle is: the whole bar sits there
        let at = (id + 0.5) / bars * g.aspect;
        let y = amplitude * (1.0 + 0.6 * audio * g.bass) * (sin(at * 3.0 + t * 1.5 + k * 2.0) * 0.6 + sin(at * 7.3 - t * 1.1 + k * 5.0) * 0.4);
        let tall = height * (0.35 + 0.65 * (0.5 + 0.5 * sin(at * 4.0 - t + k * 3.0)));
        let dy = abs(p.y - y);
        let aa = zoom / g.resolution.y;
        let blur = 0.002 + 0.03 * k;
        let across = bars / g.aspect;
        let bar = (1.0 - smoothstep(0.3 - across * (blur + aa), 0.3 + across * blur, abs(f))) * (1.0 - smoothstep(tall - blur - aa, tall + blur, dy));
        let spill = exp(-max(dy - tall, 0.0) / (0.03 + 0.05 * k)) * 0.2 * (1.0 - smoothstep(0.2, 0.5, abs(f)));
        let tint = mix(color_a.rgb, color_b.rgb, 0.5 + 0.5 * sin(at * 2.5 + t * 0.6 + k * 1.5));
        // brightest along the wave, fading to the bar's ends
        let shade = (1.0 - 0.55 * k) * (0.45 + 0.55 * (1.0 - min(dy / tall, 1.0)));
        let flash = audio * g.hat * step(0.9, fract(sin(id * 12.9898 + floor(g.time * 12.0) * 4.1 + k * 7.0) * 43758.5453));
        col += tint * (bar * (shade + flash) + spill * (1.0 - 0.5 * k)) * (1.0 + 0.6 * audio * g.kick);
    }
    return vec4f(col, 1.0);
}
