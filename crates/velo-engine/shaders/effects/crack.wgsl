/*{
  "name": "Crack",
  "category": "effect",
  "description": "Cracked glass: straight cracks across the picture, drawn as thin lines, each shard of it shifted a little its own way, the glass breaking anew every bar; the kick jolts the shards.",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "cracks", "type": "int", "default": 7, "min": 1, "max": 15 },
    { "id": "shift", "type": "float", "default": 0.02, "min": 0.0, "max": 0.1 },
    { "id": "lines", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "music", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// After radiance's crack.wgsl (MIT, Zach Banks).

fn crack_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(43.17, 91.73))) * 17431.37);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // places measured in the picture's height, so the cracks are straight at any aspect
    let p = uv * vec2f(g.aspect, 1.0);
    let bar = floor(g.beats / 4.0);
    var push = vec2f(0.0);
    // how near the nearest crack is, in pixels
    var near = 1e6;
    for (var i = 0; i < cracks; i++) {
        let k = vec2f(f32(i), bar);
        let through = vec2f(crack_hash(k) * g.aspect, crack_hash(k + 3.1));
        let turn = crack_hash(k + 5.7) * 3.1415927;
        let normal = vec2f(-sin(turn), cos(turn));
        let side = dot(p - through, normal);
        // each side of a crack is pushed toward it: a shard is pushed by every crack round it
        push += normal * (step(side, 0.0) - 0.5);
        near = min(near, abs(side) * g.resolution.y);
    }
    // none at the picture's edges, which stay in their frame
    push *= 1.0 - 2.0 * abs(uv - 0.5);
    let c = textureSample(src, smp, uv - push / vec2f(g.aspect, 1.0) * shift * (1.0 + 2.0 * music * g.kick));
    let line = (1.0 - smoothstep(0.5, 1.5, near)) * lines;
    return vec4f(mix(c.rgb, vec3f(0.85) * c.a, line), c.a);
}
