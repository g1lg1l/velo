/*{
  "name": "Hyperspace",
  "category": "generator",
  "description": "Stars stretched into streaks rushing past at warp speed, faster and longer on the kick",
  "params": [
    { "id": "speed", "type": "float", "default": 0.6, "min": 0.0, "max": 3.0 },
    { "id": "stretch", "type": "float", "default": 0.35, "min": 0.0, "max": 1.0 },
    { "id": "density", "type": "int", "default": 160, "min": 20, "max": 400 },
    { "id": "tint", "type": "color", "default": [0.6, 0.75, 1.0, 1.0] },
    { "id": "core", "type": "color", "default": [0.3, 0.4, 1.0, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = length(p);
    let around = atan2(p.y, p.x) / 6.2831853 + 0.5;
    let rush = 1.0 + 1.5 * audio * g.kick;
    var light = vec3f(0.0);
    // four layers of streaks, each on its own slices of the way round
    for (var layer = 0; layer < 4; layer++) {
        let l = f32(layer);
        let slices = f32(density) * (0.6 + 0.2 * l);
        let x = around * slices + l * 0.37;
        let slice = floor(x);
        let h = hash(vec2f(slice, l));
        // each star comes out from the middle, speeding up as it goes
        let z = fract(h * 7.3 + g.time * speed * (0.15 + 0.1 * h));
        let head = pow(z, 2.0) * 1.2;
        let tail = head * (1.0 - stretch * rush * (0.4 + 0.6 * z));
        let thin = smoothstep(0.5, 0.0, abs(fract(x) - 0.5) * (1.0 + 4.0 * r));
        let on = smoothstep(tail, head, r) * step(r, head) * step(0.55, fract(h * 13.1));
        light += mix(tint.rgb, vec3f(1.0), h) * on * thin * z * 1.6;
    }
    // a glow at the vanishing point, with the bass
    light += core.rgb * exp(-r * 6.0) * (0.25 + 0.5 * audio * g.bass);
    return vec4f(light, 1.0);
}
