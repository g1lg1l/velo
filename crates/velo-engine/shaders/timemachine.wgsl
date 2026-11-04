/*{
  "name": "Time Machine",
  "category": "effect",
  "description": "The picture as it was some frames ago (up to 31, kept at half size); with a map, each part as far back as the map is bright there: slit-scan, time smeared along a gradient or a shape",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "history": true,
  "params": [
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "frames", "type": "float", "default": 12.0, "min": 0.0, "max": 31.0 },
    { "id": "invert", "type": "bool", "default": false }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // how far back here: all of frames while nothing's in map, else as much as it's bright
    var k = 1.0;
    if textureDimensions(map).x > 1u {
        k = dot(textureSample(map, smp, uv).rgb, vec3f(0.2126, 0.7152, 0.0722));
    }
    if invert {
        k = 1.0 - k;
    }
    // the bass reaches further back
    return past_frame(uv, clamp(k, 0.0, 1.0) * frames * (1.0 + music * g.bass));
}
