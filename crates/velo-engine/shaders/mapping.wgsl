/*{
  "name": "Mapping",
  "category": "composite",
  "description": "Up to eight pictures, each laid on a surface of four corners placed by hand, as the projector sees them: the faces of a box, panels, a wall's windows. Input n fills surface n; a surface is there once something is wired into its input. Outlines show the surfaces' edges and corners, to line them up on the real thing",
  "inputs": [{ "id": "i1", "type": "texture" }, { "id": "i2", "type": "texture" }, { "id": "i3", "type": "texture" }, { "id": "i4", "type": "texture" }, { "id": "i5", "type": "texture" }, { "id": "i6", "type": "texture" }, { "id": "i7", "type": "texture" }, { "id": "i8", "type": "texture" }],
  "params": [
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "outlines", "type": "bool", "default": false },
    { "id": "top_1", "type": "vec4", "default": [0.02, 0.04, 0.23, 0.04] },
    { "id": "bottom_1", "type": "vec4", "default": [0.02, 0.46, 0.23, 0.46] },
    { "id": "top_2", "type": "vec4", "default": [0.27, 0.04, 0.48, 0.04] },
    { "id": "bottom_2", "type": "vec4", "default": [0.27, 0.46, 0.48, 0.46] },
    { "id": "top_3", "type": "vec4", "default": [0.52, 0.04, 0.73, 0.04] },
    { "id": "bottom_3", "type": "vec4", "default": [0.52, 0.46, 0.73, 0.46] },
    { "id": "top_4", "type": "vec4", "default": [0.77, 0.04, 0.98, 0.04] },
    { "id": "bottom_4", "type": "vec4", "default": [0.77, 0.46, 0.98, 0.46] },
    { "id": "top_5", "type": "vec4", "default": [0.02, 0.54, 0.23, 0.54] },
    { "id": "bottom_5", "type": "vec4", "default": [0.02, 0.96, 0.23, 0.96] },
    { "id": "top_6", "type": "vec4", "default": [0.27, 0.54, 0.48, 0.54] },
    { "id": "bottom_6", "type": "vec4", "default": [0.27, 0.96, 0.48, 0.96] },
    { "id": "top_7", "type": "vec4", "default": [0.52, 0.54, 0.73, 0.54] },
    { "id": "bottom_7", "type": "vec4", "default": [0.52, 0.96, 0.73, 0.96] },
    { "id": "top_8", "type": "vec4", "default": [0.77, 0.54, 0.98, 0.54] },
    { "id": "bottom_8", "type": "vec4", "default": [0.77, 0.96, 0.98, 0.96] }
  ]
}*/

// the surfaces' corners, in the parameter buffer after the two parameters above, in 0..1 of the
// picture: top-left and top-right (top_n), then bottom-left and bottom-right (bottom_n)
const FIRST_SURFACE: i32 = 2;

// an input with nothing wired reads a 1×1 black: its surface isn't there
fn wired(input: i32) -> bool {
    var d = vec2u(0u);
    if input == 1 {
        d = textureDimensions(i1);
    } else if input == 2 {
        d = textureDimensions(i2);
    } else if input == 3 {
        d = textureDimensions(i3);
    } else if input == 4 {
        d = textureDimensions(i4);
    } else if input == 5 {
        d = textureDimensions(i5);
    } else if input == 6 {
        d = textureDimensions(i6);
    } else if input == 7 {
        d = textureDimensions(i7);
    } else if input == 8 {
        d = textureDimensions(i8);
    }
    return d.x > 1u;
}

// one of the inputs (sampled at one level: surfaces aren't uniform control flow)
fn pick(input: i32, q: vec2f) -> vec4f {
    var c = vec4f(0.0);
    if input == 1 {
        c = textureSampleLevel(i1, smp, q, 0.0);
    } else if input == 2 {
        c = textureSampleLevel(i2, smp, q, 0.0);
    } else if input == 3 {
        c = textureSampleLevel(i3, smp, q, 0.0);
    } else if input == 4 {
        c = textureSampleLevel(i4, smp, q, 0.0);
    } else if input == 5 {
        c = textureSampleLevel(i5, smp, q, 0.0);
    } else if input == 6 {
        c = textureSampleLevel(i6, smp, q, 0.0);
    } else if input == 7 {
        c = textureSampleLevel(i7, smp, q, 0.0);
    } else if input == 8 {
        c = textureSampleLevel(i8, smp, q, 0.0);
    }
    return c;
}

// the homography taking the unit square's corners (0,0), (1,0), (1,1), (0,1) to a, b, c, d
// (Heckbert's square-to-quad, as the output's corner pin has it)
fn square_to_quad(a: vec2f, b: vec2f, c: vec2f, d: vec2f) -> mat3x3f {
    let s = a - b + c - d;
    let d1 = b - c;
    let d2 = d - c;
    let det = d1.x * d2.y - d2.x * d1.y;
    var gh = vec2f(0.0);
    if abs(det) > 1e-9 {
        gh = vec2f(s.x * d2.y - d2.x * s.y, d1.x * s.y - s.x * d1.y) / det;
    }
    return mat3x3f(vec3f(b - a + gh.x * b, gh.x), vec3f(d - a + gh.y * d, gh.y), vec3f(a, 1.0));
}

// how far p is inside the edge from a to b, in pixels (negative outside, the corners going round
// clockwise on screen)
fn inside(a: vec2f, b: vec2f, p: vec2f) -> f32 {
    let e = b - a;
    return (e.x * (p.y - a.y) - e.y * (p.x - a.x)) / max(length(e), 1e-6);
}

// how far p is from the segment a b
fn segment(a: vec2f, b: vec2f, p: vec2f) -> f32 {
    let e = b - a;
    let t = clamp(dot(p - a, e) / max(dot(e, e), 1e-6), 0.0, 1.0);
    return length(p - a - e * t);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    var out = vec4f(background.rgb * background.a, background.a);
    let size = g.resolution;
    // outlines as thick at any size as at 1080
    let k = size.y / 1080.0;
    var halo = 0.0;
    var line = 0.0;
    for (var n = 0; n < 8; n++) {
        if !wired(n + 1) {
            continue;
        }
        let top = velo_params.v[FIRST_SURFACE + 2 * n];
        let bottom = velo_params.v[FIRST_SURFACE + 2 * n + 1];
        // round the edge: top-left, top-right, bottom-right, bottom-left, in pixels
        let p0 = top.xy * size;
        let p1 = top.zw * size;
        let p2 = bottom.zw * size;
        let p3 = bottom.xy * size;
        // which way round the corners go, so the inside is inside whichever it is
        let e = p2 - p0;
        let turn = sign((p1.x - p0.x) * e.y - (p1.y - p0.y) * e.x + e.x * (p3.y - p0.y) - e.y * (p3.x - p0.x));
        // how far inside the nearest edge: the edge antialiased
        let d = min(min(turn * inside(p0, p1, px), turn * inside(p1, p2, px)), min(turn * inside(p2, p3, px), turn * inside(p3, p0, px)));
        let cover = clamp(d + 0.5, 0.0, 1.0);
        if cover > 0.0 && turn != 0.0 {
            // where this pixel is on the picture: back through the homography (its adjugate,
            // the inverse up to a scale the divide takes out)
            let h = square_to_quad(top.xy, top.zw, bottom.zw, bottom.xy);
            let back = transpose(mat3x3f(cross(h[1], h[2]), cross(h[2], h[0]), cross(h[0], h[1])));
            let q = back * vec3f(uv, 1.0);
            let c = pick(n + 1, clamp(q.xy / q.z, vec2f(0.0), vec2f(1.0))) * cover;
            out = c + out * (1.0 - c.a);
        }
        if outlines {
            let edge = min(min(segment(p0, p1, px), segment(p1, p2, px)), min(segment(p2, p3, px), segment(p3, p0, px)));
            let corner = min(min(distance(px, p0), distance(px, p1)), min(distance(px, p2), distance(px, p3)));
            // white lines and corner dots, on a dark rim so they show on any picture
            line = max(line, max(clamp(1.0 * k + 0.5 - edge, 0.0, 1.0), clamp(5.0 * k + 0.5 - corner, 0.0, 1.0)));
            halo = max(halo, max(clamp(2.5 * k + 0.5 - edge, 0.0, 1.0), clamp(6.5 * k + 0.5 - corner, 0.0, 1.0)));
        }
    }
    out = mix(out, vec4f(0.0, 0.0, 0.0, 1.0), halo * 0.6);
    return mix(out, vec4f(1.0), line);
}
