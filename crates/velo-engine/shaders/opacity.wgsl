/*{
  "name": "Opacity",
  "category": "effect",
  "description": "How much of what comes in shows: at 0 it's gone, over black on its own, over what's under it in a Blend",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "opacity", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 }
  ],
  "amount": "none"
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return textureSample(src, smp, uv) * clamp(opacity, 0.0, 1.0);
}
