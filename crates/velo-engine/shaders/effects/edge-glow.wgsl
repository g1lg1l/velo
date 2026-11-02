/*{
  "name": "Edge Glow",
  "category": "effect",
  "description": "Soft luminous Sobel outlines flare with the high frequencies.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "radius", "type": "float", "default": 2.0, "min": 1.0, "max": 8.0 },
    { "id": "strength", "type": "float", "default": 1.5, "min": 0.0, "max": 4.0 },
    { "id": "threshold", "type": "float", "default": 0.08, "min": 0.0, "max": 0.8 },
    { "id": "tint", "type": "color", "default": [0.25, 0.65, 1.0, 1.0] },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let original = textureSample(src, smp, uv);
    let d = radius / g.resolution;
    let weights = vec3f(0.2126, 0.7152, 0.0722);
    let tl = dot(textureSample(src, smp, uv + d * vec2f(-1.0, -1.0)).rgb, weights);
    let tc = dot(textureSample(src, smp, uv + d * vec2f(0.0, -1.0)).rgb, weights);
    let tr = dot(textureSample(src, smp, uv + d * vec2f(1.0, -1.0)).rgb, weights);
    let ml = dot(textureSample(src, smp, uv + d * vec2f(-1.0, 0.0)).rgb, weights);
    let mr = dot(textureSample(src, smp, uv + d * vec2f(1.0, 0.0)).rgb, weights);
    let bl = dot(textureSample(src, smp, uv + d * vec2f(-1.0, 1.0)).rgb, weights);
    let bc = dot(textureSample(src, smp, uv + d * vec2f(0.0, 1.0)).rgb, weights);
    let br = dot(textureSample(src, smp, uv + d * vec2f(1.0, 1.0)).rgb, weights);
    let gradient = vec2f(tr + 2.0 * mr + br - tl - 2.0 * ml - bl, bl + 2.0 * bc + br - tl - 2.0 * tc - tr) * 0.25;
    let edge = max(length(gradient) - threshold, 0.0);
    // A soft shoulder lights weak edges around the bright core.
    let glow = (sqrt(edge) * 0.6 + edge * 0.4) * strength * (1.0 + 0.5 * music * g.high);
    return vec4f(original.rgb + tint.rgb * glow, original.a);
}
