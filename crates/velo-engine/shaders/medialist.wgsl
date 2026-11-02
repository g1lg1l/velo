/*{
  "name": "Media List",
  "category": "generator",
  "description": "Up to eight pictures and movies in turn: in order, at random or back and forth, a new one every so many beats or seconds, cut or crossfaded",
  "inputs": [{ "id": "c1", "type": "texture" }, { "id": "c2", "type": "texture" }, { "id": "c3", "type": "texture" }, { "id": "c4", "type": "texture" }, { "id": "c5", "type": "texture" }, { "id": "c6", "type": "texture" }, { "id": "c7", "type": "texture" }, { "id": "c8", "type": "texture" }],
  "params": [
    { "id": "order", "type": "int", "default": 0, "names": ["In order", "At random", "Back and forth"] },
    { "id": "clock", "type": "int", "default": 0, "names": ["Beats", "Seconds"] },
    { "id": "every", "type": "float", "default": 4.0, "min": 0.25, "max": 64.0 },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// The files come in as c1..c8, in the list's order (Velo makes an Image node of each); an input
// with nothing is a 1×1 black.

fn count() -> i32 {
    var n = 0;
    if textureDimensions(c1).x > 1u {
        n = 1;
    }
    if textureDimensions(c2).x > 1u {
        n = 2;
    }
    if textureDimensions(c3).x > 1u {
        n = 3;
    }
    if textureDimensions(c4).x > 1u {
        n = 4;
    }
    if textureDimensions(c5).x > 1u {
        n = 5;
    }
    if textureDimensions(c6).x > 1u {
        n = 6;
    }
    if textureDimensions(c7).x > 1u {
        n = 7;
    }
    if textureDimensions(c8).x > 1u {
        n = 8;
    }
    return n;
}

// sampled at one level: which clip shows isn't uniform control flow
fn clip(k: i32, uv: vec2f) -> vec4f {
    switch k {
        case 0: {
            return textureSampleLevel(c1, smp, uv, 0.0);
        }
        case 1: {
            return textureSampleLevel(c2, smp, uv, 0.0);
        }
        case 2: {
            return textureSampleLevel(c3, smp, uv, 0.0);
        }
        case 3: {
            return textureSampleLevel(c4, smp, uv, 0.0);
        }
        case 4: {
            return textureSampleLevel(c5, smp, uv, 0.0);
        }
        case 5: {
            return textureSampleLevel(c6, smp, uv, 0.0);
        }
        case 6: {
            return textureSampleLevel(c7, smp, uv, 0.0);
        }
        default: {
            return textureSampleLevel(c8, smp, uv, 0.0);
        }
    }
}

fn hash(x: f32) -> f32 {
    return fract(sin(x * 12.9898 + 78.233) * 43758.5453);
}

// the clip a step of the clock shows
fn pick(step: i32, n: i32) -> i32 {
    if n <= 1 {
        return 0;
    }
    if order == 1 {
        // at random, not the one just before
        let k = i32(hash(f32(step)) * f32(n)) % n;
        let before = i32(hash(f32(step - 1)) * f32(n)) % n;
        return select(k, (k + 1) % n, k == before && step > 0);
    }
    if order == 2 {
        let period = 2 * n - 2;
        let s = step % period;
        return select(s, period - s, s >= n);
    }
    return step % n;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let n = count();
    if n == 0 {
        return vec4f(0.0);
    }
    let t = select(g.beats, g.time, clock == 1) / max(every, 0.01);
    // a clock moved back (a resync, a tap) doesn't count below the first
    let step = max(i32(floor(t)), 0);
    let now = clip(pick(step, n), uv);
    // the first part of each turn crossfades from the one before
    let k = fract(t);
    if fade > 0.0 && k < fade && step > 0 {
        return mix(clip(pick(step - 1, n), uv), now, smoothstep(0.0, 1.0, k / fade));
    }
    return now;
}
