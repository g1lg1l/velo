/*{
  "name": "Night Highway",
  "category": "generator",
  "description": "Driving a highway at night: headlights streaming past on one side, tail lights ahead on the other, a glow on the horizon; the bass speeds it up, the kick stretches the lights into warp trails",
  "params": [
    { "id": "speed", "type": "float", "default": 1.0, "min": 0.0, "max": 4.0 },
    { "id": "lanes", "type": "int", "default": 3, "min": 1, "max": 6 },
    { "id": "traffic", "type": "float", "default": 0.6, "min": 0.1, "max": 1.0 },
    { "id": "bend", "type": "float", "default": 0.4, "min": -2.0, "max": 2.0 },
    { "id": "horizon", "type": "float", "default": 0.05, "min": -0.3, "max": 0.4 },
    { "id": "oncoming", "type": "color", "default": [0.85, 0.9, 1.0, 1.0] },
    { "id": "ahead", "type": "color", "default": [1.0, 0.12, 0.1, 1.0] },
    { "id": "sky", "type": "color", "default": [0.1, 0.05, 0.3, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Hyperspeed (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // y up
    let s = vec2f((uv.x - 0.5) * g.aspect, 0.5 - uv.y);
    let down = horizon - s.y;
    // the sky: a glow where the road meets it, a few stars
    let star_cell = floor(s * 70.0);
    let star = step(0.975, hash(star_cell)) * (1.0 - smoothstep(0.0, 0.2, length(fract(s * 70.0) - 0.5))) * smoothstep(0.02, 0.3, -down);
    var col = sky.rgb * (0.25 + 1.2 * exp(-max(-down, 0.0) * 7.0)) + vec3f(star) * 0.6;
    if down > 0.0 {
        // the road: z grows toward the horizon, and the road winds off to one side far out
        let z = 0.25 / down;
        let x = s.x * z - bend * 0.02 * z * z * sin(g.time * 0.2);
        let fog = exp(-z * 0.09);
        let pixel = z / g.resolution.y;
        let warp = 1.0 + 2.0 * audio * g.kick;
        let run = g.time * speed + 0.5 * audio * g.bass_time;
        col = sky.rgb * 0.08;
        // its edges and its middle
        let edge = min(abs(abs(x) - 1.0), abs(x));
        col += sky.rgb * 1.2 * (1.0 - smoothstep(0.004, 0.004 + pixel * 2.0, edge)) * fog;
        // one side comes toward us, the other drives ahead, slower
        let side = select(-1.0, 1.0, x > 0.0);
        let n = f32(lanes);
        let across = (abs(x) - 0.08) / 0.84 * n;
        let lane = floor(across);
        if across >= 0.0 && lane < n {
            let f = abs(fract(across) - 0.5);
            let foot = n * pixel / 0.84;
            // the dashes between lanes, coming toward us with the road
            let dash = step(0.5, fract(z * 1.2 + run * 3.0)) * (1.0 - smoothstep(0.015, 0.015 + foot * 2.0, 0.5 - f)) * (1.0 - smoothstep(0.3, 0.8, foot));
            col += sky.rgb * 0.9 * dash * fog;
            // the road's length in stretches, some holding a car: its lights a streak, longer
            // the faster it goes
            let pace = select(4.0, 1.2, side > 0.0) * (0.8 + 0.4 * hash(vec2f(lane, side)));
            let along = z * 0.35 + run * pace * select(1.0, -1.0, side > 0.0) + hash(vec2f(lane, side + 5.0)) * 9.0;
            let h = hash(vec2f(floor(along), lane * 7.0 + side));
            let k = fract(along);
            let long = min((0.25 + 0.3 * h) * warp, 0.95);
            let streak = step(h, traffic) * smoothstep(0.0, 0.04, k) * (1.0 - smoothstep(long - 0.06, long, k));
            // two lights to a car, thin; widened, not broken up, where a pixel is wider than they
            let w = sqrt(0.0036 + foot * foot);
            let pair = exp(-pow((f - 0.2) / w, 2.0)) * 0.06 / w;
            let glow = exp(-abs(f - 0.2) * 9.0) * 0.12;
            col += select(oncoming.rgb, ahead.rgb, side > 0.0) * streak * (pair * 1.6 + glow) * fog * (1.0 + 0.5 * audio * g.kick);
        }
        col += sky.rgb * 1.2 * exp(-down * 7.0);
    }
    return vec4f(col, 1.0);
}
