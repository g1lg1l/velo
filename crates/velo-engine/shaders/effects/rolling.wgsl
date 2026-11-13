/*{
  "name": "Rolling",
  "category": "effect",
  "description": "A rolling shutter: the picture is taken a line at a time by a scan running down it, speed times a beat, so what moves leans and tears where the scan passes; the kick takes the whole picture at once.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "held" }],
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.05, "max": 4.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's rolling.wgsl (MIT, Zach Banks).

// the picture as last taken: the lines the scan crossed since the last frame are taken anew
fn buffer_held(uv: vec2f, px: vec2f) -> vec4f {
    let now = g.beats * speed;
    let before = (g.beats - g.delta * g.bpm / 60.0) * speed;
    // how far down the scan has run since it last left this line
    let since = now - (floor(now - uv.y) + uv.y);
    let taken = f32(since < now - before);
    let fresh = max(taken, music * g.kick);
    return mix(textureSampleLevel(held, smp, uv, 0.0), textureSampleLevel(src, smp, uv, 0.0), fresh);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return textureSample(held, smp, uv);
}
