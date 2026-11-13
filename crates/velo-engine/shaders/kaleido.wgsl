/*{
  "name": "Kaleido",
  "category": "effect",
  "description": "Folds its input into mirrored wedges; with a map wired, its brightness says how far each part turns and zooms",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "params": [
    { "id": "segments", "type": "int", "default": 6, "min": 2, "max": 24 },
    { "id": "spin", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.25, "max": 4.0 },
    { "id": "angle", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "center", "type": "vec2", "default": [0.0, 0.0], "min": -0.5, "max": 0.5 },
    { "id": "modulate", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let c = 0.5 + center;
    let p = (uv - c) * vec2f(g.aspect, 1.0);
    let sector = 6.2831853 / f32(segments);
    // modulate: how far the map's brightness scales the turn and the zoom here (a see-through
    // map, or none wired, scales nothing)
    let m = textureSample(map, smp, uv);
    let k = mix(1.0, dot(m.rgb, vec3f(0.2126, 0.7152, 0.0722)), modulate * m.a);
    // the kick turns the wedges a little, the bass draws them in
    var turn = g.time * spin;
    var tilt = (angle + 0.03 * music * g.kick) * 6.2831853;
    var z = zoom * (1.0 + 0.2 * music * g.bass);
    if k != 1.0 {
        turn *= k;
        tilt *= k;
        z = pow(z, k);
    }
    var a = atan2(p.y, p.x) + turn + tilt;
    // fold every wedge onto the first one, mirrored
    a = abs(fract(a / sector + 0.5) - 0.5) * sector;
    let q = vec2f(cos(a), sin(a)) * length(p) / z / vec2f(g.aspect, 1.0) + c;
    return textureSample(src, smp, q);
}
