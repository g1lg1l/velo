/*{
  "name": "Melt",
  "category": "effect",
  "description": "The picture's light runs down the screen and fades as it goes, the drips wavering a little on their way; they run further on the kick.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "buffers": [{ "id": "drip" }],
  "params": [
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "length", "type": "float", "default": 0.35, "min": 0.0, "max": 1.0 },
    { "id": "wobble", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's melt.wgsl (MIT, Zach Banks and Eric Van Albert).

// the brighter of the input and the last frame moved down a little and faded
fn buffer_drip(uv: vec2f, px: vec2f) -> vec4f {
    let frames = g.delta * 60.0;
    let fall = 0.006 * speed * (1.0 + 3.0 * music * g.kick) * frames;
    // a slow sway sideways, none near the top, where nothing is left above to read
    let sway = sin(uv.yx * 10.0 + sin(vec2f(g.beats * 0.5, g.beats * 0.75))) * 0.003 * wobble * smoothstep(0.0, 0.1, uv.y);
    let above = textureSampleLevel(drip, smp, uv - vec2f(0.0, fall) + sway * frames, 0.0);
    let fade = (0.002 + 0.03 * (1.0 - length)) * frames;
    return max(textureSampleLevel(src, smp, uv, 0.0), max(above - fade, vec4f(0.0)));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let inp = textureSample(src, smp, uv);
    let d = textureSample(drip, smp, uv);
    // the drips over the input, premultiplied
    return d + inp * (1.0 - d.a);
}
