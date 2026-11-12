/*{
  "name": "Metaballs",
  "category": "generator",
  "description": "Soft blobs that drift, merge and split, swelling with the bass",
  "params": [
    { "id": "count", "type": "int", "default": 6, "min": 2, "max": 12 },
    { "id": "size", "type": "float", "default": 0.13, "min": 0.03, "max": 0.3 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "color_a", "type": "color", "default": [1.0, 0.3, 0.5, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.85, 0.4, 1.0] },
    { "id": "background", "type": "color", "default": [0.05, 0.02, 0.12, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    // sized by the narrower side, so a tall picture holds them too
    let r = size * min(g.aspect, 1.0) * (1.0 + 0.25 * audio * g.bass);
    // each ball adds r² / d²: where the sum passes 1 is the surface
    var field = 0.0;
    var grad = vec2f(0.0);
    for (var i = 0; i < count; i++) {
        let f = f32(i);
        let c = vec2f(sin(t * (0.5 + 0.13 * f) + f * 1.7) * 0.42 * g.aspect, cos(t * (0.4 + 0.11 * f) + f * 2.3) * 0.32);
        let d = p - c;
        let dd = dot(d, d) + 0.0001;
        field += r * r / dd;
        grad -= 2.0 * r * r * d / (dd * dd);
    }
    let inside = smoothstep(0.9, 1.1, field);
    var col = mix(color_a.rgb, color_b.rgb, smoothstep(1.0, 4.0, field));
    // rounded like drops: the field falls away outward, so its slope says which way the
    // surface leans; lit from the top left, with a highlight
    let dome = smoothstep(1.0, 3.5, field);
    let lean = -grad / (length(grad) + 0.0001) * (1.0 - dome);
    let n = normalize(vec3f(lean, 0.35 + dome));
    col *= 0.7 + 0.45 * max(dot(n, normalize(vec3f(-0.5, -0.6, 0.6))), 0.0);
    col += vec3f(pow(max(dot(n, normalize(vec3f(-0.35, -0.45, 0.82))), 0.0), 40.0)) * 0.7;
    col = mix(background.rgb, col, inside);
    // a halo around the surface, brighter on the kick and shimmering with the hats
    let rim = exp(-abs(field - 1.0) * 4.0) * (1.0 - inside * 0.7);
    col += color_a.rgb * rim * (0.35 + 0.8 * audio * g.kick) * (1.0 + 0.6 * audio * g.hat);
    return vec4f(col, 1.0);
}
