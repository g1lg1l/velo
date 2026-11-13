/*{
  "name": "Transform",
  "category": "effect",
  "description": "Zoom, rotate, offset, mirror or tile its input; with a map wired, its brightness says how much each part moves",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "params": [
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.1, "max": 8.0 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "offset", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "mirror", "type": "bool", "default": false },
    { "id": "tile", "type": "bool", "default": false },
    { "id": "spin", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "scroll", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "modulate", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // modulate: how far the map's brightness scales the move here; at 1 a white part moves all the
    // way and a black one stays put (a see-through map, or none wired, scales nothing)
    let m = textureSample(map, smp, uv);
    let k = mix(1.0, dot(m.rgb, vec3f(0.2126, 0.7152, 0.0722)), modulate * m.a);
    // rotate: half-turns either way; spin: turns a second; scroll: screens a second
    var a = rotate * 3.14159265 + g.time * spin * 6.2831853;
    // the kick punches in
    var z = zoom * (1.0 + 0.15 * music * g.kick);
    var shift = offset;
    var slide = scroll;
    if k != 1.0 {
        a *= k;
        z = pow(z, k);
        shift *= k;
        slide *= k;
    }
    let p = mat2x2f(cos(a), sin(a), -sin(a), cos(a)) * ((uv - 0.5) * vec2f(g.aspect, 1.0)) / z - shift - slide * g.time;
    var q = p / vec2f(g.aspect, 1.0) + 0.5;
    if mirror {
        q = 1.0 - abs(fract(q * 0.5) * 2.0 - 1.0);
    }
    if tile {
        q = fract(q);
    }
    return textureSample(src, smp, q);
}
