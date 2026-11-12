/*{
  "name": "Lightfall",
  "category": "generator",
  "description": "Streaks of light falling down the inside of a great curved wall, each with a bright head and a long tail; the kick stretches them into trails, the bass speeds the fall",
  "params": [
    { "id": "columns", "type": "float", "default": 40.0, "min": 8.0, "max": 160.0 },
    { "id": "speed", "type": "float", "default": 0.6, "min": 0.0, "max": 4.0 },
    { "id": "trail", "type": "float", "default": 0.35, "min": 0.05, "max": 1.5 },
    { "id": "curve", "type": "float", "default": 0.8, "min": 0.0, "max": 1.0 },
    { "id": "density", "type": "float", "default": 0.7, "min": 0.1, "max": 1.0 },
    { "id": "color_a", "type": "color", "default": [0.85, 0.9, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.35, 0.3, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Lightfall (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the wall curves away at both sides: columns crowd together and dim there
    let x = uv.x * 2.0 - 1.0;
    let round = mix(x, asin(clamp(x * 0.98, -1.0, 1.0)) / 1.5707963, curve);
    let facing = mix(1.0, sqrt(max(1.0 - x * x * 0.96, 0.0)), curve);
    let q = (round * 0.5 + 0.5) * columns;
    let id = floor(q);
    let f = abs(fract(q) - 0.5);
    let aa = columns / (g.resolution.x * max(facing, 0.2));
    var light = vec3f(0.0);
    // two streaks to a column, each at its own pace
    for (var i = 0; i < 2; i++) {
        let h = hash(vec2f(id, 1.0 + f32(i) * 7.0));
        let pace = 0.25 + 0.75 * hash(vec2f(id, 3.0 + f32(i) * 5.0));
        let tail = trail * (0.5 + pace) * (1.0 + 1.5 * audio * g.kick);
        // the head goes from above the top to below the bottom, then starts over
        let head = fract(h + (g.time * speed + 0.3 * audio * g.bass_time) * pace * 0.5) * (1.0 + tail * 1.5) - 0.1;
        let behind = head - uv.y;
        let streak = step(0.0, behind) * exp(-behind / tail * 3.0);
        let tip = exp(-abs(behind) * 60.0);
        let thin = 1.0 - smoothstep(0.1, 0.1 + aa + 0.25 * (1.0 - streak), f);
        let on = step(hash(vec2f(id, 9.0 + f32(i))), density);
        light += (mix(color_b.rgb, color_a.rgb, streak) * streak + vec3f(1.0) * tip * 0.8) * thin * on * (0.5 + 0.5 * pace);
    }
    // a faint light on the wall itself, so the curve reads
    let wall = color_b.rgb * 0.05 * facing * (1.0 - 0.6 * uv.y);
    return vec4f(background.rgb + wall + light * (0.3 + 0.7 * facing) * (1.0 + 0.4 * audio * g.kick), 1.0);
}
