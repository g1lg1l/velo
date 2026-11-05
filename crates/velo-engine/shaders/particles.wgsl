/*{
  "name": "Particles",
  "category": "generator",
  "description": "Soft round sprites, or a picture of your own (a still, a GIF, a movie, or anything wired into sprite), born from an emitter (a point, a line, a circle, or the lit parts of a picture wired into emit) at emission a second, sent off along direction (in turns: 0 right, a quarter down, three quarters up) within spread, pulled by gravity, slowed by drag, swirled by turbulence, fading and growing or shrinking over their life, turning by spin. Emission starts at 0: map Kick onto it for a burst on every kick",
  "particles": true,
  "inputs": [{ "id": "emit", "type": "texture" }, { "id": "sprite", "type": "texture" }],
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "emission", "type": "float", "default": 0.0, "min": 0.0, "max": 5000.0 },
    { "id": "life", "type": "float", "default": 2.0, "min": 0.1, "max": 10.0 },
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "direction", "type": "float", "default": 0.75, "min": 0.0, "max": 1.0 },
    { "id": "spread", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "gravity", "type": "float", "default": 0.0, "min": -2.0, "max": 2.0 },
    { "id": "drag", "type": "float", "default": 0.0, "min": 0.0, "max": 5.0 },
    { "id": "turbulence", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "size", "type": "float", "default": 0.01, "min": 0.001, "max": 0.1 },
    { "id": "end_size", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "tint", "type": "color", "default": [1.0, 0.8, 0.5, 1.0] },
    { "id": "fade_out", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "shape", "type": "int", "default": 0, "names": ["Point", "Line", "Circle", "Picture"] },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "width", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "count", "type": "int", "default": 16384, "min": 256, "max": 65536 },
    { "id": "spin", "type": "float", "default": 0.0, "min": -4.0, "max": 4.0 },
    { "id": "opacity", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// Positions and speeds in canvas heights, x across 0..aspect and y down 0..1; size is a
// sprite's radius at birth, end_size how much of it is left at death; width is a line's length
// or a circle's diameter. count: how many live at most (the oldest go first). sprite: the
// node's file when it has one, else what's wired in (a 1×1 black when neither: soft dots then);
// spin: turns a second. emit: with shape Picture, particles are born where it's lit (its
// brightness times its alpha is the odds); a black one, or a few lit pixels in a big one, gives
// few or none: position moves the whole picture of them.

struct Particle {
    pos: vec2f,
    vel: vec2f,
    age: f32,
    life: f32,
    seed: f32,
    size: f32,
}

// The ring the emitter writes into: where the next one goes, this frame's first and how many
// are born, what's left over of one from the frames before.
struct Sim {
    head: u32,
    start: u32,
    spawned: u32,
    carry: f32,
    p: array<Particle>,
}

// past the textures (the sprite is binding 3): the buffer of the particles
@group(0) @binding(15) var<storage, read_write> sim: Sim;

const TAU: f32 = 6.2831853;

// a stall never throws everything out at once
fn dt() -> f32 {
    return min(g.delta, 0.1);
}

fn kept() -> u32 {
    return clamp(u32(max(count, 1)), 1u, arrayLength(&sim.p));
}

// PCG
fn hash(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn random(s: ptr<function, u32>) -> f32 {
    *s = hash(*s);
    return f32(*s >> 8u) / 16777216.0;
}

fn noise(p: vec2f) -> f32 {
    let c = floor(p);
    let f = p - c;
    let u = f * f * (3.0 - 2.0 * f);
    var d = array<f32, 4>();
    for (var i = 0; i < 4; i++) {
        let o = vec2f(f32(i & 1), f32(i >> 1));
        let a = f32(hash((bitcast<u32>(i32(c.x + o.x)) * 1597334677u) ^ (bitcast<u32>(i32(c.y + o.y)) * 3812015801u))) / 4294967296.0 * TAU;
        d[i] = dot(vec2f(cos(a), sin(a)), f - o);
    }
    return mix(mix(d[0], d[1], u.x), mix(d[2], d[3], u.x), u.y);
}

// the noise's slope turned a quarter: a swirl that never bunches them up
fn curl(p: vec2f) -> vec2f {
    let e = 0.01;
    let dx = noise(p + vec2f(e, 0.0)) - noise(p - vec2f(e, 0.0));
    let dy = noise(p + vec2f(0.0, e)) - noise(p - vec2f(0.0, e));
    return vec2f(dy, -dx) / (2.0 * e);
}

@compute @workgroup_size(1)
fn velo_emit() {
    velo_load();
    let n = kept();
    // audio: a burst on every kick, a stream as loud as the music
    let due = min(sim.carry + max(emission + audio * (2500.0 * g.kick + 200.0 * g.level), 0.0) * dt(), f32(n));
    let spawned = u32(due);
    sim.carry = due - f32(spawned);
    sim.start = sim.head % n;
    sim.spawned = spawned;
    sim.head = (sim.start + spawned) % n;
}

// a lit place on the emit picture (0..1 across, z is 1 when found), by guessing: the brighter,
// the likelier. Only the few born each frame guess, so a lot of guesses is cheap; a picture lit
// on under a hundredth of it finds one less and less often, none on a black one.
fn lit_spot(s: ptr<function, u32>) -> vec3f {
    let dims = vec2f(textureDimensions(emit));
    for (var i = 0; i < 512; i++) {
        let q = vec2f(random(s), random(s));
        let c = textureLoad(emit, vec2i(q * dims), 0);
        if dot(c.rgb, vec3f(0.299, 0.587, 0.114)) * c.a > random(s) {
            return vec3f(q, 1.0);
        }
    }
    return vec3f(0.0);
}

// the k-th born this frame, in slot i: the later ones younger, so a stream doesn't come in clumps
fn born(i: u32, k: u32) -> Particle {
    var s = hash(i ^ hash(u32(g.frame) * 9781u + k));
    let d = direction * TAU;
    var at = (0.5 + position * 0.5) * vec2f(g.aspect, 1.0);
    var found = true;
    if shape == 3 {
        let spot = lit_spot(&s);
        at = (spot.xy + position * 0.5) * vec2f(g.aspect, 1.0);
        found = spot.z > 0.5;
    } else if shape == 1 {
        // across the direction
        at += vec2f(-sin(d), cos(d)) * (random(&s) - 0.5) * width;
    } else if shape == 2 {
        let a = random(&s) * TAU;
        at += vec2f(cos(a), sin(a)) * width * 0.5;
    }
    // 0 is right, a quarter turn down (y runs down), three quarters up
    let a = d + (random(&s) - 0.5) * spread * TAU;
    var p: Particle;
    p.vel = vec2f(cos(a), sin(a)) * speed * mix(0.6, 1.0, random(&s));
    // one that found no place is dead at birth
    p.life = select(0.0, life * mix(0.75, 1.25, random(&s)), found);
    p.seed = random(&s);
    p.size = mix(0.5, 1.0, random(&s));
    p.age = dt() * (1.0 - (f32(k) + 0.5) / f32(sim.spawned));
    p.pos = at + p.vel * p.age;
    return p;
}

@compute @workgroup_size(64)
fn velo_step(@builtin(global_invocation_id) id: vec3u) {
    let i = id.x;
    if i >= arrayLength(&sim.p) {
        return;
    }
    velo_load();
    let n = kept();
    let k = (i + n - sim.start) % n;
    var p = sim.p[i];
    if i >= n {
        p.life = 0.0;
    } else if k < sim.spawned {
        p = born(i, k);
    } else if p.age < p.life {
        let t = dt();
        p.vel.y += gravity * t;
        p.vel *= exp(-drag * t);
        p.pos += (p.vel + curl(p.pos * 3.0 + p.seed * 0.3) * turbulence) * t;
        p.age += t;
    }
    sim.p[i] = p;
}

struct Sprite {
    @builtin(position) pos: vec4f,
    @location(0) corner: vec2f,
    @location(1) color: vec4f,
    // the corner turned by its spin, for the picture
    @location(2) turned: vec2f,
}

// a: position and velocity, b: age, life, seed, size
@vertex
fn velo_vs(@builtin(vertex_index) v: u32, @location(0) a: vec4f, @location(1) b: vec4f) -> Sprite {
    velo_load();
    let corner = vec2f(f32(v & 1u), f32(v >> 1u)) * 2.0 - 1.0;
    let t = clamp(b.x / max(b.y, 0.0001), 0.0, 1.0);
    // a dead one has no area: nothing drawn
    let r = select(0.0, size * b.w * mix(1.0, end_size, t), b.x < b.y);
    let at = a.xy + corner * r;
    var out: Sprite;
    out.pos = vec4f(at.x / g.aspect * 2.0 - 1.0, 1.0 - at.y * 2.0, 0.0, 1.0);
    out.corner = corner;
    let turn = spin * b.x * 6.2831853;
    out.turned = vec2f(corner.x * cos(turn) - corner.y * sin(turn), corner.x * sin(turn) + corner.y * cos(turn));
    // as every picture: its opacity, over black or what's under it
    out.color = vec4f(tint.rgb, 1.0) * tint.a * clamp(opacity, 0.0, 1.0) * (1.0 - fade_out * smoothstep(0.0, 1.0, t));
    return out;
}

@fragment
fn velo_fs(s: Sprite) -> @location(0) vec4f {
    let d = 1.0 - min(dot(s.corner, s.corner), 1.0);
    // the picture, its shape kept inside the sprite's square (y down, as the picture's)
    let size = vec2f(textureDimensions(sprite));
    let fit = select(vec2f(1.0, size.x / size.y), vec2f(size.y / size.x, 1.0), size.y > size.x);
    let q = s.turned / fit;
    let picture = textureSample(sprite, smp, q * 0.5 + 0.5);
    let inside = f32(all(abs(q) <= vec2f(1.0)));
    let shown = vec4f(picture.rgb * picture.a, picture.a) * inside;
    return select(s.color * d * d, s.color * shown, size.x > 1.0);
}
