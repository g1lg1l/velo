/*{
  "name": "Layer Stack",
  "category": "composite",
  "description": "Up to eight pictures stacked, each with its blend mode, opacity and order: the lowest order at the bottom, so a signal on a layer's order lifts it over the rest",
  "inputs": [{ "id": "l1", "type": "texture" }, { "id": "l2", "type": "texture" }, { "id": "l3", "type": "texture" }, { "id": "l4", "type": "texture" }, { "id": "l5", "type": "texture" }, { "id": "l6", "type": "texture" }, { "id": "l7", "type": "texture" }, { "id": "l8", "type": "texture" }],
  "params": [
    { "id": "mode_1", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_1", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_1", "type": "float", "default": 0.0000, "min": 0.0, "max": 1.0 },
    { "id": "mode_2", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_2", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_2", "type": "float", "default": 0.1429, "min": 0.0, "max": 1.0 },
    { "id": "mode_3", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_3", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_3", "type": "float", "default": 0.2857, "min": 0.0, "max": 1.0 },
    { "id": "mode_4", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_4", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_4", "type": "float", "default": 0.4286, "min": 0.0, "max": 1.0 },
    { "id": "mode_5", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_5", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_5", "type": "float", "default": 0.5714, "min": 0.0, "max": 1.0 },
    { "id": "mode_6", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_6", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_6", "type": "float", "default": 0.7143, "min": 0.0, "max": 1.0 },
    { "id": "mode_7", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_7", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_7", "type": "float", "default": 0.8571, "min": 0.0, "max": 1.0 },
    { "id": "mode_8", "type": "int", "default": 0, "names": ["Normal", "Add", "Screen", "Multiply", "Difference", "Lighten", "Subtract", "Overlay"] },
    { "id": "opacity_8", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "order_8", "type": "float", "default": 1.0000, "min": 0.0, "max": 1.0 }
  ]
}*/

// top over base, as Blend does it
fn blend_mode(base: vec3f, top: vec3f, mode: i32) -> vec3f {
    switch mode {
        case 1: { return base + top; }
        case 2: { return 1.0 - (1.0 - base) * (1.0 - top); }
        case 3: { return base * top; }
        case 4: { return abs(base - top); }
        case 5: { return max(base, top); }
        case 6: { return max(base - top, vec3f(0.0)); }
        case 7: { return select(1.0 - 2.0 * (1.0 - base) * (1.0 - top), 2.0 * base * top, base < vec3f(0.5)); }
        default: {}
    }
    return top;
}

// a layer over what's below: an unwired input is clear, so it changes nothing
fn over(base: vec3f, top: vec4f, mode: i32, opacity: f32) -> vec3f {
    return mix(base, blend_mode(base, top.rgb, mode), clamp(opacity * top.a, 0.0, 1.0));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    var tops = array<vec4f, 8>(textureSample(l1, smp, uv), textureSample(l2, smp, uv), textureSample(l3, smp, uv), textureSample(l4, smp, uv), textureSample(l5, smp, uv), textureSample(l6, smp, uv), textureSample(l7, smp, uv), textureSample(l8, smp, uv));
    var modes = array<i32, 8>(mode_1, mode_2, mode_3, mode_4, mode_5, mode_6, mode_7, mode_8);
    var opacities = array<f32, 8>(opacity_1, opacity_2, opacity_3, opacity_4, opacity_5, opacity_6, opacity_7, opacity_8);
    var orders = array<f32, 8>(order_1, order_2, order_3, order_4, order_5, order_6, order_7, order_8);
    // from the lowest order up, ties in input order: each time the lowest not drawn yet
    var drawn = array<bool, 8>(false, false, false, false, false, false, false, false);
    var c = vec3f(0.0);
    for (var n = 0; n < 8; n++) {
        var pick = 0;
        var lowest = 1e9;
        for (var k = 0; k < 8; k++) {
            if !drawn[k] && orders[k] < lowest {
                lowest = orders[k];
                pick = k;
            }
        }
        drawn[pick] = true;
        c = over(c, tops[pick], modes[pick], opacities[pick]);
    }
    return vec4f(c, 1.0);
}
