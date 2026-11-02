<script setup lang="ts">
// A small patch to play with, drawn the way Velo's canvas draws it: Audio In's bands plugged into
// jacks at the foot of the card they move, the picture wired through to the Output, and a cable
// tie holding the signals' cables. The cards and the tie drag; the music runs on the page's clock.
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { withBase } from 'vitepress'
import { hear, still } from '../clock'

const HEADER = 28
const ROW = 56
const VIEW = 107
const VIEW_SMALL = 84
const FOOT = 22
const JACK = 22
const GAP = 9

const nodes = reactive({
  audio: { x: 24, y: 36, w: 200, title: 'Audio In', kind: 'SIGNAL' },
  velo: { x: 384, y: 40, w: 190, title: 'Velo', kind: 'SHADER' },
  fb: { x: 626, y: 206, w: 150, title: 'Feedback', kind: 'EFFECT' },
  out: { x: 796, y: 40, w: 150, title: 'Output', kind: 'OUTPUT' },
})
type Id = keyof typeof nodes

const bands = ['Level', 'Bass', 'Mid', 'High'] as const
// what each signal moves, a jack each on Velo's card
const links = [
  { row: 1, jack: 0, value: 'Spread' },
  { row: 2, jack: 1, value: 'Smoke' },
  { row: 3, jack: 2, value: 'Rays' },
]
const tie = reactive({ x: 318, y: 236 })

const heights: Record<Id, number> = {
  audio: HEADER + ROW * bands.length + FOOT,
  velo: HEADER + VIEW + FOOT + JACK * links.length,
  fb: HEADER + VIEW_SMALL + FOOT,
  out: HEADER + VIEW_SMALL + FOOT,
}

const rowOut = (i: number) => ({ x: nodes.audio.x + nodes.audio.w, y: nodes.audio.y + HEADER + ROW * i + ROW / 2 })
const jackIn = (j: number) => ({ x: nodes.velo.x, y: nodes.velo.y + HEADER + VIEW + FOOT + JACK * j + JACK / 2 })
const outOf = (id: Id) => ({ x: nodes[id].x + nodes[id].w, y: nodes[id].y + HEADER + (id === 'velo' ? VIEW : VIEW_SMALL) / 2 })
const inOf = (id: Id) => ({ x: nodes[id].x, y: nodes[id].y + HEADER + VIEW_SMALL / 2 })

// a cable leaves and arrives level, curving between
function curve(a: { x: number; y: number }, b: { x: number; y: number }) {
  const c = Math.max(30, Math.abs(b.x - a.x) * 0.5)
  return `M ${a.x} ${a.y} C ${a.x + c} ${a.y}, ${b.x - c} ${b.y}, ${b.x} ${b.y}`
}

// through the tie, side by side and straight while it holds them
const signalPaths = computed(() =>
  links.map((l, i) => {
    const a = rowOut(l.row)
    const b = jackIn(l.jack)
    const y = tie.y + (i - (links.length - 1) / 2) * GAP
    const enter = { x: tie.x - 12, y }
    const leave = { x: tie.x + 12, y }
    return `${curve(a, enter)} L ${leave.x} ${leave.y} ${curve(leave, b).replace(/^M [^C]+/, '')}`
  }),
)
const wires = computed(() => [curve(outOf('velo'), inOf('fb')), curve(outOf('fb'), inOf('out'))])
const tieHeight = (links.length - 1) * GAP + 14

// the music: a second and a half of each band, and a pulse along a cable on its hits
const TRAIL = 40
const history = reactive(bands.map(() => Array<number>(TRAIL).fill(0.2)))
const pulse = reactive(links.map(() => -1))
const kick = ref(0.5)
const lengths = ref<number[]>(links.map(() => 300))
const paths = ref<SVGPathElement[]>([])

function area(values: number[], x: number, y: number, w: number, h: number) {
  const step = w / (values.length - 1)
  const top = values.map((v, i) => `${(x + i * step).toFixed(1)},${(y + h - v * h * 0.9).toFixed(1)}`)
  return `${x},${y + h} ${top.join(' ')} ${x + w},${y + h}`
}

// dragging a card or the tie
const svg = ref<SVGSVGElement>()
let held: { what: Id | 'tie'; dx: number; dy: number } | null = null
const grabbing = ref(false)

function point(e: PointerEvent) {
  const m = svg.value!.getScreenCTM()!.inverse()
  const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(m)
  return { x: p.x, y: p.y }
}

function grab(e: PointerEvent, what: Id | 'tie') {
  const p = point(e)
  const at = what === 'tie' ? tie : nodes[what]
  held = { what, dx: p.x - at.x, dy: p.y - at.y }
  grabbing.value = true
  try {
    svg.value!.setPointerCapture(e.pointerId)
  } catch {
    // a pointer the browser no longer tracks: the drag still follows moves over the patch
  }
}

function drag(e: PointerEvent) {
  if (!held) return
  const p = point(e)
  const at = held.what === 'tie' ? tie : nodes[held.what]
  const w = held.what === 'tie' ? 0 : nodes[held.what].w
  const h = held.what === 'tie' ? 0 : heights[held.what]
  at.x = Math.min(960 - w - 4, Math.max(4, p.x - held.dx))
  at.y = Math.min(384 - h - 4, Math.max(4, p.y - held.dy))
}

function drop() {
  held = null
  grabbing.value = false
}

let frame = 0
let seen: IntersectionObserver | undefined

onMounted(() => {
  const calm = still()
  let visible = true
  let lastSample = 0
  let beat = -1
  let eighth = -1
  const started = [0, 0, 0]
  const tick = (now: number) => {
    const m = hear(now / 1000)
    kick.value = m.kick
    if (now - lastSample > 33) {
      lastSample = now
      const values = [m.level, m.bass, 0.25 + 0.6 * m.snare + 0.15 * m.kick, m.high]
      values.forEach((v, i) => {
        history[i].shift()
        history[i].push(Math.min(1, v))
      })
    }
    const n = Math.floor(m.beats)
    const e = Math.floor(m.beats * 2)
    if (n !== beat) {
      beat = n
      started[0] = now
      if (n % 2 === 1) started[1] = now
    }
    if (e !== eighth) {
      eighth = e
      if (e % 2 === 1) started[2] = now
    }
    paths.value.forEach((p, i) => (lengths.value[i] = p.getTotalLength()))
    started.forEach((s, i) => (pulse[i] = Math.min(1.2, (now - s) / 420)))
    frame = requestAnimationFrame(tick)
  }
  if (calm) {
    paths.value.forEach((p, i) => (lengths.value[i] = p.getTotalLength()))
    return
  }
  const run = () => {
    cancelAnimationFrame(frame)
    if (visible) frame = requestAnimationFrame(tick)
  }
  seen = new IntersectionObserver(([e]) => {
    visible = e.isIntersecting
    run()
  })
  seen.observe(svg.value!)
})

onBeforeUnmount(() => {
  cancelAnimationFrame(frame)
  seen?.disconnect()
})

const logo = withBase('/velo-256.png')
const PULSE = 26
const dash = (i: number) => {
  const len = lengths.value[i]
  const p = pulse[i]
  return { 'stroke-dasharray': `${PULSE} ${len + PULSE}`, 'stroke-dashoffset': `${PULSE - p * (len + PULSE)}`, opacity: p < 0 || p > 1 ? 0 : 1 }
}
</script>

<template>
  <svg
    ref="svg"
    class="patch"
    :class="{ grabbing }"
    viewBox="0 0 960 384"
    role="img"
    aria-label="A patch: Audio In's Bass, Mid and High plugged into jacks on the Velo shader through a cable tie, Velo wired into Feedback and Feedback into the Output"
    @pointermove="drag"
    @pointerup="drop"
    @pointercancel="drop"
  >
    <defs>
      <pattern id="dots" width="24" height="24" patternUnits="userSpaceOnUse">
        <circle cx="1" cy="1" r="1" fill="rgba(255,236,214,0.09)" />
      </pattern>
      <filter id="soft" x="-50%" y="-50%" width="200%" height="200%">
        <feGaussianBlur stdDeviation="3" />
      </filter>
      <clipPath id="view-velo"><rect :width="nodes.velo.w - 2" :height="VIEW" /></clipPath>
      <clipPath id="view-small"><rect :width="148" :height="VIEW_SMALL" /></clipPath>
    </defs>
    <rect width="960" height="384" fill="url(#dots)" />

    <!-- pictures -->
    <g class="wires">
      <path v-for="(d, i) in wires" :key="i" :d="d" class="wire glow" />
      <path v-for="(d, i) in wires" :key="'w' + i" :d="d" class="wire" />
    </g>

    <!-- signals, through the tie -->
    <g class="signals">
      <path v-for="(d, i) in signalPaths" :key="i" ref="paths" :d="d" class="signal" />
      <path v-for="(d, i) in signalPaths" :key="'p' + i" :d="d" class="pulse" :style="dash(i)" />
    </g>

    <!-- the cards -->
    <g v-for="(n, id) in nodes" :key="id" :transform="`translate(${n.x} ${n.y})`" class="card" @pointerdown.prevent="grab($event, id)">
      <rect :width="n.w" :height="heights[id]" rx="10" class="body" :class="{ audio: id === 'audio' }":style="id === 'audio' ? { strokeOpacity: 0.35 + kick * 0.5 } : undefined" />
      <circle cx="16" cy="14" r="4" :class="id === 'audio' ? 'dot-signal' : id === 'out' ? 'dot-out' : 'dot-picture'" />
      <text x="28" y="18.5" class="title">{{ n.title }}</text>
      <line x1="1" :x2="n.w - 1" :y1="HEADER" :y2="HEADER" class="rule" />

      <template v-if="id === 'audio'">
        <g v-for="(b, i) in bands" :key="b" :transform="`translate(0 ${HEADER + ROW * i})`">
          <polygon :points="area(history[i], 1, 6, n.w - 2, ROW - 8)" class="trail" />
          <text x="10" y="17" class="row">{{ b }}</text>
          <line v-if="i > 0" x1="1" :x2="n.w - 1" y1="0" y2="0" class="rule" />
          <circle :cx="n.w" :cy="ROW / 2" r="5" class="port port-signal" />
        </g>
      </template>

      <template v-else>
        <g :transform="`translate(1 ${HEADER})`">
          <g :clip-path="id === 'velo' ? 'url(#view-velo)' : 'url(#view-small)'">
            <rect :width="n.w - 2" :height="id === 'velo' ? VIEW : VIEW_SMALL" fill="#000" />
            <!-- the picture: the mark, its trails echoing behind it past Feedback -->
            <image
              v-for="echo in id === 'velo' ? [1] : [1.2, 1.1, 1]"
              :key="echo"
              :href="logo"
              :x="((n.w - 2) * (1 - echo)) / 2"
              :y="((id === 'velo' ? VIEW : VIEW_SMALL) - (n.w - 2) * echo) / 2 + 6"
              :width="(n.w - 2) * echo"
              :height="(n.w - 2) * echo"
              :opacity="echo === 1 ? 0.8 + kick * 0.2 : 0.28"
            />
            <rect :width="n.w - 2" :height="id === 'velo' ? VIEW : VIEW_SMALL" fill="#fff1df" :opacity="kick * 0.07" />
          </g>
        </g>
        <line x1="1" :x2="n.w - 1" :y1="HEADER + (id === 'velo' ? VIEW : VIEW_SMALL)" :y2="HEADER + (id === 'velo' ? VIEW : VIEW_SMALL)" class="rule" />
        <text x="10" :y="HEADER + (id === 'velo' ? VIEW : VIEW_SMALL) + 15" class="kind">{{ n.kind }}</text>
        <circle v-if="id !== 'out'" :cx="n.w" :cy="HEADER + (id === 'velo' ? VIEW : VIEW_SMALL) / 2" r="5" class="port" />
        <circle v-if="id !== 'velo'" cx="0" :cy="HEADER + VIEW_SMALL / 2" r="5" class="port" />
        <template v-if="id === 'velo'">
          <g v-for="l in links" :key="l.value" :transform="`translate(0 ${HEADER + VIEW + FOOT + JACK * l.jack})`">
            <line x1="1" :x2="n.w - 1" y1="0" y2="0" class="rule" />
            <circle cx="0" :cy="JACK / 2" r="5" class="port port-signal jack" />
            <text x="14" :y="JACK / 2 + 4" class="jack-text">Audio In · {{ bands[l.row] }} <tspan class="arrow">→</tspan> {{ l.value }}</text>
          </g>
        </template>
      </template>
    </g>

    <!-- the tie -->
    <g class="tie" @pointerdown.prevent.stop="grab($event, 'tie')">
      <rect :x="tie.x - 7" :y="tie.y - tieHeight / 2" width="14" :height="tieHeight" rx="7" class="tie-body" />
      <line :x1="tie.x" :x2="tie.x" :y1="tie.y - tieHeight / 2 + 4" :y2="tie.y + tieHeight / 2 - 4" class="tie-line" />
      <rect :x="tie.x - 22" :y="tie.y - tieHeight / 2 - 12" width="44" :height="tieHeight + 24" fill="transparent" />
    </g>
  </svg>
</template>

<style scoped>
.patch {
  display: block;
  width: 100%;
  height: auto;
  touch-action: none;
  user-select: none;
  font-family: var(--vp-font-family-base);
}

.card {
  cursor: grab;
}

.grabbing,
.grabbing .card,
.grabbing .tie {
  cursor: grabbing;
}

.body {
  fill: #181614;
  stroke: rgba(255, 236, 214, 0.14);
  stroke-width: 1;
}

.body.audio {
  stroke: var(--velo-green);
}

.title {
  fill: var(--velo-light);
  font-size: 13px;
  font-weight: 600;
}

.row {
  fill: var(--velo-text);
  font-size: 11px;
}

.kind {
  fill: var(--velo-dim);
  font-family: var(--vp-font-family-mono);
  font-size: 9.5px;
  letter-spacing: 0.08em;
}

.jack-text {
  fill: var(--velo-muted);
  font-size: 10.5px;
}

.arrow {
  fill: var(--velo-green);
}

.rule {
  stroke: rgba(255, 236, 214, 0.08);
}

.dot-signal {
  fill: var(--velo-green);
}

.dot-picture {
  fill: #ffc58a;
}

.dot-out {
  fill: var(--velo-orange);
}

.trail {
  fill: rgba(155, 225, 93, 0.22);
  stroke: var(--velo-green);
  stroke-width: 1.2;
  stroke-linejoin: round;
}

.port {
  fill: #0b0a09;
  stroke: #ffe1bf;
  stroke-width: 1.6;
}

.port-signal {
  stroke: var(--velo-green);
}

.wire {
  fill: none;
  stroke: #ffe1bf;
  stroke-width: 2;
  stroke-dasharray: 10 6;
  animation: flow 1.2s linear infinite;
}

.wire.glow {
  stroke: rgba(255, 200, 150, 0.35);
  stroke-width: 6;
  stroke-dasharray: none;
  filter: url(#soft);
  animation: none;
}

.signal {
  fill: none;
  stroke: rgba(155, 225, 93, 0.75);
  stroke-width: 1.8;
  stroke-dasharray: 2 5;
  stroke-linecap: round;
}

.pulse {
  fill: none;
  stroke: #e6ffcc;
  stroke-width: 3.5;
  stroke-linecap: round;
  filter: drop-shadow(0 0 4px var(--velo-green));
}

.tie {
  cursor: grab;
}

.tie-body {
  fill: #2a2522;
  stroke: rgba(255, 225, 190, 0.6);
  stroke-width: 1.2;
  transition: stroke 0.2s;
}

.tie:hover .tie-body {
  stroke: var(--velo-orange);
}

.tie-line {
  stroke: rgba(255, 225, 190, 0.35);
  stroke-width: 1;
}

@keyframes flow {
  to {
    stroke-dashoffset: -32;
  }
}

@media (prefers-reduced-motion: reduce) {
  .wire {
    animation: none;
  }
}
</style>
