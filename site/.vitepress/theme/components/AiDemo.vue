<script setup lang="ts">
// The AI panel at work: a request typed in, then the operators it runs, one row each.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { still } from '../clock'

const prompt = 'A purple tunnel that pulses on the kick, with neon rings'
const steps = [
  ['scene.add', 'Neon tunnel'],
  ['node.add', 'Shader, the Tunnel preset'],
  ['node.set_param', 'tint → violet'],
  ['node.add', 'Shapes, rings'],
  ['node.add', 'Blend · Feedback'],
  ['edge.connect', 'into the Output'],
  ['mapping.add', 'Kick → Speed'],
  ['snapshot', 'looks at the picture'],
]

const typed = ref(prompt)
const shown = ref(steps.length)
const el = ref<HTMLElement>()
let timer = 0
let seen: IntersectionObserver | undefined

onMounted(() => {
  if (still()) return
  typed.value = ''
  shown.value = 0
  let running = false
  const play = () => {
    if (!running) return
    if (typed.value.length < prompt.length) {
      typed.value = prompt.slice(0, typed.value.length + 1)
      timer = window.setTimeout(play, 34)
    } else if (shown.value < steps.length) {
      shown.value++
      timer = window.setTimeout(play, shown.value === 1 ? 700 : 380)
    } else {
      timer = window.setTimeout(() => {
        typed.value = ''
        shown.value = 0
        play()
      }, 4200)
    }
  }
  seen = new IntersectionObserver(([e]) => {
    running = e.isIntersecting
    clearTimeout(timer)
    if (running) play()
  })
  seen.observe(el.value!)
})

onBeforeUnmount(() => {
  clearTimeout(timer)
  seen?.disconnect()
})
</script>

<template>
  <div ref="el" class="ai" aria-label="The AI panel building a scene from a request" role="img">
    <div class="msg">
      <span class="you">You</span>
      <p>{{ typed }}<span class="caret" /></p>
    </div>
    <ol class="steps">
      <li v-for="(s, i) in steps" :key="i" :class="{ on: i < shown }">
        <span class="tick">✓</span>
        <code>{{ s[0] }}</code>
        <span class="what">{{ s[1] }}</span>
      </li>
    </ol>
    <div class="box">
      <span>Describe a visual…</span>
      <span class="model">Claude Code · Opus 5.5</span>
    </div>
  </div>
</template>

<style scoped>
.ai {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 22px;
  font-size: 14px;
}

.msg {
  align-self: flex-end;
  max-width: 92%;
  padding: 10px 14px;
  border-radius: 14px 14px 4px 14px;
  background: rgba(244, 120, 96, 0.12);
  border: 1px solid rgba(244, 120, 96, 0.25);
}

.you {
  display: block;
  margin-bottom: 2px;
  font-size: 11px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--vp-c-brand-1);
}

.msg p {
  margin: 0;
  min-height: 1.5em;
  color: var(--velo-light);
}

.caret {
  display: inline-block;
  width: 2px;
  height: 1em;
  margin-left: 2px;
  vertical-align: -0.15em;
  background: var(--velo-orange);
  animation: blink 1s steps(1) infinite;
}

.steps {
  margin: 0;
  padding: 0;
  list-style: none;
  display: grid;
  gap: 6px;
  min-height: 268px;
}

.steps li {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  border: 1px solid var(--velo-line);
  border-radius: 8px;
  background: rgba(255, 240, 220, 0.02);
  opacity: 0;
  transform: translateY(6px);
  transition: opacity 0.35s, transform 0.45s var(--velo-ease);
}

.steps li.on {
  opacity: 1;
  transform: none;
}

.tick {
  color: var(--velo-green);
  font-size: 12px;
}

.steps code {
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  color: var(--velo-amber);
}

.what {
  color: var(--velo-muted);
  font-size: 13px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.box {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 14px;
  border: 1px solid var(--velo-line-2);
  border-radius: 12px;
  color: var(--velo-dim);
  font-size: 13px;
}

.model {
  color: var(--velo-muted);
  font-family: var(--vp-font-family-mono);
  font-size: 11.5px;
}

@keyframes blink {
  50% {
    opacity: 0;
  }
}

@media (prefers-reduced-motion: reduce) {
  .caret {
    animation: none;
  }
}
</style>
