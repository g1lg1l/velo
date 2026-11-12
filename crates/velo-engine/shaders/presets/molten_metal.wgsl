/*{
  "name": "Molten Metal",
  "category": "generator",
  "description": "Glowing filaments swirling like light on molten metal, white-hot where they cross; the bass heats them, the kick flares them",
  "params": [
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "scale", "type": "float", "default": 1.5, "min": 0.4, "max": 6.0 },
    { "id": "heat", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "swirl", "type": "float", "default": 0.55, "min": 0.0, "max": 1.2 },
    { "id": "color_a", "type": "color", "default": [0.6, 0.08, 0.02, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.55, 0.1, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.0, 0.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Molten Metal (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    var q = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale * 2.0;
    let t = g.time * speed;
    var glow = 0.0;
    // the surface stirred five times over; after each stir, a filament where a wave of it
    // passes through zero, the later ones finer and fainter
    for (var i = 0; i < 5; i++) {
        let f = f32(i);
        q += swirl * vec2f(sin(q.y * 1.7 + t + f * 1.3), cos(q.x * 1.5 - t * 0.8 + f * 2.1));
        glow += 0.03 / (abs(sin(q.x + q.y * 0.6 + f * 0.9 + t * 0.3)) + 0.03) / (1.0 + 0.6 * f);
    }
    glow *= heat * (1.0 + 0.6 * audio * g.bass + 0.8 * audio * g.kick);
    // heat as color: deep, then hot, then white
    var col = background.rgb + color_a.rgb * smoothstep(0.0, 0.6, glow) * (0.3 + glow);
    col = mix(col, color_b.rgb, smoothstep(0.5, 1.4, glow));
    col += vec3f(1.0, 0.95, 0.85) * smoothstep(1.2, 2.4, glow);
    return vec4f(col, 1.0);
}
