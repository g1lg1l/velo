/*{
  "name": "Tunnel",
  "category": "generator",
  "description": "A tunnel of rings of light rushing toward you, spiralling: it breathes with the bass and a flash races down it on the kick",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 4.0 },
    { "id": "rings", "type": "float", "default": 8.0, "min": 1.0, "max": 32.0 },
    { "id": "twist", "type": "float", "default": 1.0, "min": -4.0, "max": 4.0 },
    { "id": "tint", "type": "color", "default": [0.55, 0.35, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// uv runs 0..1 across the picture, px counts pixels. `g` holds time, aspect, bpm, beat_phase,
// bar_phase and the audio, 0..1: level, bass, mid, high, kick, snare, hat. Each param above is
// a variable by its id. For an effect, add "inputs": [{ "id": "src", "type": "texture" }] to
// the header and read it with textureSample(src, smp, uv).
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = max(length(p), 0.001);
    let around = atan2(p.y, p.x) / 6.2831853;
    // how far down the tunnel: the bass pulls the walls in
    let z = 0.25 / r * (1.0 - 0.15 * audio * g.bass);
    // rings coming toward us, spiralling by `twist` turns a ring
    let v = (z + g.time * speed) * rings * 0.25 + around * twist;
    // how much of v one pixel covers: rings closer together than that blur into a haze
    let pixel = rings * 0.0625 / (r * r * g.resolution.y);
    let d = abs(fract(v) - 0.5);
    let ring = (1.0 - smoothstep(0.05, 0.05 + max(pixel * 1.5, 0.03), d)) * (1.0 - smoothstep(0.25, 0.6, pixel));
    // their light spilling on the wall, and ribs running the tunnel's length
    let spill = pow(1.0 - 2.0 * d, 3.0) * 0.35;
    let ribs_at = abs(fract(around * 12.0 + 0.5 * twist * floor(v + 0.5)) - 0.5);
    let ribs = (1.0 - smoothstep(0.03, 0.03 + 3.0 / (r * g.resolution.y), ribs_at)) * 0.25;
    // waves of light travelling down it, and a flash racing out from the far end on the kick
    let wave = 0.55 + 0.45 * sin(z * 1.3 + g.time * speed * 3.0);
    let front = (1.0 - g.kick) * 5.0;
    let shock = audio * g.kick * exp(-pow((z - front) * 1.2, 2.0)) * 1.2;
    // the far end fades into the dark, where a glow beats with the bass
    let fog = exp(-z * 0.3);
    var col = tint.rgb * ((ring * 1.3 + spill + ribs) * wave * fog * (1.0 + 0.8 * audio * g.kick) + 0.06 * fog);
    col += mix(tint.rgb, vec3f(1.0), 0.5) * (ring + spill) * shock * fog;
    col += tint.rgb * exp(-r * 9.0) * (0.25 + 0.8 * audio * g.bass);
    return vec4f(col, 1.0);
}
