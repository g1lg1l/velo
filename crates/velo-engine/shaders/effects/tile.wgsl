/*{
  "name": "Tile",
  "category": "effect",
  "description": "The picture repeated in a grid, mirrored tile to tile if you like, drifting, closing in on the kick with music up.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "tiles", "type": "int", "default": 3, "min": 1, "max": 8 },
    { "id": "mirror", "type": "bool", "default": true },
    { "id": "drift", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// drift: tiles a second sideways

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let n = f32(max(tiles, 1)) * (1.0 - 0.15 * music * g.kick);
    let p = (uv - 0.5) * n + 0.5 * n + vec2f(g.time * drift, 0.0);
    let cell = floor(p);
    var f = fract(p);
    if mirror {
        // every other tile flipped, so the edges meet
        let odd = abs(cell - 2.0 * floor(cell * 0.5));
        f = mix(f, 1.0 - f, odd);
    }
    return textureSampleLevel(src, smp, f, 0.0);
}
