/*{
  "name": "Mesh",
  "category": "generator",
  "description": "A 3D model (an .obj or .ply with faces, set as its file) drawn solid, with a depth test, lit by a light, and seen from a camera turning round it: smooth or faceted shading, a shine, a studio reflection, a rim light; the surface breathes with the bass and everything flashes on the kick. One mesh, no shadows or textures",
  "mesh": true,
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "size", "type": "float", "default": 0.5, "min": 0.05, "max": 1.5 },
    { "id": "distance", "type": "float", "default": 1.6, "min": 0.4, "max": 6.0 },
    { "id": "orbit", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "tilt", "type": "float", "default": 0.04, "min": -0.25, "max": 0.25 },
    { "id": "turn", "type": "float", "default": 0.05, "min": -1.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.85, 0.9, 1.0, 1.0] },
    { "id": "light", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "light_color", "type": "color", "default": [1.0, 0.95, 0.85, 1.0] },
    { "id": "light_at", "type": "vec2", "default": [-0.55, -0.6], "min": -1.0, "max": 1.0 },
    { "id": "ambient", "type": "float", "default": 0.18, "min": 0.0, "max": 1.0 },
    { "id": "shine", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "mirror", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "rim", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "facets", "type": "bool", "default": false },
    { "id": "bulge", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "opacity", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// A model's triangles (gpu.rs): each corner a position and a normal, the model fitted into a
// unit ball (model.rs), y down as the canvas runs. size is its radius in canvas heights;
// distance, orbit, tilt and turn work as the Particles node's Orbit view: a pinhole camera of 50
// degrees round the middle of the canvas, x right, y down, z toward the camera. light_at is
// where the light sits on the model's front (x right, y down; the middle is straight on),
// fixed in the world while the camera goes round. The picture is see-through where there's no
// model, and premultiplied, as every picture.

const TAU: f32 = 6.2831853;

// from the model's world to what the camera sees: yaw round the vertical, then pitch
fn turned(v: vec3f) -> vec3f {
    let yaw = (orbit + turn * mix(g.time, g.bass_time, clamp(audio, 0.0, 1.0))) * TAU;
    let pitch = tilt * TAU;
    let x = v.x * cos(yaw) + v.z * sin(yaw);
    let front = -v.x * sin(yaw) + v.z * cos(yaw);
    return vec3f(x, v.y * cos(pitch) - front * sin(pitch), v.y * sin(pitch) + front * cos(pitch));
}

// v made a unit, or `fallback` when it has no direction (a flat face, a stray vertex)
fn unit(v: vec3f, fallback: vec3f) -> vec3f {
    let len = length(v);
    return select(fallback, v / max(len, 1e-6), len > 1e-6);
}

struct Vertex {
    @builtin(position) pos: vec4f,
    @location(0) eye: vec3f,
    @location(1) normal: vec3f,
}

@vertex
fn velo_vs(@location(0) p: vec3f, @location(1) n: vec3f) -> Vertex {
    velo_load();
    // the surface breathing: a wave over the model, pushed along the normal, harder with the bass
    let wave = sin(dot(p, vec3f(7.1, 5.3, 6.2)) + g.time * 3.0) * sin(dot(p, vec3f(-4.3, 8.1, 3.7)) - g.time * 2.1);
    let e = turned((p + n * wave * bulge * (0.12 + 0.5 * audio * g.bass)) * size);
    let ahead = max(distance - e.z, 0.001);
    var out: Vertex;
    out.eye = e;
    out.normal = turned(n);
    // depth from 0.05 to 50 canvas heights in front of the camera
    out.pos = vec4f(e.x / (0.4663 * g.aspect), -e.y / 0.4663, 50.0 * (ahead - 0.05) / 49.95, ahead);
    return out;
}

@fragment
fn velo_fs(v: Vertex) -> @location(0) vec4f {
    velo_load();
    // the facet the pixel is on, from how its position changes across the screen
    let facet = cross(dpdx(v.eye), dpdy(v.eye));
    let rounded = unit(v.normal, vec3f(0.0, 0.0, 1.0));
    var n = rounded;
    if facets {
        n = unit(facet, rounded);
        n = select(-n, n, dot(n, rounded) >= 0.0);
    }
    let view = unit(vec3f(0.0, 0.0, distance) - v.eye, vec3f(0.0, 0.0, 1.0));
    // seen from behind (faces wound the other way): lit as the side it shows
    n = select(-n, n, dot(n, view) >= 0.0);
    let on = clamp(light_at, vec2f(-1.0), vec2f(1.0));
    let l = turned(unit(vec3f(on, sqrt(max(1.0 - dot(on, on), 0.04))), vec3f(0.0, 0.0, 1.0)));
    let lit = max(dot(n, l), 0.0);
    let mid = unit(l + view, view);
    let spec = pow(max(dot(n, mid), 0.0), 48.0) * shine * select(0.0, 1.0, lit > 0.0);
    let fresnel = pow(1.0 - clamp(dot(n, view), 0.0, 1.0), 3.0);
    // a studio to reflect: sky above (y runs down), a dark floor, a bright softbox
    let r = reflect(-view, n);
    let sky = smoothstep(-0.3, 0.7, -r.y);
    let softbox = pow(max(dot(r, unit(vec3f(-0.4, -0.8, 0.5), view)), 0.0), 18.0) * 2.5;
    let studio = mix(vec3f(0.02, 0.025, 0.04), vec3f(0.75, 0.85, 1.0), sky) + vec3f(softbox);
    // more ambient from above
    let around = ambient * (0.6 + 0.4 * clamp(-n.y, -1.0, 1.0));
    var color = tint.rgb * (around + light * lit * light_color.rgb) + light_color.rgb * spec * light;
    color = mix(color, studio * mix(vec3f(1.0), tint.rgb, 0.6), clamp(mirror, 0.0, 1.0) * (0.2 + 0.8 * fresnel));
    color += light_color.rgb * rim * fresnel * 0.8;
    // the kick flashes it; never below black, never a number that isn't one
    color = max(color * (1.0 + audio * g.kick * 0.6), vec3f(0.0));
    let a = clamp(opacity, 0.0, 1.0);
    return vec4f(color * a, a);
}
