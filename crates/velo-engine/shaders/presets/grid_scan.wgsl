/*{
  "name": "Grid Scan",
  "category": "generator",
  "description": "Looking down a room of grid lines while a plane of light sweeps to the far end and back; the kick sends another through, the bass brightens the lines",
  "params": [
    { "id": "speed", "type": "float", "default": 0.35, "min": 0.0, "max": 2.0 },
    { "id": "density", "type": "float", "default": 4.0, "min": 1.0, "max": 16.0 },
    { "id": "width", "type": "float", "default": 1.5, "min": 0.5, "max": 4.0 },
    { "id": "reach", "type": "float", "default": 10.0, "min": 3.0, "max": 30.0 },
    { "id": "glow", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "lines", "type": "color", "default": [0.45, 0.4, 0.95, 1.0] },
    { "id": "scan", "type": "color", "default": [1.0, 0.62, 0.99, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Grid Scan (its code isn't used: MIT + Commons Clause).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    // a corridor as wide and as high as the picture: 1 at its near end, growing with depth
    let room = vec2f(0.5 * g.aspect, 0.5);
    let m = abs(s) / room;
    let z = 1.0 / max(max(m.x, m.y), 0.0001);
    // on a side wall or on the floor or ceiling: where across it, and how wide it is
    let side = m.x > m.y;
    let across = select(s.x, s.y, side) * z;
    let far_side = select(room.y, room.x, side);
    // lines every 1 / density across and along; how much of that one pixel covers
    let cu = across * density;
    let cz = z * density * 0.5;
    let fu = density * z / g.resolution.y;
    let fz = density * 0.5 * z * z / (far_side * g.resolution.y);
    let l = min(abs(fract(cu + 0.5) - 0.5) / fu, abs(fract(cz + 0.5) - 0.5) / fz);
    // lines crowding into the distance turn into haze, then into the dark
    let line = (1.0 - smoothstep(0.5 * width, 0.5 * width + 1.0, l)) * (1.0 - smoothstep(0.25, 0.9, max(fu, fz)));
    let fog = exp(-z * 2.5 / reach);
    // the scan: a plane of light going to the far end and back, and the kick's own going out
    let at = 1.0 + (0.5 - 0.5 * cos(g.time * speed * 2.0)) * reach * 0.8;
    let sweep = exp(-abs(z - at) * 2.5);
    let pulse = audio * g.kick * exp(-abs(z - 1.0 - (1.0 - g.kick) * reach * 0.8) * 2.5);
    let light = (sweep + pulse) * glow;
    var col = background.rgb + lines.rgb * line * fog * (1.0 + 0.8 * audio * g.bass);
    col += scan.rgb * (line * 2.2 + 0.2) * light * fog;
    // the far end glows faintly
    col += lines.rgb * exp(-length(s) * 9.0) * 0.25;
    return vec4f(col, 1.0);
}
