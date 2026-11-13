/*{
  "name": "Attractor",
  "category": "effect",
  "description": "The picture's light flowing over a second picture, its map, up from the map's dark parts towards its bright, leaving trails that fade; with nothing in map, over itself; the kick pushes the flow further.",
  "inputs": [{ "id": "src", "type": "texture" }, { "id": "map", "type": "texture" }],
  "buffers": [{ "id": "trail" }],
  "params": [
    { "id": "flow", "type": "float", "default": 0.7, "min": 0.0, "max": 1.0 },
    { "id": "length", "type": "float", "default": 0.85, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's attractor.wgsl (MIT, Zach Banks).

// the map's brightness, its three channels added; the input's own while nothing's wired
fn attractor_light(at: vec2f) -> f32 {
    if textureDimensions(map).x > 1u {
        return dot(textureSampleLevel(map, smp, at, 0.0).rgb, vec3f(1.0));
    }
    return dot(textureSampleLevel(src, smp, at, 0.0).rgb, vec3f(1.0));
}

// the brighter of the input and the trail read from down the map's slope, faded
fn buffer_trail(uv: vec2f, px: vec2f) -> vec4f {
    let frames = g.delta * 60.0;
    let e = 0.01;
    let here = attractor_light(uv);
    let slope = clamp(0.8 * vec2f(attractor_light(uv + vec2f(e, 0.0)) - here, attractor_light(uv + vec2f(0.0, e)) - here), vec2f(-1.0), vec2f(1.0));
    let step = 0.05 * flow * (1.0 + 2.0 * music * g.kick) * frames;
    let moved = textureSampleLevel(trail, smp, uv - step * slope, 0.0);
    let fade = (0.01 + 0.2 * (1.0 - length)) * frames;
    return max(textureSampleLevel(src, smp, uv, 0.0), max(moved - fade, vec4f(0.0)));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let inp = textureSample(src, smp, uv);
    let t = textureSample(trail, smp, uv);
    // the trails over the input, premultiplied
    return t + inp * (1.0 - t.a);
}
