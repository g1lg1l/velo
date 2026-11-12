<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { withBase } from 'vitepress'
import Beams from './Beams.vue'
import BeatClock from './BeatClock.vue'
import Marquee from './Marquee.vue'
import Patch from './Patch.vue'
import Spotlight from './Spotlight.vue'
import BorderBeam from './BorderBeam.vue'
import CountUp from './CountUp.vue'
import AiDemo from './AiDemo.vue'
import Gallery from './Gallery.vue'
import { still } from '../clock'

const download = 'https://github.com/g1lg1l/velo/releases/latest'
const repo = 'https://github.com/g1lg1l/velo'

const features = [
  'Computer audio, no cables',
  'Kick · Snare · Hat · Melody',
  'Auto BPM',
  'Tap tempo',
  'Ableton Link',
  'MIDI clock',
  'MIDI learn',
  'Soft takeover',
  'OSC in',
  'Spout',
  'NDI',
  'Fullscreen on any display',
  'WGSL shaders, reloaded on save',
  'ISF import',
  'HAP · H.264 · HEVC',
  '65k GPU particles',
  'Feedback',
  'Autopilot',
  'Blackout · Freeze',
  'Test pattern',
  'MCP server',
]
const presets = [
  'V Beams',
  'Plasma',
  'Liquid Chrome',
  'Fire',
  'Lasers',
  'Fractal',
  'Code Rain',
  'Aurora',
  'Galaxy',
  'Synthwave',
  'Hyperspace',
  'Ferrofluid',
  'Metaballs',
  'Voronoi',
  'Truchet',
  'Starfield',
  'Iridescence',
  'Light Rays',
  'Glitch',
  'VHS',
  'LED Wall',
  'God Rays',
  'Halftone',
  'Shake',
]

const cards = [
  {
    title: 'Hears the music',
    glow: '155, 225, 93',
    icon: 'M3 12h2l2-6 3 14 3-11 2 6 2-3h4',
    text: 'The computer’s own sound or any input, split into level, bass, mid, high, kick, snare, hat and melody. The tempo heard, tapped, or followed from Ableton Link or a MIDI clock.',
  },
  {
    title: 'Makes pictures',
    glow: '255, 205, 160',
    icon: 'M12 3l2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5z',
    text: 'Generators, effects and composites, 76 shader presets and 25 effect presets. Particles on the GPU, stills, GIFs and movies in HAP, H.264 and HEVC.',
  },
  {
    title: 'Patches by hand',
    glow: '255, 205, 160',
    icon: 'M5 6h4v4H5zM15 14h4v4h-4zM9 8c4 0 2 8 6 8',
    text: 'One graph per scene, every node showing its picture. Tab opens the node menu, wires snap into ports, and cables bundle under ties.',
  },
  {
    title: 'Plays live',
    glow: '244, 120, 96',
    icon: 'M7 4v16l13-8z',
    text: 'Scenes launched on the beat, the bar or four bars, through Cut, Crossfade, Wipe, Zoom, Glitch or Flash. Eight macros a scene and variations that morph.',
  },
  {
    title: 'Goes out',
    glow: '255, 205, 160',
    icon: 'M3 5h18v11H3zM8 20h8M12 16v4',
    text: 'Fullscreen on any display with the picture’s shape kept. Spout to Resolume, TouchDesigner, OBS or MadMapper, NDI to any computer on the network.',
  },
  {
    title: 'Builds with AI',
    glow: '244, 120, 96',
    icon: 'M12 3v4M12 17v4M3 12h4M17 12h4M6 6l2.5 2.5M15.5 15.5L18 18M18 6l-2.5 2.5M8.5 15.5L6 18',
    text: 'Describe a visual and Claude Code, Codex or a local model builds it from Velo’s own nodes. Velo is an MCP server too.',
  },
]

const stats = [
  { to: 40, suffix: '', label: 'shader presets' },
  { to: 25, suffix: '', label: 'effect presets' },
  { to: 8, suffix: '', label: 'macros a scene' },
  { to: 6, suffix: '', label: 'transitions' },
  { to: 65, suffix: 'k', label: 'particles on the GPU' },
]

const shots = [
  { src: '/shots/demo.jpg', title: 'The demo', alt: 'The Velo scene live: Audio In’s bands moving a V Beams shader, the preview, the inspector and the deck of macros and sources.' },
  { src: '/shots/menu.jpg', title: 'The node menu', alt: 'Tab opens every node with its icon and a line on what it does, a tab per group.' },
  { src: '/shots/shared.jpg', title: 'Shared signals', alt: 'One signal graph for the whole show: a kick and an energy built once, mapped by every scene.' },
  { src: '/shots/signals.jpg', title: 'Signal cards', alt: 'Each signal card says what it does and draws what comes in and what goes out, on a grid.' },
  { src: '/shots/mapping.jpg', title: 'A mapping', alt: 'Kick moving Kaleido’s zoom: its amount, attack, release and invert on the link itself.' },
  { src: '/shots/macros.jpg', title: 'Macros and MIDI', alt: 'Eight macros with their controller badges, variations A to H, and MIDI learn waiting for a knob.' },
  { src: '/shots/shader.jpg', title: 'Your own shader', alt: 'A WGSL shader open in Velo’s editor, its knobs made from its header.' },
  { src: '/shots/scenes.jpg', title: 'Scenes', alt: 'Two scenes: one live, the next one previewed and armed to launch on four bars.' },
  { src: '/shots/tunnel.jpg', title: 'Tunnel', alt: 'The Tunnel template in split view: the picture beside its nodes, its Speed and Trails macros on the deck.' },
  { src: '/shots/rings.jpg', title: 'Pulse Rings', alt: 'The Pulse Rings template: Shapes through Feedback, its Size, Spin and Trails macros.' },
  { src: '/shots/ai.webp', title: 'The AI panel', alt: 'Asked for a purple tunnel that pulses on the kick, the AI built it from Velo’s own nodes.' },
]

const steps = [
  { n: '01', title: 'Play some music', text: 'Velo hears what the computer plays. No cables, no setup.' },
  { n: '02', title: 'Press Tab', text: 'On the canvas, pick a visual: type a few letters, then Enter.' },
  { n: '03', title: 'Drag Kick onto a slider', text: 'From the sources at the bottom. Now the kick moves it.' },
  { n: '04', title: 'Output → a display', text: 'It goes fullscreen there. B blacks it out, F freezes it.' },
]

// sections rise into view once
let seen: IntersectionObserver | undefined
onMounted(() => {
  if (still()) return
  const els = document.querySelectorAll<HTMLElement>('.home [data-reveal]')
  seen = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue
        e.target.classList.add('in')
        seen?.unobserve(e.target)
      }
    },
    { rootMargin: '0px 0px -10% 0px' },
  )
  els.forEach((el) => {
    el.classList.add('pre')
    seen!.observe(el)
  })
})
onBeforeUnmount(() => seen?.disconnect())
</script>

<template>
  <div class="home">
    <!-- the hero: Velo's mark, live -->
    <section class="hero">
      <Beams :root="[0, 0.7]" />
      <div class="veil" />
      <div class="hero-inner">
        <a class="badge" :href="download">
          <span class="badge-new">0.9 beta</span>
          <span>Windows and macOS<span class="badge-more"> · free and open source</span></span>
          <span class="badge-arrow">→</span>
        </a>
        <h1 class="title"><span class="shiny">Sound shapes light.</span></h1>
        <p class="lead">Live visuals played by the music, for VJs, DJs and anyone with a projector. Velo hears what your computer plays and every picture moves with it.</p>
        <div class="actions">
          <a class="btn primary" :href="download">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v11m0 0l-4.5-4.5M12 15l4.5-4.5M5 19h14" /></svg>
            Download
          </a>
          <a class="btn ghost" :href="withBase('/guide/')">Get started</a>
        </div>
      </div>
      <div class="hero-foot">
        <BeatClock />
        <span class="hint">Move the pointer to lean the light. Click for a kick.</span>
      </div>
    </section>

    <!-- what's in it, drifting past -->
    <section class="strip" aria-label="Features">
      <Marquee :items="features" :seconds="60" />
      <Marquee :items="presets" reverse :seconds="70" />
    </section>

    <!-- the patch -->
    <section class="wrap section" data-reveal>
      <div class="head">
        <p class="eyebrow">The patch</p>
        <h2>Wires you can feel.</h2>
        <p class="sub">Every node shows its picture. A signal plugs into a jack at the foot of the card it moves, and cables bundle under a tie you slide along them. Go on: drag the cards, drag the tie.</p>
      </div>
      <div class="patch-frame">
        <Patch />
      </div>
    </section>

    <!-- what it does -->
    <section class="wrap section" data-reveal>
      <div class="head">
        <p class="eyebrow">What it does</p>
        <h2>An instrument, not a timeline.</h2>
        <p class="sub">TouchDesigner’s patching and Resolume’s stage, small enough to learn in five minutes and fast enough for a whole night.</p>
      </div>
      <div class="cards">
        <Spotlight v-for="c in cards" :key="c.title" :glow="c.glow" class="card">
          <svg class="icon" viewBox="0 0 24 24" aria-hidden="true" :style="{ color: `rgb(${c.glow})` }"><path :d="c.icon" /></svg>
          <h3>{{ c.title }}</h3>
          <p>{{ c.text }}</p>
        </Spotlight>
      </div>
      <div class="stats">
        <div v-for="s in stats" :key="s.label" class="stat">
          <span class="num"><CountUp :to="s.to" :suffix="s.suffix" /></span>
          <span class="label">{{ s.label }}</span>
        </div>
      </div>
    </section>

    <!-- AI -->
    <section class="wrap section split" data-reveal>
      <div class="head left">
        <p class="eyebrow">Builds with AI</p>
        <h2>Describe it. Watch it build.</h2>
        <p class="sub">The AI panel turns a few words, and pictures as references if you like, into a scene made of Velo’s own nodes, one undo step for all of it. Claude Code and Codex use their own sign-in; Ollama and LM Studio run on your computer; OpenAI, Anthropic, Gemini, OpenRouter and Groq take a key.</p>
        <p class="sub">Velo is an MCP server too: Claude Code, Claude Desktop or Cursor can drive it from outside.</p>
        <div class="actions left">
          <a class="btn ghost" :href="withBase('/guide/ai')">Connect an AI</a>
        </div>
      </div>
      <BorderBeam class="ai-frame">
        <AiDemo />
      </BorderBeam>
    </section>

    <!-- screenshots -->
    <section class="wrap section" data-reveal>
      <div class="head">
        <p class="eyebrow">See it</p>
        <h2>Calm on the laptop, loud on the wall.</h2>
        <p class="sub">A dark interface that stays out of the way of the picture. Click one to look closer.</p>
      </div>
      <Gallery :shots="shots" />
    </section>

    <!-- five minutes -->
    <section class="wrap section" data-reveal>
      <div class="head">
        <p class="eyebrow">Five minutes</p>
        <h2>From silence to a projector.</h2>
      </div>
      <ol class="steps">
        <li v-for="s in steps" :key="s.n">
          <span class="step-n">{{ s.n }}</span>
          <h3>{{ s.title }}</h3>
          <p>{{ s.text }}</p>
        </li>
      </ol>
      <p class="more"><a :href="withBase('/guide/five-minutes')">The five minutes, step by step →</a></p>
    </section>

    <!-- the close -->
    <section class="closing">
      <Beams :root="[0, 0.9]" :spread="0.55" :budget="600000" />
      <div class="veil closing-veil" />
      <div class="closing-inner" data-reveal>
        <h2 class="title small"><span class="shiny">Play some music.</span></h2>
        <p class="lead">Velo is free and open source. Download it, or build it from the sources.</p>
        <div class="actions">
          <a class="btn primary" :href="download">Download Velo</a>
          <a class="btn ghost" :href="repo">GitHub</a>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.home {
  --wrap: 1180px;
  overflow-x: clip;
}

.wrap {
  max-width: var(--wrap);
  margin: 0 auto;
  padding: 0 24px;
}

.section {
  padding-top: 120px;
}

/* the hero */
.hero {
  position: relative;
  display: grid;
  place-items: center;
  min-height: max(640px, 100svh);
  margin-top: calc(-1 * var(--vp-nav-height));
  overflow: hidden;
  background: #000;
}

.veil {
  position: absolute;
  inset: 0;
  pointer-events: none;
  background:
    radial-gradient(ellipse 48% 34% at 50% 44%, rgba(0, 0, 0, 0.5), transparent 70%),
    linear-gradient(to bottom, rgba(0, 0, 0, 0.5), transparent 18%, transparent 72%, var(--velo-bg));
}

.hero-inner {
  position: relative;
  z-index: 2;
  max-width: 1200px;
  padding: 0 20px;
  text-align: center;
  transform: translateY(-7vh);
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  padding: 5px 14px 5px 5px;
  border: 1px solid rgba(255, 236, 214, 0.16);
  border-radius: 999px;
  background: rgba(12, 10, 9, 0.5);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  color: var(--velo-text);
  font-size: 13px;
  text-decoration: none;
  transition: border-color 0.25s;
}

.badge:hover {
  border-color: rgba(244, 120, 96, 0.6);
}

.badge-new {
  white-space: nowrap;
  padding: 2px 9px;
  border-radius: 999px;
  background: var(--velo-orange);
  color: #1a0d08;
  font-weight: 600;
}

.badge-arrow {
  color: var(--velo-muted);
  transition: transform 0.25s;
}

.badge:hover .badge-arrow {
  transform: translateX(3px);
}

.title {
  margin: 22px 0 18px;
  font-size: clamp(46px, 9.2vw, 124px);
  font-weight: 700;
  line-height: 0.96;
  letter-spacing: -0.05em;
  filter: drop-shadow(0 4px 30px rgba(0, 0, 0, 0.55));
}

.title.small {
  font-size: clamp(40px, 7vw, 88px);
}

.lead {
  max-width: 620px;
  margin: 0 auto;
  font-size: clamp(16px, 1.6vw, 19px);
  line-height: 1.6;
  color: #d8cfc2;
  text-shadow: 0 1px 16px rgba(0, 0, 0, 0.8);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 12px;
  margin-top: 34px;
}

.actions.left {
  justify-content: flex-start;
  margin-top: 26px;
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 46px;
  padding: 0 24px;
  border-radius: 999px;
  font-size: 15px;
  font-weight: 600;
  text-decoration: none;
  transition: transform 0.25s var(--velo-ease), box-shadow 0.3s, background-color 0.25s, border-color 0.25s;
}

.btn svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.btn.primary {
  background: var(--velo-orange);
  color: #1a0d08;
  box-shadow: 0 0 0 1px rgba(255, 170, 140, 0.4) inset, 0 8px 32px rgba(244, 120, 96, 0.35);
}

.btn.primary:hover {
  background: #ff8b72;
  box-shadow: 0 0 0 1px rgba(255, 200, 180, 0.6) inset, 0 10px 44px rgba(244, 120, 96, 0.55);
  transform: translateY(-1px);
}

.btn.ghost {
  border: 1px solid rgba(255, 236, 214, 0.22);
  background: rgba(12, 10, 9, 0.45);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  color: var(--velo-light);
}

.btn.ghost:hover {
  border-color: rgba(255, 236, 214, 0.45);
  transform: translateY(-1px);
}

.hero-foot {
  position: absolute;
  z-index: 2;
  left: 0;
  right: 0;
  bottom: 28px;
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  align-items: center;
  gap: 10px 16px;
  padding: 0 20px;
}

.hint {
  color: rgba(236, 229, 218, 0.55);
  font-size: 13px;
}

/* the strip */
.strip {
  display: grid;
  gap: 12px;
  padding-top: 8px;
}

/* section heads */
.head {
  max-width: 720px;
  margin: 0 auto 48px;
  text-align: center;
}

.head.left {
  margin: 0;
  text-align: left;
}

.eyebrow {
  margin: 0 0 12px;
  color: var(--velo-orange);
  font-family: var(--vp-font-family-mono);
  font-size: 12.5px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}

h2 {
  margin: 0;
  font-size: clamp(32px, 4.4vw, 52px);
  font-weight: 700;
  line-height: 1.05;
  letter-spacing: -0.035em;
  color: var(--velo-light);
  text-wrap: balance;
}

.sub {
  margin: 18px 0 0;
  font-size: 17px;
  line-height: 1.65;
  color: var(--velo-muted);
}

/* the patch */
.patch-frame {
  padding: 8px;
  border: 1px solid var(--velo-line);
  border-radius: 22px;
  background: radial-gradient(ellipse at 30% 0%, rgba(155, 225, 93, 0.06), transparent 60%), #0e0d0c;
}

/* cards */
.cards {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
}

.card {
  padding: 26px 24px 24px;
}

.icon {
  width: 26px;
  height: 26px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
  filter: drop-shadow(0 0 8px currentColor);
}

.card h3 {
  margin: 18px 0 8px;
  font-size: 18px;
  font-weight: 650;
  letter-spacing: -0.01em;
  color: var(--velo-light);
}

.card p {
  margin: 0;
  font-size: 14.5px;
  line-height: 1.6;
  color: var(--velo-muted);
}

.stats {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  margin-top: 16px;
  border: 1px solid var(--velo-line);
  border-radius: 18px;
  background: var(--velo-card);
}

.stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 28px 12px;
}

.stat + .stat {
  border-left: 1px solid var(--velo-line);
}

.num {
  display: block;
  line-height: 1.15;
  font-size: clamp(32px, 4vw, 46px);
  font-weight: 700;
  letter-spacing: -0.04em;
  background: linear-gradient(180deg, #fffaf2, #f7b48e);
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}

.stat .label {
  color: var(--velo-muted);
  font-size: 13.5px;
  text-align: center;
}

/* AI */
.split {
  display: grid;
  grid-template-columns: 1fr 1.05fr;
  gap: 56px;
  align-items: center;
}

/* steps */
.steps {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 16px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.steps li {
  position: relative;
  padding: 24px 22px;
  border: 1px solid var(--velo-line);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(255, 240, 220, 0.03), transparent), var(--velo-card);
}

.step-n {
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
  color: var(--velo-green);
}

.steps h3 {
  margin: 12px 0 6px;
  font-size: 17px;
  font-weight: 650;
  color: var(--velo-light);
}

.steps p {
  margin: 0;
  font-size: 14.5px;
  line-height: 1.55;
  color: var(--velo-muted);
}

.more {
  margin: 28px 0 0;
  text-align: center;
}

.more a {
  color: var(--vp-c-brand-1);
  font-weight: 500;
  text-decoration: none;
}

.more a:hover {
  text-decoration: underline;
}

/* the close */
.closing {
  position: relative;
  display: grid;
  place-items: center;
  min-height: 620px;
  margin-top: 140px;
  overflow: hidden;
  background: #000;
}

.closing-veil {
  background:
    radial-gradient(ellipse 50% 40% at 50% 50%, rgba(0, 0, 0, 0.45), transparent 70%),
    linear-gradient(to bottom, var(--velo-bg), transparent 30%, transparent 80%, var(--velo-bg));
}

.closing-inner {
  position: relative;
  z-index: 2;
  padding: 0 20px;
  text-align: center;
  transform: translateY(-40px);
}

/* rising into view */
[data-reveal].pre {
  opacity: 0;
  transform: translateY(28px);
  transition: opacity 0.9s var(--velo-ease), transform 1s var(--velo-ease);
}

[data-reveal].pre.in {
  opacity: 1;
  transform: none;
}

.closing-inner[data-reveal].pre.in {
  transform: translateY(-40px);
}

@media (max-width: 960px) {
  .cards {
    grid-template-columns: 1fr 1fr;
  }
  .split {
    grid-template-columns: 1fr;
    gap: 36px;
  }
  .steps {
    grid-template-columns: 1fr 1fr;
  }
  .stats {
    grid-template-columns: repeat(3, 1fr);
  }
  .stat:nth-child(4) {
    border-left: none;
  }
  .stat:nth-child(n + 4) {
    border-top: 1px solid var(--velo-line);
  }
}

@media (max-width: 640px) {
  .section {
    padding-top: 88px;
  }
  .wrap {
    padding: 0 16px;
  }
  .cards,
  .steps {
    grid-template-columns: 1fr;
  }
  .stats {
    grid-template-columns: 1fr 1fr;
  }
  .stat:nth-child(n) {
    border-left: none;
    border-top: 1px solid var(--velo-line);
  }
  .stat:nth-child(even) {
    border-left: 1px solid var(--velo-line);
  }
  .stat:nth-child(-n + 2) {
    border-top: none;
  }
  .stat:last-child {
    grid-column: span 2;
  }
  .hero-inner {
    transform: translateY(-4vh);
  }
  .badge-more {
    display: none;
  }
  .hint {
    display: none;
  }
  .patch-frame {
    padding: 2px;
    border-radius: 14px;
  }
}
</style>
