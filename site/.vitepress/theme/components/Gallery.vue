<script setup lang="ts">
// Screenshots in a grid; one opens large, the arrows step through them, Esc closes.
import { ref } from 'vue'
import { withBase } from 'vitepress'

const props = defineProps<{ shots: { src: string; title: string; alt: string }[] }>()
const box = ref<HTMLDialogElement>()
const open = ref(0)

function show(i: number) {
  open.value = i
  box.value?.showModal()
}

function step(by: number) {
  open.value = (open.value + by + props.shots.length) % props.shots.length
}

function key(e: KeyboardEvent) {
  if (e.key === 'ArrowRight') step(1)
  if (e.key === 'ArrowLeft') step(-1)
}

// a click on the backdrop, outside the picture, closes it
function outside(e: MouseEvent) {
  if (e.target === box.value) box.value?.close()
}
</script>

<template>
  <div class="gallery">
    <button v-for="(s, i) in shots" :key="s.src" class="shot" :class="{ wide: i === 0 }" type="button" @click="show(i)">
      <img :src="withBase(s.src)" :alt="s.alt" loading="lazy" decoding="async" width="1440" height="900" />
      <span class="label">{{ s.title }}</span>
    </button>
  </div>
  <dialog ref="box" class="lightbox" @keydown="key" @click="outside">
    <figure>
      <img :src="withBase(shots[open].src)" :alt="shots[open].alt" width="1440" height="900" />
      <figcaption>
        <strong>{{ shots[open].title }}</strong>
        <span>{{ shots[open].alt }}</span>
      </figcaption>
    </figure>
    <button class="nav prev" type="button" aria-label="Previous screenshot" @click="step(-1)">‹</button>
    <button class="nav next" type="button" aria-label="Next screenshot" @click="step(1)">›</button>
    <button class="close" type="button" aria-label="Close" @click="box?.close()">×</button>
  </dialog>
</template>

<style scoped>
.gallery {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 14px;
}

.shot {
  position: relative;
  overflow: hidden;
  padding: 0;
  border: 1px solid var(--velo-line);
  border-radius: 14px;
  background: #000;
  cursor: zoom-in;
  transition: border-color 0.3s, transform 0.5s var(--velo-ease);
}

.shot.wide {
  grid-column: span 2;
  grid-row: span 2;
}

.shot img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  transition: transform 0.8s var(--velo-ease), opacity 0.3s;
  opacity: 0.9;
}

.shot:hover,
.shot:focus-visible {
  border-color: rgba(255, 200, 150, 0.45);
}

.shot:hover img {
  transform: scale(1.035);
  opacity: 1;
}

.label {
  position: absolute;
  left: 12px;
  bottom: 12px;
  padding: 4px 10px;
  border-radius: 999px;
  background: rgba(8, 7, 6, 0.72);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  color: var(--velo-light);
  font-size: 12.5px;
}

.lightbox {
  width: min(1240px, 94vw);
  max-width: none;
  padding: 0;
  border: 1px solid var(--velo-line-2);
  border-radius: 16px;
  background: #0d0c0b;
  color: var(--velo-text);
  overflow: visible;
}

.lightbox::backdrop {
  background: rgba(0, 0, 0, 0.82);
  backdrop-filter: blur(6px);
}

.lightbox figure {
  margin: 0;
}

.lightbox img {
  display: block;
  width: 100%;
  height: auto;
  border-radius: 16px 16px 0 0;
}

.lightbox figcaption {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  padding: 14px 18px 16px;
  font-size: 14px;
  color: var(--velo-muted);
}

.lightbox figcaption strong {
  color: var(--velo-light);
}

.nav,
.close {
  position: absolute;
  display: grid;
  place-items: center;
  width: 40px;
  height: 40px;
  border: 1px solid var(--velo-line-2);
  border-radius: 50%;
  background: rgba(13, 12, 11, 0.85);
  color: var(--velo-light);
  font-size: 22px;
  line-height: 1;
}

.nav:hover,
.close:hover {
  border-color: var(--velo-orange);
}

.prev {
  left: 12px;
  top: 40%;
}

.next {
  right: 12px;
  top: 40%;
}

.close {
  right: 12px;
  top: 12px;
}

@media (max-width: 760px) {
  .gallery {
    grid-template-columns: 1fr 1fr;
  }
  .shot.wide {
    grid-column: span 2;
    grid-row: auto;
  }
}
</style>
