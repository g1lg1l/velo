/*{
  "name": "VHS",
  "category": "effect",
  "description": "Wandering tape, smeared chroma and noisy tracking lines react to the snare.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "wobble", "type": "float", "default": 3.0, "min": 0.0, "max": 20.0 },
    { "id": "bleed", "type": "float", "default": 5.0, "min": 0.0, "max": 24.0 },
    { "id": "noise", "type": "float", "default": 0.12, "min": 0.0, "max": 0.5 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn vhs_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(57.31, 23.89))) * 19341.19);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let pulse = 1.0 + music * g.snare;
    let tracking = pow(0.5 + 0.5 * sin(uv.y * 21.0 - g.time * 2.0), 16.0);
    let wave = sin(uv.y * 31.0 + g.time * 3.1) + tracking * sin(g.time * 19.0) * 2.0;
    let q = uv + vec2f(wave * wobble * pulse / g.resolution.x, 0.0);
    let center = textureSample(src, smp, q);
    let off = vec2f(bleed * pulse / g.resolution.x, 0.0);
    let smear = (textureSample(src, smp, q - off).rgb + textureSample(src, smp, q - off * 2.0).rgb) * 0.5;
    let weights = vec3f(0.299, 0.587, 0.114);
    let chroma = smear - vec3f(dot(smear, weights));
    let line = step(0.94, vhs_hash(vec2f(floor(px.y / 2.0), floor(g.time * 30.0))));
    let grain = vhs_hash(floor(px) + floor(g.time * 30.0)) - 0.5;
    let color = vec3f(dot(center.rgb, weights)) + chroma + grain * noise * pulse * (0.2 + 2.0 * line);
    return vec4f(max(color, vec3f(0.0)), original.a);
}
