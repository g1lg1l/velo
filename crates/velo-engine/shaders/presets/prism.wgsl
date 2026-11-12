/*{
  "name": "Prism",
  "category": "generator",
  "description": "A pyramid of glass drawn in light, turning, its edges split into the colors of the spectrum; the bass swells it, the kick splits the colors wider",
  "params": [
    { "id": "size", "type": "float", "default": 0.4, "min": 0.1, "max": 1.0 },
    { "id": "spin", "type": "float", "default": 0.1, "min": -1.0, "max": 1.0 },
    { "id": "lean", "type": "float", "default": 0.12, "min": -0.25, "max": 0.25 },
    { "id": "dispersion", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.3, "max": 4.0 },
    { "id": "glow", "type": "float", "default": 0.8, "min": 0.0, "max": 2.0 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Prism (its code isn't used: MIT + Commons Clause).

// how far p is from the stretch a to b
fn seg(p: vec2f, a: vec2f, b: vec2f) -> f32 {
    let pa = p - a;
    let ba = b - a;
    return length(pa - ba * clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0));
}

// a point of the pyramid (already turned about its axis) on the picture: tipped toward the
// eye by the lean (its cosine and sine in l), the far side smaller
fn project(v: vec3f, l: vec2f) -> vec2f {
    let y = v.y * l.x - v.z * l.y;
    let far = v.y * l.y + v.z * l.x;
    return vec2f(v.x, -y) * 2.6 / (2.6 + far);
}

// how far p is from the nearest of the pyramid's eight edges
fn pyramid(p: vec2f, turn: f32, s: f32, l: vec2f) -> f32 {
    let apex = project(vec3f(0.0, 0.8 * s, 0.0), l);
    // the base's corners are a quarter turn apart: the next is this one turned, at no cost
    var corner = vec2f(cos(0.7853982 - turn), sin(0.7853982 - turn)) * 0.85 * s;
    var prev = project(vec3f(corner.y, -0.5 * s, -corner.x), l);
    var d = 1e9;
    for (var i = 0; i < 4; i++) {
        let c = project(vec3f(corner.x, -0.5 * s, corner.y), l);
        d = min(d, min(seg(p, c, apex), seg(p, c, prev)));
        prev = c;
        corner = vec2f(-corner.y, corner.x);
    }
    return d;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let turn = g.time * spin * 6.2831853;
    let s = size * (1.0 + 0.12 * audio * g.bass);
    let l = vec2f(cos(lean * 6.2831853), sin(lean * 6.2831853));
    // red, green and blue each see it a little differently, as through glass
    let split = dispersion * (1.0 + 1.5 * audio * g.kick);
    let d = vec3f(pyramid(p, turn - 0.05 * split, s * (1.0 - 0.05 * split), l), pyramid(p, turn, s, l), pyramid(p, turn + 0.05 * split, s * (1.0 + 0.05 * split), l));
    let w = width * 0.0025;
    let aa = 1.0 / g.resolution.y;
    let core = 1.0 - smoothstep(vec3f(w), vec3f(w + aa), d);
    let halo = exp(-d / (w * 14.0)) * 0.55 * glow + exp(-d / (s * 0.5)) * 0.12 * glow;
    let flash = 1.0 + 0.8 * audio * g.kick;
    return vec4f(background.rgb + tint.rgb * (core + halo) * flash, 1.0);
}
