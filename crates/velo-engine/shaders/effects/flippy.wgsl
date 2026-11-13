/*{
  "name": "Flippy",
  "category": "effect",
  "description": "The picture cut in square tiles, each flipped over along its diagonal, the grid drifting slowly; on the kick they flip over the other diagonal.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "tiles", "type": "float", "default": 12.0, "min": 2.0, "max": 50.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's flippy.wgsl (MIT, Zach Banks).

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    // `tiles` square tiles down the screen, the grid going a tile diagonally every 64 beats
    let drift = vec2f(fract(g.beats / 64.0));
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * tiles - drift;
    let f = fract(p);
    // over the diagonal, or the other one on the kick
    let flipped = select(1.0 - f.yx, f.yx, music * g.kick > 0.5);
    let q = (floor(p) + flipped + drift) / tiles;
    let color = textureSampleLevel(src, smp, q / vec2f(g.aspect, 1.0) + 0.5, 0.0);
    return vec4f(color.rgb, original.a);
}
