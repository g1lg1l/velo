/*{
  "name": "Merge",
  "category": "composite",
  "description": "A picture made of up to four: its red from the first one's brightness, its green from the second's, its blue from the third's, its alpha from the fourth's",
  "inputs": [{ "id": "red", "type": "texture" }, { "id": "green", "type": "texture" }, { "id": "blue", "type": "texture" }, { "id": "alpha", "type": "texture" }],
  "params": [
    { "id": "source", "type": "int", "default": 0, "names": ["Brightness", "Own channel"] },
    { "id": "opaque", "type": "bool", "default": true }
  ]
}*/

// what an input gives: its brightness, or the channel it's for (`k`: 0 red to 3 alpha)
fn part(c: vec4f, k: i32) -> f32 {
    if source == 0 {
        return dot(c.rgb, vec3f(0.2126, 0.7152, 0.0722));
    }
    switch k {
        case 0: { return c.r; }
        case 1: { return c.g; }
        case 2: { return c.b; }
        default: { return c.a; }
    }
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let rgb = vec3f(part(textureSample(red, smp, uv), 0), part(textureSample(green, smp, uv), 1), part(textureSample(blue, smp, uv), 2));
    // opaque: the fourth input left out (unwired it's black, which would hide it all)
    let a = select(clamp(part(textureSample(alpha, smp, uv), 3), 0.0, 1.0), 1.0, opaque);
    return vec4f(rgb * a, a);
}
