<script setup lang="ts">
// A beam of light running round a card's border, as a scene card's ring counts down a launch.
withDefaults(defineProps<{ seconds?: number }>(), { seconds: 7 })
</script>

<template>
  <div class="framed" :style="{ '--turn': `${seconds}s` }">
    <slot />
  </div>
</template>

<style>
@property --beam-angle {
  syntax: '<angle>';
  initial-value: 0deg;
  inherits: false;
}
</style>

<style scoped>
.framed {
  position: relative;
  border-radius: 22px;
  border: 1px solid var(--velo-line);
  background: var(--velo-card);
}

.framed::before {
  content: '';
  position: absolute;
  inset: -1px;
  z-index: 1;
  border-radius: inherit;
  padding: 1.5px;
  background: conic-gradient(
    from var(--beam-angle),
    transparent 0turn,
    transparent 0.62turn,
    rgba(244, 120, 96, 0.9) 0.8turn,
    #fff1df 0.86turn,
    rgba(155, 225, 93, 0.8) 0.9turn,
    transparent 0.93turn
  );
  -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
  -webkit-mask-composite: xor;
  mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  animation: velo-beam var(--turn) linear infinite;
  pointer-events: none;
}

@keyframes velo-beam {
  to {
    --beam-angle: 360deg;
  }
}

@media (prefers-reduced-motion: reduce) {
  .framed::before {
    animation: none;
    --beam-angle: 200deg;
  }
}
</style>
