// The site's beat clock: a four-on-the-floor loop at 124 BPM, heard the way Velo hears music
// (0..1 envelopes for the kick, snare and hats, and slower bands), so every moving thing on the
// page plays to the same beat.

export const BPM = 124

export interface Music {
  beats: number // beats since the page opened
  phase: number // 0..1 through the beat
  beat: number // which beat of the bar, 0..3
  kick: number
  snare: number
  hat: number
  bass: number
  level: number
  high: number
  energy: number // builds over eight bars, then drops
}

const smooth = (x: number) => x * x * (3 - 2 * x)

export function hear(seconds: number): Music {
  const beats = (seconds * BPM) / 60
  const n = Math.floor(beats)
  const phase = beats - n
  const beat = n % 4
  // eight bars that build, the drop on the ninth
  const energy = 0.55 + 0.45 * smooth((beats % 32) / 32)
  const kick = Math.exp(-phase * 7) * (beat === 0 ? 1 : 0.8)
  const snare = beat % 2 === 1 ? Math.exp(-phase * 9) : 0
  const eighth = beats * 2
  const hat = Math.floor(eighth) % 2 === 1 ? Math.exp(-(eighth % 1) * 14) : 0.15 * Math.exp(-(eighth % 1) * 20)
  const bass = 0.25 + 0.5 * Math.exp(-phase * 2.5) * energy
  const level = 0.4 + 0.35 * kick * energy + 0.1 * snare
  const high = 0.2 + 0.55 * hat
  return { beats, phase, beat, kick, snare, hat, bass, level, high, energy }
}

/** Whether the visitor asked for less motion. */
export function still(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
}
