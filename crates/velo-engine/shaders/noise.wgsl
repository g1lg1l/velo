/*{
  "name": "Noise",
  "category": "generator",
  "description": "Fractal noise: value, gradient (Perlin), simplex, cells (Worley), cell edges or a mosaic; smooth, ridged or turbulent; warped, boiling or drifting, between two colors",
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "noise", "type": "int", "default": 0, "names": ["Value", "Gradient", "Simplex", "Cells", "Cell edges", "Mosaic"] },
    { "id": "scale", "type": "float", "default": 3.0, "min": 0.5, "max": 20.0 },
    { "id": "detail", "type": "int", "default": 4, "min": 1, "max": 8 },
    { "id": "roughness", "type": "float", "default": 0.5, "min": 0.2, "max": 0.9 },
    { "id": "lacunarity", "type": "float", "default": 2.03, "min": 1.2, "max": 4.0 },
    { "id": "style", "type": "int", "default": 0, "names": ["Smooth", "Ridged", "Turbulent"] },
    { "id": "contrast", "type": "float", "default": 1.0, "min": 0.2, "max": 4.0 },
    { "id": "brightness", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "steps", "type": "int", "default": 0, "min": 0, "max": 16 },
    { "id": "invert", "type": "bool", "default": false },
    { "id": "warp", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "speed", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "direction", "type": "float", "default": 0.0974, "min": 0.0, "max": 1.0 },
    { "id": "evolve", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "offset", "type": "vec2", "default": [0.0, 0.0], "min": -10.0, "max": 10.0 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "seed", "type": "float", "default": 0.0, "min": 0.0, "max": 100.0 },
    { "id": "color_a", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] }
  ]
}*/

// two random numbers 0..1 for each whole cell, of the seed's own pattern (an integer hash:
// sin() of large numbers loses its precision on the GPU and draws blocks)
fn hash22(p: vec2f) -> vec2f {
    var v = bitcast<vec2u>(vec2i(floor(p))) * 1664525u + 1013904223u + u32(seed * 1000.0);
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v ^= v >> vec2u(16u);
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v ^= v >> vec2u(16u);
    return vec2f(v) / 4294967296.0;
}

fn hash2(p: vec2f) -> f32 {
    return hash22(p).x;
}

fn value_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2f(1.0, 0.0));
    let c = hash2(i + vec2f(0.0, 1.0));
    let d = hash2(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Perlin's: a random slope at each corner, eased between them
fn gradient_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let ga = hash2(i) * 6.2831853;
    let gb = hash2(i + vec2f(1.0, 0.0)) * 6.2831853;
    let gc = hash2(i + vec2f(0.0, 1.0)) * 6.2831853;
    let gd = hash2(i + vec2f(1.0, 1.0)) * 6.2831853;
    let a = dot(vec2f(cos(ga), sin(ga)), f);
    let b = dot(vec2f(cos(gb), sin(gb)), f - vec2f(1.0, 0.0));
    let c = dot(vec2f(cos(gc), sin(gc)), f - vec2f(0.0, 1.0));
    let d = dot(vec2f(cos(gd), sin(gd)), f - vec2f(1.0, 1.0));
    return 0.5 + 0.7 * mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// on a grid of triangles: three corners a point, not four
fn simplex_noise(p: vec2f) -> f32 {
    let k1 = 0.366025404;
    let k2 = 0.211324865;
    let i = floor(p + (p.x + p.y) * k1);
    let a = p - i + (i.x + i.y) * k2;
    let o = select(vec2f(0.0, 1.0), vec2f(1.0, 0.0), a.x > a.y);
    let b = a - o + k2;
    let c = a - 1.0 + 2.0 * k2;
    let h = max(0.5 - vec3f(dot(a, a), dot(b, b), dot(c, c)), vec3f(0.0));
    let ga = hash22(i) * 2.0 - 1.0;
    let gb = hash22(i + o) * 2.0 - 1.0;
    let gc = hash22(i + 1.0) * 2.0 - 1.0;
    let n = h * h * h * h * vec3f(dot(a, ga), dot(b, gb), dot(c, gc));
    return 0.5 + 35.0 * (n.x + n.y + n.z);
}

// Worley's: the nearest of a point per cell, the next nearest, and which cell
fn cells(p: vec2f) -> vec3f {
    let i = floor(p);
    let f = fract(p);
    var near = vec3f(8.0, 8.0, 0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2f(f32(x), f32(y));
            let d = length(o + hash22(i + o) - f);
            if d < near.x {
                near = vec3f(d, near.x, hash2(i + o));
            } else if d < near.y {
                near.y = d;
            }
        }
    }
    return near;
}

fn basis(p: vec2f) -> f32 {
    switch noise {
        case 1: { return gradient_noise(p); }
        case 2: { return simplex_noise(p); }
        case 3: { return min(cells(p).x, 1.0); }
        case 4: {
            let c = cells(p);
            return min((c.y - c.x) * 2.0, 1.0);
        }
        case 5: { return cells(p).z; }
        default: { return value_noise(p); }
    }
}

// octaves of noise: as they are, folded into ridges, or folded into turbulence; each octave
// drifts round its own circle as it evolves, so the noise boils where it is
fn fbm(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    var total = 0.0;
    for (var i = 0; i < detail; i++) {
        let t = g.time * evolve * (0.3 + 0.2 * f32(i));
        var n = clamp(basis(p + vec2f(cos(t + f32(i)), sin(t + f32(i) * 1.3)) * select(0.0, 1.0, evolve > 0.0)), 0.0, 1.0);
        if style == 1 {
            n = 1.0 - abs(n * 2.0 - 1.0);
        } else if style == 2 {
            n = abs(n * 2.0 - 1.0);
        }
        sum += amp * n;
        total += amp;
        p = p * lacunarity + vec2f(1.7, 9.2);
        amp *= roughness;
    }
    // the same range whatever the roughness: as the octaves at half each always summed
    return sum / total * (1.0 - pow(0.5, f32(detail)));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let a = direction * 6.2831853;
    let r = rotate * 3.14159265;
    let q = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // audio: the kick punches in, the bass lifts it
    var p = (mat2x2f(cos(r), sin(r), -sin(r), cos(r)) * q) * scale * (1.0 - 0.08 * audio * g.kick) + offset + vec2f(cos(a), sin(a)) * 1.2207 * g.time * speed;
    // the noise bending itself
    if warp > 0.0 {
        p += warp * (vec2f(fbm(p + vec2f(5.2, 1.3)), fbm(p + vec2f(1.7, 9.2))) - 0.5) * 2.0;
    }
    var v = clamp(pow(fbm(p), contrast) + brightness + 0.3 * audio * g.bass, 0.0, 1.0);
    if steps > 1 {
        v = floor(v * f32(steps) * 0.9999) / f32(steps - 1);
    }
    if invert {
        v = 1.0 - v;
    }
    let c = mix(color_a, color_b, v);
    return vec4f(c.rgb * c.a, c.a);
}
