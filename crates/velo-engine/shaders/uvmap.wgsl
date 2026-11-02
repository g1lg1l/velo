/*{
  "name": "UV Map",
  "category": "effect",
  "description": "Each pixel shows the picture where its map says: the map's red is how far across, its green how far down; amount goes from the picture as it is to all the way there",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "params": [
    { "id": "outside", "type": "int", "default": 1, "names": ["Clamp", "Repeat", "Mirror"] },
    { "id": "scale", "type": "vec2", "default": [1.0, 1.0], "min": -4.0, "max": 4.0 },
    { "id": "offset", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 }
  ],
  "amount": "manual"
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let m = textureSample(map, smp, uv);
    var q = mix(uv, m.rg * scale + offset, clamp(amount, 0.0, 1.0));
    // past its edges: the edge, the picture again, or the picture mirrored
    if outside == 1 {
        q = fract(q);
    } else if outside == 2 {
        q = 1.0 - abs(fract(q * 0.5) * 2.0 - 1.0);
    }
    return textureSampleLevel(src, smp, q, 0.0);
}
