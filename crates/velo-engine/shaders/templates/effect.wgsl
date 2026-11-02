/*{
  "name": "My Effect",
  "category": "effect",
  "description": "What's wired in, its colors turned round the hue and zoomed; turn audio up and it jumps on the kick. Change anything: it runs as you type",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "hue", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "zoom", "type": "float", "default": 1.0, "min": 0.5, "max": 4.0 },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// What's wired in is src: read it with textureSample(src, smp, uv) anywhere, as often as you
// like. uv runs 0..1 across the picture, px counts pixels; `g` holds time, aspect, bpm,
// beat_phase, bar_phase and the audio, 0..1: level, bass, mid, high, kick, snare, hat, melody.
fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let z = zoom * (1.0 + 0.1 * audio * g.kick);
    let c = textureSample(src, smp, (uv - 0.5) / z + 0.5);
    // a turn round the grey axis
    let k = vec3f(0.57735);
    let a = hue * 6.2831853;
    let rgb = c.rgb * cos(a) + cross(k, c.rgb) * sin(a) + k * dot(k, c.rgb) * (1.0 - cos(a));
    return vec4f(rgb, c.a);
}
