/*{
  "name": "Edges",
  "category": "effect",
  "description": "Where the picture's brightness changes: lines in a color, alone (see-through elsewhere) or over the picture",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "width", "type": "float", "default": 1.0, "min": 0.5, "max": 6.0 },
    { "id": "threshold", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "strength", "type": "float", "default": 2.0, "min": 0.25, "max": 8.0 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "over", "type": "bool", "default": false }
  ]
}*/

fn luma(uv: vec2f) -> f32 {
    return dot(textureSampleLevel(src, smp, uv, 0.0).rgb, vec3f(0.2126, 0.7152, 0.0722));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let o = width / g.resolution;
    let tl = luma(uv + vec2f(-o.x, -o.y));
    let t = luma(uv + vec2f(0.0, -o.y));
    let tr = luma(uv + vec2f(o.x, -o.y));
    let l = luma(uv + vec2f(-o.x, 0.0));
    let r = luma(uv + vec2f(o.x, 0.0));
    let bl = luma(uv + vec2f(-o.x, o.y));
    let b = luma(uv + vec2f(0.0, o.y));
    let br = luma(uv + vec2f(o.x, o.y));
    // Sobel: the change across and down
    let gx = (tr + 2.0 * r + br) - (tl + 2.0 * l + bl);
    let gy = (bl + 2.0 * b + br) - (tl + 2.0 * t + tr);
    // the kick lights them up
    let e = smoothstep(threshold, threshold + 0.2, length(vec2f(gx, gy)) * strength * 0.25) * tint.a * (1.0 + music * g.kick);
    let line = vec4f(tint.rgb * e, min(e, 1.0));
    if over {
        let s = textureSampleLevel(src, smp, uv, 0.0);
        return vec4f(s.rgb + line.rgb, max(s.a, line.a));
    }
    return line;
}
