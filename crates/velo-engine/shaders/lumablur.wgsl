/*{
  "name": "Luma Blur",
  "category": "effect",
  "description": "Blurs the picture as much as a second one, its map, is bright there: a focus that moves, a blur that follows a shape; with nothing in map, everywhere",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "params": [
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "radius", "type": "float", "default": 12.0, "min": 0.0, "max": 64.0 },
    { "id": "invert", "type": "bool", "default": false }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // how much blur here: the map's brightness (all of it while nothing's wired), the kick more
    var k = 1.0;
    if textureDimensions(map).x > 1u {
        k = dot(textureSample(map, smp, uv).rgb, vec3f(0.2126, 0.7152, 0.0722));
    }
    if invert {
        k = 1.0 - k;
    }
    let r = clamp(k, 0.0, 1.0) * radius * (1.0 + 2.0 * music * g.kick);
    // Blur's sunflower of taps, 41 of them for the larger radius
    var sum = vec4f(0.0);
    for (var i = 0; i < 41; i++) {
        let t = (f32(i) + 0.5) / 41.0;
        let a = f32(i) * 2.39996;
        sum += textureSampleLevel(src, smp, uv + vec2f(cos(a), sin(a)) * sqrt(t) * r / g.resolution, 0.0);
    }
    return sum / 41.0;
}
