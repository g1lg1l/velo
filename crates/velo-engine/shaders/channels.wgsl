/*{
  "name": "Channels",
  "category": "effect",
  "description": "Each channel out from any channel in: swapped, one copied to all (a channel split), brightness as alpha, or set black or white",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "red", "type": "int", "default": 0, "names": ["Red", "Green", "Blue", "Alpha", "Brightness", "Black", "White"] },
    { "id": "green", "type": "int", "default": 1, "names": ["Red", "Green", "Blue", "Alpha", "Brightness", "Black", "White"] },
    { "id": "blue", "type": "int", "default": 2, "names": ["Red", "Green", "Blue", "Alpha", "Brightness", "Black", "White"] },
    { "id": "alpha", "type": "int", "default": 3, "names": ["Red", "Green", "Blue", "Alpha", "Brightness", "Black", "White"] }
  ]
}*/

// a channel of the color (not premultiplied), as the names above
fn pick(which: i32, c: vec4f) -> f32 {
    switch which {
        case 0: { return c.r; }
        case 1: { return c.g; }
        case 2: { return c.b; }
        case 3: { return c.a; }
        case 4: { return dot(c.rgb, vec3f(0.2126, 0.7152, 0.0722)); }
        case 5: { return 0.0; }
        default: { return 1.0; }
    }
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = textureSample(src, smp, uv);
    let c = vec4f(s.rgb / max(s.a, 0.0001), s.a);
    let a = clamp(pick(alpha, c), 0.0, 1.0);
    return vec4f(vec3f(pick(red, c), pick(green, c), pick(blue, c)) * a, a);
}
