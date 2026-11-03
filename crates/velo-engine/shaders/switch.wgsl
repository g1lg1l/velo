/*{
  "name": "Switch",
  "category": "composite",
  "description": "One of its inputs, as value picks: 0 the first, 1 the last of count, the nearest in between. Fade turns the cut into a transition, its style a crossfade, a dissolve, a wipe or a circle. Map a Counter, an LFO or Time onto value to swap views",
  "inputs": [{ "id": "i1", "type": "texture" }, { "id": "i2", "type": "texture" }, { "id": "i3", "type": "texture" }, { "id": "i4", "type": "texture" }, { "id": "i5", "type": "texture" }, { "id": "i6", "type": "texture" }, { "id": "i7", "type": "texture" }, { "id": "i8", "type": "texture" }, { "id": "i9", "type": "texture" }, { "id": "i10", "type": "texture" }, { "id": "i11", "type": "texture" }, { "id": "i12", "type": "texture" }, { "id": "i13", "type": "texture" }, { "id": "i14", "type": "texture" }, { "id": "i15", "type": "texture" }, { "id": "i16", "type": "texture" }],
  "params": [
    { "id": "value", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "count", "type": "int", "default": 2, "min": 2, "max": 16 },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "style", "type": "int", "default": 0, "names": ["Crossfade", "Dissolve", "Wipe", "Circle"] }
  ]
}*/

// a transition's edge, in its pattern's 0..1
const EDGE: f32 = 0.04;

// input n (0 the first), sampled at one level: which one isn't uniform control flow
fn pick(n: i32, uv: vec2f) -> vec4f {
    var c = vec4f(0.0);
    if n == 0 {
        c = textureSampleLevel(i1, smp, uv, 0.0);
    } else if n == 1 {
        c = textureSampleLevel(i2, smp, uv, 0.0);
    } else if n == 2 {
        c = textureSampleLevel(i3, smp, uv, 0.0);
    } else if n == 3 {
        c = textureSampleLevel(i4, smp, uv, 0.0);
    } else if n == 4 {
        c = textureSampleLevel(i5, smp, uv, 0.0);
    } else if n == 5 {
        c = textureSampleLevel(i6, smp, uv, 0.0);
    } else if n == 6 {
        c = textureSampleLevel(i7, smp, uv, 0.0);
    } else if n == 7 {
        c = textureSampleLevel(i8, smp, uv, 0.0);
    } else if n == 8 {
        c = textureSampleLevel(i9, smp, uv, 0.0);
    } else if n == 9 {
        c = textureSampleLevel(i10, smp, uv, 0.0);
    } else if n == 10 {
        c = textureSampleLevel(i11, smp, uv, 0.0);
    } else if n == 11 {
        c = textureSampleLevel(i12, smp, uv, 0.0);
    } else if n == 12 {
        c = textureSampleLevel(i13, smp, uv, 0.0);
    } else if n == 13 {
        c = textureSampleLevel(i14, smp, uv, 0.0);
    } else if n == 14 {
        c = textureSampleLevel(i15, smp, uv, 0.0);
    } else if n == 15 {
        c = textureSampleLevel(i16, smp, uv, 0.0);
    }
    return c;
}

fn hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // where value sits along the inputs: between `first` and the next, `t` of the way
    let x = clamp(value, 0.0, 1.0) * f32(count - 1);
    let first = min(i32(floor(x)), count - 2);
    let t = x - f32(first);
    // a cut halfway, or a transition `fade` wide around it
    let k = select(step(0.5, t), smoothstep(0.5 - fade * 0.5, 0.5 + fade * 0.5, t), fade > 0.0);
    // when each pixel turns as k goes from 0 to 1: all at once, grains of two pixels each at its
    // own time, left to right, or out from the middle
    var at = 0.0;
    if style == 1 {
        at = hash(floor(px / 2.0));
    } else if style == 2 {
        at = uv.x;
    } else if style == 3 {
        at = length((uv - 0.5) * vec2f(g.aspect, 1.0)) / length(vec2f(g.aspect, 1.0) * 0.5);
    }
    let m = select(k, smoothstep(at, at + EDGE, k * (1.0 + EDGE)), style != 0);
    return mix(pick(first, uv), pick(first + 1, uv), m);
}
