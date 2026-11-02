<script setup lang="ts">
// A card lit from where the pointer is: a soft spot inside and its border catching the light.
defineProps<{ glow?: string }>()

function move(e: PointerEvent) {
  const el = e.currentTarget as HTMLElement
  const box = el.getBoundingClientRect()
  el.style.setProperty('--x', `${e.clientX - box.left}px`)
  el.style.setProperty('--y', `${e.clientY - box.top}px`)
}
</script>

<template>
  <div class="spot" :style="glow ? { '--glow': glow } : undefined" @pointermove="move">
    <slot />
  </div>
</template>

<style scoped>
.spot {
  --glow: 255, 205, 160;
  position: relative;
  isolation: isolate;
  overflow: hidden;
  border: 1px solid var(--velo-line);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(255, 240, 220, 0.035), rgba(255, 240, 220, 0.01)), var(--velo-card);
  transition: border-color 0.3s, transform 0.4s var(--velo-ease);
}

.spot::before,
.spot::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  opacity: 0;
  transition: opacity 0.35s;
  pointer-events: none;
}

.spot::before {
  z-index: -1;
  background: radial-gradient(420px circle at var(--x, 50%) var(--y, 0%), rgba(var(--glow), 0.11), transparent 45%);
}

.spot::after {
  padding: 1px;
  background: radial-gradient(240px circle at var(--x, 50%) var(--y, 0%), rgba(var(--glow), 0.7), transparent 60%);
  -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
  -webkit-mask-composite: xor;
  mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
}

.spot:hover::before,
.spot:hover::after {
  opacity: 1;
}

.spot:hover {
  transform: translateY(-2px);
}

@media (prefers-reduced-motion: reduce) {
  .spot,
  .spot:hover {
    transform: none;
  }
}
</style>
