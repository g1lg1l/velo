/*{
  "name": "Blend",
  "category": "composite",
  "description": "Up to sixteen pictures, each over the ones before (b over a, c over that...): fourteen modes, an intensity, and a key that lets their dark parts through",
  "inputs": [{ "id": "i1", "type": "texture" }, { "id": "i2", "type": "texture" }, { "id": "i3", "type": "texture" }, { "id": "i4", "type": "texture" }, { "id": "i5", "type": "texture" }, { "id": "i6", "type": "texture" }, { "id": "i7", "type": "texture" }, { "id": "i8", "type": "texture" }, { "id": "i9", "type": "texture" }, { "id": "i10", "type": "texture" }, { "id": "i11", "type": "texture" }, { "id": "i12", "type": "texture" }, { "id": "i13", "type": "texture" }, { "id": "i14", "type": "texture" }, { "id": "i15", "type": "texture" }, { "id": "i16", "type": "texture" }],
  "params": [
    { "id": "inputs", "type": "int", "default": 2, "min": 2, "max": 16 },
    { "id": "mode", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay", "Darken", "Soft Light", "Hard Light", "Color Dodge", "Color Burn", "Exclusion"] },
    { "id": "opacity", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "intensity", "type": "float", "default": 1.0, "min": 0.0, "max": 2.0 },
    { "id": "key", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "softness", "type": "float", "default": 0.1, "min": 0.0, "max": 0.5 }
  ]
}*/

fn luma(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

// top over base in the node's mode, the top as bright as the intensity says, as much as top
// covers (and its dark parts keyed out when the key is up): an unwired input is clear, so it
// changes nothing
fn over(base: vec3f, top: vec4f) -> vec3f {
    let t = max(top.rgb * intensity, vec3f(0.0));
    var c = t;
    switch mode {
        case 1: { c = base + t; }
        case 2: { c = 1.0 - (1.0 - base) * (1.0 - t); }
        case 3: { c = base * t; }
        case 4: { c = abs(base - t); }
        case 5: { c = max(base, t); }
        case 6: { c = max(base - t, vec3f(0.0)); }
        case 7: { c = select(1.0 - 2.0 * (1.0 - base) * (1.0 - t), 2.0 * base * t, base < vec3f(0.5)); }
        case 8: { c = min(base, t); }
        case 9: { c = (1.0 - 2.0 * t) * base * base + 2.0 * t * base; }
        case 10: { c = select(1.0 - 2.0 * (1.0 - base) * (1.0 - t), 2.0 * base * t, t < vec3f(0.5)); }
        case 11: { c = min(base / max(1.0 - t, vec3f(0.001)), vec3f(1.0)); }
        case 12: { c = max(1.0 - (1.0 - base) / max(t, vec3f(0.001)), vec3f(0.0)); }
        case 13: { c = base + t - 2.0 * base * t; }
        default: {}
    }
    let keyed = select(1.0, smoothstep(key, key + softness + 0.0001, luma(top.rgb)), key > 0.0);
    return mix(base, c, opacity * top.a * keyed);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let tops = array<vec4f, 15>(textureSample(i2, smp, uv), textureSample(i3, smp, uv), textureSample(i4, smp, uv), textureSample(i5, smp, uv), textureSample(i6, smp, uv), textureSample(i7, smp, uv), textureSample(i8, smp, uv), textureSample(i9, smp, uv), textureSample(i10, smp, uv), textureSample(i11, smp, uv), textureSample(i12, smp, uv), textureSample(i13, smp, uv), textureSample(i14, smp, uv), textureSample(i15, smp, uv), textureSample(i16, smp, uv));
    var c = textureSample(i1, smp, uv).rgb;
    for (var k = 0; k < 15; k++) {
        c = over(c, tops[k]);
    }
    return vec4f(c, 1.0);
}
