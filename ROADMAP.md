# Velo: Roadmap

## Today

Velo is in beta on Windows and macOS. It hears the computer's sound or any input (bands, drum hits from a small live model, melody, tempo), runs a graph of 64 node kinds per scene on the GPU with 76 shader and 41 effect presets and your own WGSL or ISF shaders, reads the music once for the whole show through shared signals, puts photos, movies and the camera on 3D models, launches scenes on the beat with transitions, and is played with macros, variations, MIDI, the computer's keys, OSC, Ableton Link and MIDI clock, ahead of a slow projector by its lag. It goes out fullscreen, corner-pinned and masked onto a screen that isn't square to the projector, over Spout and over NDI, graded by its Output. An AI panel (Claude Code, Codex, a local or API model) builds scenes from Velo's own nodes, and `velo-mcp` lets any MCP client drive it.

The milestones that got it here, named in some code comments: M1 First Light (a shader on a projector), M2 Hear It (audio becomes motion), M3 Play It (scenes and control), M4 Keep It (your own shaders, bundles, the demo), M5 Talk to It (the AI). M6 Ship It is the release itself.

## Next

- **Trust on install:** a signed Windows installer and a signed, notarized macOS app.
- **Lessons** for the first five minutes, in English and Italian.
- **A stage theme:** a dimmed UI that doesn't light up the performer's face.
- **Show control:** a setlist or cue list; more than one output, each with crop and rotation.
- **Controllers:** profiles for common controllers, MIDI out and LED feedback; OSC out.
- **Inputs:** one app's sound on Windows (macOS has it), several audio inputs at once, a channel pair from a multichannel interface, screen capture; HEIC stills and turned phone movies upright on Windows (macOS has them).
- **Shaders:** shaders and scenes shared as packages.
- **Syphon** on macOS, Spout's counterpart there.
- **Linux:** one app's sound, and video through the system's decoders.

## Later, maybe

Shadows, animation and a file's own materials for meshes, glTF · Warping past the corner pin (a grid, curved screens) and edge blending · recording to a file · DMX and Art-Net out · Pro DJ Link · a web remote · an installation mode (autostart, watchdog, scheduled scenes) · HDR outputs.

Ideas and requests are welcome in the [issues](https://github.com/g1lg1l/velo/issues).
