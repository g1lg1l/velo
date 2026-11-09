/*{
  "name": "Media List",
  "category": "generator",
  "description": "Pictures and movies in turn, streamed from disk or held in memory: in order, at random or back and forth, a new one every so many beats or seconds (movies for as long as their own), cut or crossfaded",
  "inputs": [{ "id": "s1", "type": "texture" }, { "id": "s2", "type": "texture" }, { "id": "s3", "type": "texture" }],
  "params": [
    { "id": "order", "type": "int", "default": 0, "names": ["In order", "At random", "Back and forth"] },
    { "id": "clock", "type": "int", "default": 0, "names": ["Beats", "Seconds"] },
    { "id": "every", "type": "float", "default": 4.0, "min": 0.25, "max": 64.0 },
    { "id": "movie_every", "type": "float", "default": 0.0, "min": 0.0, "max": 64.0 },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "load", "type": "int", "default": 0, "names": ["Streamed", "In memory"] },
    { "id": "_now", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "_fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// Velo plays the list (media.rs, Reel): it counts the turns (a movie's lasting movie_every, a
// picture's every; movie_every at 0 is every), picks each turn's file, loads it, streamed or
// all in memory (load), into slot k % 3 for turn k, an Image node of its own, with the turn
// before and the next on the other two, and tells this which slot shows (_now) and how much of
// the one before still does (_fade, crossfading out). A slot still loading is black.

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
    let now = i32(round(_now));
    let shown = slot(now, uv);
    if _fade > 0.0 {
        // the one before: two slots on
        return mix(shown, slot(now + 2, uv), _fade);
    }
    return shown;
}
