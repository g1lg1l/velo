import { defineConfig } from 'vitepress'

const repo = 'https://github.com/g1lg1l/velo'
const site = 'https://g1lg1l.github.io/velo/'

export default defineConfig({
  title: 'Velo',
  description: 'Live visuals played by the music, for VJs and anyone with a projector. Sound shapes light.',
  lang: 'en-US',
  base: '/velo/',
  cleanUrls: true,
  appearance: 'force-dark',
  head: [
    ['link', { rel: 'icon', type: 'image/png', sizes: '32x32', href: '/velo/velo-32.png' }],
    ['link', { rel: 'apple-touch-icon', href: '/velo/velo-256.png' }],
    ['meta', { name: 'theme-color', content: '#0b0a09' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:title', content: 'Velo: sound shapes light' }],
    ['meta', { property: 'og:description', content: 'Live visuals played by the music. Open source, built in Rust, for Windows and macOS.' }],
    ['meta', { property: 'og:image', content: `${site}og.jpg` }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
  ],
  themeConfig: {
    logo: { src: '/velo-64.png', alt: 'Velo' },
    nav: [
      { text: 'Guide', link: '/guide/', activeMatch: '/guide/' },
      { text: 'Download', link: `${repo}/releases/latest` },
    ],
    sidebar: {
      '/guide/': [
        {
          text: 'Start',
          items: [
            { text: 'Introduction', link: '/guide/' },
            { text: 'Install', link: '/guide/install' },
            { text: 'Five minutes', link: '/guide/five-minutes' },
          ],
        },
        {
          text: 'Patching',
          items: [
            { text: 'The canvas', link: '/guide/canvas' },
            { text: 'Signals and jacks', link: '/guide/signals' },
            { text: 'Cable ties', link: '/guide/cable-ties' },
          ],
        },
        {
          text: 'Playing live',
          items: [
            { text: 'Scenes and launching', link: '/guide/scenes' },
            { text: 'Macros and variations', link: '/guide/macros' },
            { text: 'MIDI and OSC', link: '/guide/midi-osc' },
            { text: 'Outputs', link: '/guide/outputs' },
          ],
        },
        {
          text: 'Going deeper',
          items: [
            { text: 'Shaders', link: '/guide/shaders' },
            { text: 'AI', link: '/guide/ai' },
            { text: 'Shortcuts', link: '/guide/shortcuts' },
          ],
        },
      ],
    },
    socialLinks: [{ icon: 'github', link: repo }],
    search: { provider: 'local' },
    outline: { level: [2, 3], label: 'On this page' },
    editLink: { pattern: `${repo}/edit/main/site/:path`, text: 'Edit this page on GitHub' },
    docFooter: { prev: 'Previous', next: 'Next' },
    footer: {
      message: 'The app is GPL-3.0-or-later; its libraries are MIT or Apache-2.0.',
      copyright: 'Velo. Sound shapes light.',
    },
  },
})
