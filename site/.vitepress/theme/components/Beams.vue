<script setup lang="ts">
// Velo's mark, live: two beams of light rising from one point into a V through drifting smoke,
// thin rays fanning round it, dust glinting in the light and a flare along the floor. A port of
// the app's V Beams preset (crates/velo-engine/shaders/presets/v_beams.wgsl) to WebGL 2, played
// by the page's beat clock. The pointer leans and opens the V; a click is a kick.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { hear, still } from '../clock'

const props = withDefaults(
  defineProps<{
    /** Where the beams rise from, in the app's units: -1..1 across, 0.64 near the floor. */
    root?: [number, number]
    spread?: number
    /** The pointer moves the light. */
    follow?: boolean
    /** The most pixels drawn a frame; the picture is soft, so it's scaled up. */
    budget?: number
  }>(),
  { root: () => [0, 0.64], spread: 0.42, follow: true, budget: 1_100_000 },
)

const el = ref<HTMLCanvasElement>()
const failed = ref(false)

const VERTEX = `#version 300 es
void main() {
  vec2 p = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`

const FRAGMENT = `#version 300 es
precision highp float;
precision highp int;

uniform vec2 res;
uniform float time;
uniform vec2 root;
uniform float tilt;
uniform float spread;
uniform float audio;
// the music, as Velo hears it: 0..1 envelopes and clocks that run with the bands
uniform float bass, kick, snare, hat, high, level, levelTime, highTime, beatPhase, beats;
out vec4 color;

// a random number 0..1 for each whole cell
float hash(vec2 c) {
  uvec2 n = uvec2(ivec2(floor(c)));
  uint h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
  h = (h ^ (h >> 16u)) * 2246822519u;
  return float(h ^ (h >> 13u)) / 4294967296.0;
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

// smoke: noise over a few octaves
float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 4; i++) {
    v += a * noise(p);
    p = p * 2.03 + vec2(1.7, 9.2);
    a *= 0.5;
  }
  return v;
}

// a beam from o along dir, opening as it goes and fading far from its root: its core and a halo
vec2 beam(vec2 p, vec2 o, vec2 dir, float w) {
  vec2 d = p - o;
  float along = dot(d, dir);
  float x = abs(dot(d, vec2(-dir.y, dir.x))) / (w * (0.12 + max(along, 0.0)));
  float fade = exp(-max(along, 0.0) * 1.1) * smoothstep(-0.01, 0.03, along);
  return vec2(exp(-x * x * 2.5), exp(-x * x * 0.2)) * fade;
}

// straight up at 0, leaning right as it grows
vec2 heading(float a) { return vec2(sin(a), -cos(a)); }

void main() {
  // the app's coordinates: from the top left, the height 1
  vec2 px = vec2(gl_FragCoord.x, res.y - gl_FragCoord.y);
  vec2 uv = px / res;
  float aspect = res.x / res.y;
  vec2 p = (uv - 0.5) * vec2(aspect, 1.0);
  vec2 o = root * 0.5 * vec2(aspect, 1.0);
  vec2 d = p - o;
  float r = length(d);
  float ang = atan(d.x, -d.y) - tilt;
  float b = audio * bass;
  float k = audio * kick;
  float t = time;
  const float smoke = 0.8;
  const float rays = 0.75;
  const float grain = 0.45;
  const float width = 0.08;
  const vec3 tint = vec3(1.0, 0.88, 0.74);

  // the smoke churns faster as the music gets louder
  float ts = t * 0.5 + audio * levelTime * 0.5;
  // a breath on every beat while something plays
  float breath = 1.0 + 0.3 * audio * smoothstep(0.02, 0.25, level) * exp(-beatPhase * 4.0);
  float lift = breath * (1.0 + 0.9 * b);

  // the V opens and its beams widen with the bass
  float a = spread * (1.0 + 0.25 * b);
  float w = width * (1.0 + 0.6 * b);
  vec2 dl = heading(tilt - a);
  vec2 dr = heading(tilt + a);
  vec2 bl = beam(p, o, dl, w);
  vec2 br = beam(p, o, dr, w);
  float core = bl.x + br.x;
  float halo = bl.y + br.y;

  // smoke rising in two layers, folded by a slow warp so it billows
  vec2 q = p * 2.4 + vec2(0.0, ts * 0.22);
  vec2 warp = vec2(noise(q * 0.6 + vec2(ts * 0.07, 3.1)), noise(q * 0.6 + vec2(7.4, -ts * 0.05))) - 0.5;
  float near = fbm(q + warp * 1.8);
  float far = fbm(p * 1.2 + vec2(-ts * 0.04, ts * 0.09) + warp * 0.9);
  float dens = smoke * (0.3 + 0.7 * smoothstep(-0.5, 0.35, p.y)) * smoothstep(0.45, 0.72, near * 0.65 + far * 0.55);
  // and a bank of it rolling along the floor, thickest round the root
  float up = -d.y;
  float bank = smoke * smoothstep(0.38, 0.66, fbm(vec2(p.x * 1.6 + ts * 0.06, p.y * 3.5 + ts * 0.1) + warp * 1.4))
    * exp(-max(up, 0.0) * 2.5) * exp(-max(-up, 0.0) * 40.0) * exp(-abs(d.x) * 0.6);

  // shafts in the light, radiating from the root
  float shafts = smoothstep(0.2, 0.8, noise(vec2(ang * 26.0, r * 1.2 - ts * 0.35)) * 0.6 + noise(vec2(ang * 61.0 + 4.0, r * 2.0 - ts * 0.5)) * 0.4);
  // dusty grain in the light
  float grit = mix(1.0, 0.55 + 0.9 * hash(px * 0.7 + vec2(floor(t * 24.0) * 13.0, 0.0)), grain);
  // the beams: solid where the smoke is thick, streaked by the shafts
  float light = (core * (0.4 + 1.3 * dens) * (0.5 + 0.9 * shafts) + halo * 0.1 * (0.2 + 1.5 * dens)) * grit * lift;

  // thin rays fanning round the V, drifting, sweeping with the highs, flickered by the hats and snare
  float fan = 0.0;
  float step16 = floor(beats * 4.0);
  for (int i = 0; i < 9; i++) {
    float n = float(i);
    float h = hash(vec2(n, 17.0));
    float at = (n - 4.0) * 0.24 + 0.08 * sin(t * (0.2 + 0.2 * h) + h * 6.28) + 0.06 * sin(audio * highTime * 0.9 + n * 2.1);
    float off = abs(sin(ang - at)) * r;
    float lit = exp(-off / (0.0015 + r * 0.006)) + 0.15 * exp(-off / (0.01 + r * 0.04));
    float on = hash(vec2(n, step16));
    fan += lit * (0.12 + 0.08 * sin(t * 1.7 + n * 2.3) + audio * (hat * step(0.4, on) + 0.6 * snare * on));
  }
  light += fan * rays * (0.15 + 1.2 * dens) * exp(-r * 1.6) * smoothstep(0.0, 0.06, r) * smoothstep(-0.01, 0.03, up) * grit * lift;

  // the root: a hot point, a bloom round it and a flare along the floor, flashing on the kick
  float flash = 1.0 + 1.6 * k;
  float glow = exp(-r * 6.0);
  float hot = exp(-r * 30.0);
  float flare = exp(-abs(d.y) * 160.0) * exp(-abs(d.x) * 2.5 / (1.0 + k)) + 0.2 * exp(-abs(d.y) * 30.0) * exp(-abs(d.x) * 2.0);
  light += (hot * 1.4 + glow * 0.15 + flare * 0.8) * flash * breath;
  // the smoke above the floor lit by it and by what spills from the beams, in shafts
  // (darker between the beams, so the V keeps its shape)
  float spill = (exp(-r * 2.0) * 0.45 * (0.3 + 0.7 * smoothstep(0.0, a, abs(ang))) + halo * 0.25 + glow * 0.3) * (1.0 - smoothstep(-0.01, 0.03, d.y));
  light += (dens * (0.5 + shafts * shafts) + bank * 2.0) * spill * (1.0 + 0.6 * k) * (1.0 + 0.5 * b);
  light += bank * (core * 0.5 + flare * 0.5 + 0.05) * flash;

  // dust glinting where the light is, more with the highs
  float glints = 0.0;
  for (int l = 0; l < 2; l++) {
    float fl = float(l);
    vec2 s = (p + vec2(0.02 * sin(t * 0.3 + fl), ts * (0.03 + 0.02 * fl))) * (70.0 + 50.0 * fl);
    vec2 cell = floor(s) + fl * 57.0;
    float h = hash(cell);
    vec2 spot = vec2(hash(cell + vec2(113.0, 0.0)), hash(cell + vec2(0.0, 211.0))) * 0.6 + 0.2;
    float twinkle = pow(0.5 + 0.5 * sin(t * (2.0 + 4.0 * h) + h * 40.0), 6.0) * (0.6 + 1.5 * audio * max(hat, high));
    glints += smoothstep(0.18, 0.0, length(fract(s) - spot)) * step(0.55, h) * twinkle;
  }
  light += glints * (core * 1.5 + halo * 0.3 + glow * 0.6);

  // a shockwave of light running out along the floor on the kick
  float e = length(vec2(d.x, d.y * 6.0));
  float rad = (1.0 - kick) * 1.2;
  float x = (e - rad) / (0.02 + 0.08 * rad);
  float wisp = noise(vec2(d.x * 7.0, d.y * 40.0) - ts);
  light += exp(-x * x) * sqrt(kick) * audio * (0.25 + 0.75 * wisp) * (0.5 + 1.8 * bank) * (0.6 + 0.4 * step(0.0, d.y));

  // the floor: a faint, rippled reflection of the V below the root
  if (d.y > 0.0) {
    vec2 pm = vec2(p.x + (noise(vec2(p.x * 8.0, d.y * 90.0 - t * 2.0)) - 0.5) * 0.02, o.y - d.y);
    float rl = beam(pm, o, dl, w * 1.4).x + beam(pm, o, dr, w * 1.4).x;
    light += rl * 0.3 * exp(-d.y * 10.0) * lift;
  }

  // warm light, white where it's hottest, the unlit smoke a cold deep blue
  vec3 c = tint * light + vec3(1.0) * (hot * hot * (0.6 + k) + core * core * 0.12 * (1.0 + b)) + vec3(0.3, 0.38, 0.55) * (dens * 0.1 + bank * 0.1);
  c *= 1.0 - 0.6 * dot(uv - 0.5, uv - 0.5);
  color = vec4(1.0 - exp(-c * 1.25), 1.0);
}`

let stop = () => {}

onMounted(() => {
  const canvas = el.value!
  const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, depth: false, stencil: false, powerPreference: 'high-performance' })
  if (!gl) {
    failed.value = true
    return
  }
  const shader = (type: number, src: string) => {
    const s = gl.createShader(type)!
    gl.shaderSource(s, src)
    gl.compileShader(s)
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) console.warn(gl.getShaderInfoLog(s))
    return s
  }
  const program = gl.createProgram()!
  gl.attachShader(program, shader(gl.VERTEX_SHADER, VERTEX))
  gl.attachShader(program, shader(gl.FRAGMENT_SHADER, FRAGMENT))
  gl.linkProgram(program)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    console.warn(gl.getProgramInfoLog(program))
    failed.value = true
    return
  }
  gl.useProgram(program)
  const names = ['res', 'time', 'root', 'tilt', 'spread', 'audio', 'bass', 'kick', 'snare', 'hat', 'high', 'level', 'levelTime', 'highTime', 'beatPhase', 'beats'] as const
  const u = Object.fromEntries(names.map((n) => [n, gl.getUniformLocation(program, n)])) as Record<(typeof names)[number], WebGLUniformLocation | null>

  const calm = still()
  // the share of the budget drawn: lowered while frames come slow
  let quality = 1
  const size = () => {
    const box = canvas.getBoundingClientRect()
    const dpr = Math.min(window.devicePixelRatio || 1, 2)
    const w = box.width * dpr
    const h = box.height * dpr
    const s = Math.min(1, Math.sqrt((props.budget * quality) / Math.max(1, w * h)))
    canvas.width = Math.max(1, Math.round(w * s))
    canvas.height = Math.max(1, Math.round(h * s))
    gl.viewport(0, 0, canvas.width, canvas.height)
  }

  // the pointer, eased: 0..1 over the canvas, the middle when it's away
  const aim = { x: 0.5, y: 0.5 }
  const at = { x: 0.5, y: 0.5 }
  let punch = 0
  let levelTime = 0
  let highTime = 0
  let last = performance.now()
  let slow = 0

  const draw = (now: number) => {
    const t = now / 1000
    const dt = Math.min(0.1, (now - last) / 1000)
    last = now
    const m = hear(t)
    const ease = 1 - Math.exp(-dt * 3)
    at.x += (aim.x - at.x) * ease
    at.y += (aim.y - at.y) * ease
    punch *= Math.exp(-dt * 5)
    levelTime += dt * m.level * 1.4
    highTime += dt * (0.4 + m.high)
    const aspect = canvas.width / canvas.height
    // on a narrow screen the V closes so its beams stay in the picture
    const spread = Math.min(props.spread, Math.atan((0.6 * aspect) / 0.82)) * (1 + (0.5 - at.y) * 0.3)
    gl.uniform2f(u.res, canvas.width, canvas.height)
    gl.uniform1f(u.time, t)
    gl.uniform2f(u.root, props.root[0] + (at.x - 0.5) * 0.3, props.root[1])
    gl.uniform1f(u.tilt, (at.x - 0.5) * 0.32)
    gl.uniform1f(u.spread, spread)
    gl.uniform1f(u.audio, 0.55 + 0.45 * m.energy)
    gl.uniform1f(u.bass, m.bass)
    gl.uniform1f(u.kick, Math.min(1, m.kick + punch))
    gl.uniform1f(u.snare, m.snare)
    gl.uniform1f(u.hat, m.hat)
    gl.uniform1f(u.high, m.high)
    gl.uniform1f(u.level, m.level)
    gl.uniform1f(u.levelTime, levelTime)
    gl.uniform1f(u.highTime, highTime)
    gl.uniform1f(u.beatPhase, m.phase)
    gl.uniform1f(u.beats, m.beats)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    // under 40 frames a second for a while: fewer pixels
    slow = dt > 1 / 40 ? slow + 1 : Math.max(0, slow - 1)
    if (slow > 45 && quality > 0.3) {
      quality *= 0.7
      slow = 0
      size()
    }
  }

  // for anyone who asked for less motion: one still frame, just after a kick
  const still_frame = () => {
    last = performance.now()
    draw((64.04 * 60 * 1000) / 124)
  }

  let frame = 0
  let visible = true
  const loop = (now: number) => {
    draw(now)
    frame = requestAnimationFrame(loop)
  }
  const run = () => {
    cancelAnimationFrame(frame)
    frame = 0
    if (calm) return still_frame()
    if (visible && !document.hidden) {
      last = performance.now()
      frame = requestAnimationFrame(loop)
    }
  }

  const resized = new ResizeObserver(() => {
    size()
    if (calm || !frame) still_frame()
  })
  resized.observe(canvas)
  const seen = new IntersectionObserver(([e]) => {
    visible = e.isIntersecting
    run()
  })
  seen.observe(canvas)
  const shown = () => run()
  document.addEventListener('visibilitychange', shown)

  const host = canvas.parentElement!.parentElement!
  const move = (e: PointerEvent) => {
    if (!props.follow) return
    const box = canvas.getBoundingClientRect()
    aim.x = Math.min(1, Math.max(0, (e.clientX - box.left) / box.width))
    aim.y = Math.min(1, Math.max(0, (e.clientY - box.top) / box.height))
  }
  const away = () => {
    aim.x = 0.5
    aim.y = 0.5
  }
  const hit = (e: PointerEvent) => {
    if (!(e.target as HTMLElement).closest('a, button')) punch = 1
  }
  host.addEventListener('pointermove', move, { passive: true })
  host.addEventListener('pointerleave', away)
  host.addEventListener('pointerdown', hit)
  canvas.addEventListener('webglcontextlost', (e) => {
    e.preventDefault()
    cancelAnimationFrame(frame)
    failed.value = true
  })

  size()
  run()
  stop = () => {
    cancelAnimationFrame(frame)
    resized.disconnect()
    seen.disconnect()
    document.removeEventListener('visibilitychange', shown)
    host.removeEventListener('pointermove', move)
    host.removeEventListener('pointerleave', away)
    host.removeEventListener('pointerdown', hit)
  }
})

onBeforeUnmount(() => stop())
</script>

<template>
  <div class="beams" :class="{ fallback: failed }">
    <canvas ref="el" aria-hidden="true" />
  </div>
</template>

<style scoped>
.beams {
  position: absolute;
  inset: 0;
  overflow: hidden;
  background: #000;
}

canvas {
  display: block;
  width: 100%;
  height: 100%;
}

/* no WebGL 2: the V drawn with gradients */
.fallback canvas {
  display: none;
}

.fallback::before,
.fallback::after {
  content: '';
  position: absolute;
  left: 50%;
  bottom: 18%;
  width: 14%;
  height: 90%;
  transform-origin: 50% 100%;
  background: linear-gradient(to top, rgba(255, 240, 220, 0.85), rgba(255, 225, 190, 0.15) 70%, transparent);
  filter: blur(18px);
  clip-path: polygon(46% 100%, 54% 100%, 100% 0, 0 0);
}

.fallback::before {
  transform: translateX(-50%) rotate(-24deg);
}

.fallback::after {
  transform: translateX(-50%) rotate(24deg);
}
</style>
