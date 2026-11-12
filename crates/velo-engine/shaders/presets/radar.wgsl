/*{
  "name": "Radar",
  "category": "generator",
  "description": "A radar scope: the sweep goes round leaving a fading glow, and contacts light up as it passes over them; the kick pings a ring out, the hats add static",
  "params": [
    { "id": "speed", "type": "float", "default": 0.25, "min": -2.0, "max": 2.0 },
    { "id": "rings", "type": "int", "default": 4, "min": 1, "max": 10 },
    { "id": "contacts", "type": "int", "default": 8, "min": 0, "max": 24 },
    { "id": "afterglow", "type": "float", "default": 0.5, "min": 0.05, "max": 1.0 },
    { "id": "size", "type": "float", "default": 0.44, "min": 0.1, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.25, 1.0, 0.5, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.03, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Radar (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // in the scope's radii from its middle
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0) / size;
    let r = length(s);
    let aa = 1.0 / (g.resolution.y * size);
    let turns = g.time * speed;
    // how long ago the sweep passed here, in turns
    let ago = fract(turns - atan2(s.y, s.x) / 6.2831853);
    let fade = 4.0 / afterglow;
    let trail = exp(-ago * fade);
    // range rings, the cross, the rim
    let n = f32(rings);
    let ring = 1.0 - smoothstep(aa * n, aa * n * 2.5, abs(fract(r * n + 0.5) - 0.5));
    let cross = 1.0 - smoothstep(aa, aa * 2.5, min(abs(s.x), abs(s.y)));
    var light = (ring + cross) * 0.22 * (0.5 + trail) + trail * 0.3 + exp(-ago * 250.0) * 1.2;
    // contacts drifting slowly: each lights as the sweep crosses it, then fades
    for (var i = 0; i < contacts; i++) {
        let k = f32(i);
        let way = hash(vec2f(k, 2.0)) + g.time * 0.01 * (hash(vec2f(k, 3.0)) - 0.5);
        let at = (0.2 + 0.7 * hash(vec2f(k, 1.0))) * vec2f(cos(way * 6.2831853), sin(way * 6.2831853));
        let d = length(s - at);
        let seen = exp(-fract(turns - way) * fade * 0.7);
        light += seen * ((1.0 - smoothstep(0.012, 0.03, d)) * 2.0 + exp(-d * 30.0) * 0.6);
    }
    // the kick's ping
    light += audio * g.kick * exp(-abs(r - (1.0 - g.kick)) * 25.0);
    light += audio * g.hat * 0.1 * hash(px + floor(g.time * 30.0) * 17.0);
    let scope = 1.0 - smoothstep(1.0, 1.0 + aa * 2.0, r);
    let rim = 1.0 - smoothstep(aa * 1.5, aa * 4.0, abs(r - 1.0));
    return vec4f(background.rgb * scope + tint.rgb * (light * scope + rim * 0.8), 1.0);
}
