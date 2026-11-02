<script setup lang="ts">
// A row of words drifting past, twice over so it never ends; it waits while pointed at.
withDefaults(defineProps<{ items: string[]; reverse?: boolean; seconds?: number }>(), { seconds: 50 })
</script>

<template>
  <div class="marquee" :class="{ reverse }" :style="{ '--time': `${seconds}s` }">
    <div class="track">
      <span v-for="(item, i) in [...items, ...items]" :key="i" class="chip" :aria-hidden="i >= items.length ? 'true' : undefined">
        {{ item }}
      </span>
    </div>
  </div>
</template>

<style scoped>
.marquee {
  overflow: hidden;
  -webkit-mask: linear-gradient(90deg, transparent, #000 10%, #000 90%, transparent);
  mask: linear-gradient(90deg, transparent, #000 10%, #000 90%, transparent);
}

.track {
  display: flex;
  gap: 12px;
  width: max-content;
  animation: drift var(--time) linear infinite;
}

.reverse .track {
  animation-direction: reverse;
}

.marquee:hover .track {
  animation-play-state: paused;
}

.chip {
  flex: none;
  padding: 8px 16px;
  border: 1px solid var(--velo-line);
  border-radius: 999px;
  background: rgba(255, 240, 220, 0.025);
  color: var(--velo-muted);
  font-size: 14px;
  white-space: nowrap;
  transition: color 0.2s, border-color 0.2s;
}

.chip:hover {
  color: var(--velo-light);
  border-color: var(--velo-line-2);
}

@keyframes drift {
  to {
    transform: translateX(calc(-50% - 6px));
  }
}

@media (prefers-reduced-motion: reduce) {
  .marquee {
    -webkit-mask: none;
    mask: none;
  }
  .track {
    animation: none;
    flex-wrap: wrap;
    justify-content: center;
    width: auto;
  }
  .chip[aria-hidden] {
    display: none;
  }
}
</style>
