/*{
  "name": "Freeze",
  "category": "effect",
  "description": "Holds the picture: while hold is over a half (map Kick onto it for a stutter), or taking a new frame only every so many beats",
  "inputs": [{ "id": "src", "type": "texture" }],
  "feedback": true,
  "params": [
    { "id": "hold", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "every", "type": "float", "default": 0.0, "min": 0.0, "max": 16.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let live = textureSample(src, smp, uv);
    let last = textureSample(prev, smp, uv);
    var take = hold < 0.5;
    // every so many beats: only on the frame a step begins
    if every > 0.0 {
        let before = g.beats - g.delta * g.bpm / 60.0;
        take = take && floor(g.beats / every) != floor(before / every);
    }
    // nothing held yet: what comes in
    return select(last, live, take || (last.a == 0.0 && live.a > 0.0));
}
