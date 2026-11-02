<script setup lang="ts">
// A number that counts up the first time it comes into view.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { still } from '../clock'

const props = withDefaults(defineProps<{ to: number; suffix?: string; seconds?: number }>(), { suffix: '', seconds: 1.6 })
const shown = ref(props.to)
const el = ref<HTMLElement>()
let seen: IntersectionObserver | undefined
let frame = 0

onMounted(() => {
  if (still()) return
  shown.value = 0
  seen = new IntersectionObserver(([e]) => {
    if (!e.isIntersecting) return
    seen?.disconnect()
    const start = performance.now()
    const tick = (now: number) => {
      const x = Math.min(1, (now - start) / (props.seconds * 1000))
      shown.value = x < 1 ? Math.round(props.to * (1 - Math.pow(2, -10 * x))) : props.to
      if (x < 1) frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
  })
  seen.observe(el.value!)
})

onBeforeUnmount(() => {
  seen?.disconnect()
  cancelAnimationFrame(frame)
})
</script>

<template>
  <span ref="el" class="count">{{ shown.toLocaleString('en-US') }}{{ suffix }}</span>
</template>

<style scoped>
.count {
  font-variant-numeric: tabular-nums;
}
</style>
