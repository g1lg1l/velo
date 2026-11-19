
// Velo plays the list (media.rs, Reel): it counts the turns (a movie's lasting movie_every, a
// picture's every; movie_every at 0 is every; a movie shorter than its turn moves on once it
// has played through, or repeats, as short_movie says; with the clock on Hits a turn ends when
// hit next rises past a half, and a fade takes that share of every beats), picks each turn's file, loads it, streamed or
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
