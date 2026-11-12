/*{
  "name": "Shape Grid",
  "category": "generator",
  "description": "A drifting grid of outlined squares, circles or triangles; cells fill with light and fade at random, and under a light that wanders over them; the kick lights a ring of them",
  "params": [
    { "id": "density", "type": "float", "default": 10.0, "min": 3.0, "max": 40.0 },
    { "id": "shape", "type": "int", "default": 0, "names": ["Squares", "Circles", "Triangles"] },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 3.0 },
    { "id": "angle", "type": "float", "default": 0.125, "min": 0.0, "max": 1.0 },
    { "id": "fill", "type": "float", "default": 0.25, "min": 0.0, "max": 1.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.5, "max": 4.0 },
    { "id": "color_a", "type": "color", "default": [0.35, 0.3, 0.65, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.5, 0.9, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.0, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Shape Grid (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    let drift = vec2f(cos(angle * 6.2831853), sin(angle * 6.2831853)) * t * 2.0;
    let q = s * density + drift;
    let cell = floor(q);
    let f = fract(q) - 0.5;
    // how far outside the cell's shape (inside: below zero)
    var d = max(abs(f.x), abs(f.y)) - 0.42;
    if shape == 1 {
        d = length(f) - 0.42;
    } else if shape == 2 {
        d = max(abs(f.x) * 0.8660254 + f.y * 0.5, -f.y) - 0.3;
    }
    let aa = density / g.resolution.y;
    let w = width * 0.5 * aa;
    let outline = 1.0 - smoothstep(w, w + aa, abs(d));
    let inside = 1.0 - smoothstep(-aa, aa, d);
    // each cell on its own clock: some of its turns it flashes on and fades
    let h = hash(cell);
    let clock = h * 17.0 + g.time * (0.15 + 0.25 * hash(cell + 5.0));
    let flash = step(1.0 - fill, hash(cell + 11.0 + floor(clock) * 13.0)) * exp(-fract(clock) * 3.0);
    // a light wandering over the grid, and the kick's ring moving out
    let at = (cell + 0.5 - drift) / density;
    let lamp = at - vec2f(0.4 * g.aspect * sin(t * 1.1), 0.3 * cos(t * 1.7));
    let ring = audio * g.kick * exp(-abs(length(at) - (1.0 - g.kick) * 0.9) * 12.0);
    let glint = audio * g.hat * step(0.95, hash(cell + floor(g.time * 10.0) * 7.0));
    let light = max(max(flash, exp(-dot(lamp, lamp) * 14.0)), max(ring, glint));
    var col = background.rgb + color_a.rgb * outline * (1.0 + 0.5 * audio * g.bass);
    col += color_b.rgb * light * (inside * 0.8 + outline);
    return vec4f(col, 1.0);
}
