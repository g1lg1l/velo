/*{
  "name": "Broken",
  "description": "What a Shader node runs until its code first compiles: its input, untouched",
  "inputs": [{ "id": "src", "type": "texture" }]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    return textureSample(src, smp, uv);
}
