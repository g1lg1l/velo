/*{
  "name": "Crystal Ball",
  "category": "generator",
  "description": "A glass ball with a crackling electric rim and glowing dust that swirls inside it, gathering where focus points",
  "params": [
    { "id": "size", "type": "float", "default": 0.3, "min": 0.05, "max": 0.6 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "glass", "type": "color", "default": [0.35, 0.55, 1.0, 1.0] },
    { "id": "refraction", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "rim", "type": "color", "default": [0.55, 0.85, 1.0, 1.0] },
    { "id": "glow", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "crackle", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 1.0, "min": 0.0, "max": 4.0 },
    { "id": "dust", "type": "float", "default": 0.6, "min": 0.0, "max": 1.0 },
    { "id": "dust_color", "type": "color", "default": [1.0, 0.75, 0.45, 1.0] },
    { "id": "swirl", "type": "float", "default": 0.4, "min": -2.0, "max": 2.0 },
    { "id": "focus", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "pull", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "background", "type": "color", "default": [0.02, 0.02, 0.05, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Crystalized Ball (its code isn't used: MIT + Commons Clause).
// Where that one follows the cursor, focus says where the dust gathers: map a signal onto it.

fn hash2(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2f(1.0, 0.0));
    let c = hash2(i + vec2f(0.0, 1.0));
    let d = hash2(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p: vec2f) -> f32 {
    var v = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        v += amp * noise(q);
        q = q * 2.03 + vec2f(17.0, 9.0);
        amp *= 0.5;
    }
    return v;
}

fn turn(a: f32) -> mat2x2f {
    return mat2x2f(cos(a), sin(a), -sin(a), cos(a));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let t = g.time * speed;
    // in the ball's radii from its middle (y down, as Shapes' position): 1 at the rim
    let q = (uv - 0.5 - position * 0.5) * vec2f(g.aspect, 1.0) / max(size, 0.01);
    let r = length(q);
    var col = background.rgb;
    if r < 1.0 {
        let z = sqrt(1.0 - r * r);
        // seen through the glass the middle is magnified
        let bent = q * mix(1.0, 0.6 + 0.4 * z, refraction);
        // the dust turns round the middle, faster near it, and gathers about focus
        let f = focus * 0.8;
        let d = turn(g.time * swirl * (1.6 - r)) * (bent - f) + f;
        let cloud = fbm(d * 3.0 + vec2f(0.0, t * 0.2));
        let grains = pow(noise(d * 40.0 + t), 12.0);
        let near = exp(-length(bent - f) * (1.0 + 4.0 * pull));
        col += dust_color.rgb * (smoothstep(0.45, 0.85, cloud) * 0.6 + grains * 2.0) * near * dust * (1.0 + audio * g.bass);
        // the glass: tinted, brighter toward its edge, a highlight up on the left
        col = mix(col, glass.rgb, 0.12 + 0.5 * pow(1.0 - z, 3.0));
        col += vec3f(pow(max(dot(vec3f(q, z), normalize(vec3f(-0.45, -0.55, 0.7))), 0.0), 60.0));
    }
    // the rim crackles: a thin bright edge pushed in and out along it, arcs reaching out where
    // the noise peaks (read round a circle, so it has no seam)
    let around = q / max(r, 1e-4);
    let edge = abs(r - 1.0 - (fbm(around * 2.5 + vec2f(t * 3.0, 0.0)) - 0.5) * 0.08 * crackle);
    var light = exp(-edge * 90.0) + 0.35 * exp(-edge * 18.0);
    light += pow(noise(around * 6.0 + vec2f(t * 6.0, t * 2.0)), 8.0) * crackle * exp(-max(r - 1.0, 0.0) * 12.0) * step(0.98, r) * 3.0;
    col += rim.rgb * light * glow * (1.0 + audio * g.kick);
    return vec4f(col, 1.0);
}
