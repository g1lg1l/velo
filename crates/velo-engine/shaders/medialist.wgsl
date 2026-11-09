/*{
  "name": "Media List",
  "category": "generator",
  "description": "Pictures and movies in turn, streamed from disk: in order, at random or back and forth, a new one every so many beats or seconds, cut or crossfaded",
  "inputs": [{ "id": "s1", "type": "texture" }, { "id": "s2", "type": "texture" }, { "id": "s3", "type": "texture" }],
  "params": [
    { "id": "order", "type": "int", "default": 0, "names": ["In order", "At random", "Back and forth"] },
    { "id": "clock", "type": "int", "default": 0, "names": ["Beats", "Seconds"] },
    { "id": "every", "type": "float", "default": 4.0, "min": 0.25, "max": 64.0 },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// Velo streams the files (media.rs, Reel): turn k's file comes in on slot k % 3, an Image node
// of its own, with the turn before and the next on the other two. Which file a turn shows (the
// order) is picked there; here the turns cut or crossfade. A slot still loading is black.

// sampled at one level: which slot shows isn't uniform control flow
fn slot(k: i32, uv: vec2f) -> vec4f {
    switch k % 3 {
        case 0: {
            return textureSampleLevel(s1, smp, uv, 0.0);
        }
        case 1: {
            return textureSampleLevel(s2, smp, uv, 0.0);
        }
        default: {
            return textureSampleLevel(s3, smp, uv, 0.0);
        }
    }
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let t = select(g.beats, g.time, clock == 1) / max(every, 0.01);
    // a clock moved back (a resync, a tap) doesn't count below the first
    let step = max(i32(floor(t)), 0);
    let now = slot(step, uv);
    // the first part of each turn crossfades from the one before
    let k = fract(t);
    if fade > 0.0 && k < fade && step > 0 {
        return mix(slot(step - 1, uv), now, smoothstep(0.0, 1.0, k / fade));
    }
    return now;
}
