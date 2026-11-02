<script setup lang="ts">
// The tempo chip of Velo's top bar, on the page's clock: the BPM, a dot that flashes on the
// kick and the four beats of the bar.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { BPM, hear, still } from '../clock'

const root = ref<HTMLElement>()
let frame = 0

onMounted(() => {
  if (still()) return
  const el = root.value!
  const dots = [...el.querySelectorAll<HTMLElement>('.bar i')]
  let beat = -1
  const tick = (now: number) => {
    const m = hear(now / 1000)
    el.style.setProperty('--kick', m.kick.toFixed(3))
    if (m.beat !== beat) {
      beat = m.beat
      dots.forEach((d, i) => d.classList.toggle('on', i === beat))
    }
    frame = requestAnimationFrame(tick)
  }
  frame = requestAnimationFrame(tick)
})

onBeforeUnmount(() => cancelAnimationFrame(frame))
</script>

<template>
  <div ref="root" class="clock" aria-hidden="true">
    <span class="dot" />
    <span class="bpm">{{ BPM.toFixed(1) }} BPM</span>
    <span class="bar"><i class="on" /><i /><i /><i /></span>
  </div>
</template>

<style scoped>
.clock {
  --kick: 0.6;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px 6px 10px;
  border: 1px solid rgba(155, 225, 93, calc(0.18 + var(--kick) * 0.3));
  border-radius: 999px;
  background: rgba(10, 12, 8, 0.55);
  backdrop-filter: blur(10px);
  -webkit-backdrop-filter: blur(10px);
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  color: #d6f5bb;
  box-shadow: 0 0 calc(var(--kick) * 22px) rgba(155, 225, 93, calc(var(--kick) * 0.35));
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--velo-green);
  opacity: calc(0.35 + var(--kick) * 0.65);
  box-shadow: 0 0 calc(4px + var(--kick) * 10px) var(--velo-green);
}

.bar {
  display: inline-flex;
  gap: 4px;
}

.bar i {
  width: 6px;
  height: 6px;
  border-radius: 2px;
  background: rgba(155, 225, 93, 0.2);
  transition: background-color 0.12s;
}

.bar i.on {
  background: var(--velo-green);
}
</style>
