/*{
  "name": "Ripple Grid",
  "category": "generator",
  "description": "A crisp grid that ripples like water, bending and lighting up as rings pass through it from the middle; the bass deepens the waves, the kick sends a bright one",
  "params": [
    { "id": "density", "type": "float", "default": 16.0, "min": 4.0, "max": 60.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.5, "max": 4.0 },
    { "id": "speed", "type": "float", "default": 1.0, "min": -3.0, "max": 3.0 },
    { "id": "ripples", "type": "float", "default": 3.0, "min": 0.5, "max": 12.0 },
    { "id": "strength", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "color_a", "type": "color", "default": [0.3, 0.45, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.7, 0.95, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Ripple Grid (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = length(s);
    let out = s / max(r, 0.0001);
    // rings spreading from the middle, weaker far out; the kick's own ring on top
    let wave = sin(r * ripples * 6.2831853 - g.time * speed * 3.0);
    let fall = exp(-r * 1.2);
    let ring = audio * g.kick * exp(-abs(r - (1.0 - g.kick) * 0.9) * 10.0);
    // they push the grid out and back
    let push = strength * 0.02 * (1.0 + audio * g.bass) * wave * fall + 0.05 * ring;
    // (nothing to push at the very middle, where "out" has no direction)
    let p = (s + out * push * smoothstep(0.0, 0.1, r)) * density;
    let aa = density / g.resolution.y;
    let d = min(abs(fract(p.x + 0.5) - 0.5), abs(fract(p.y + 0.5) - 0.5));
    let w = width * aa * 0.75;
    let line = 1.0 - smoothstep(w, w + aa * 1.5, d);
    let halo = exp(-d / (aa * 5.0)) * 0.18;
    // lit where a crest passes
    let crest = pow(max(wave, 0.0), 3.0) * fall * min(strength, 1.5) + ring * 1.5;
    var col = background.rgb + color_a.rgb * (line + halo) * (0.35 + 0.4 * fall);
    col += color_b.rgb * (line + halo) * crest * 1.6;
    return vec4f(col * smoothstep(1.3, 0.3, r), 1.0);
}
