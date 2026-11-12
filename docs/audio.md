# Velo: Audio

How Velo hears the music: where the sound comes from, what the analysis measures, and how the tempo is found. The code is `crates/velo-audio` (no GPU, no UI) and the audio thread in `crates/velo/src/live.rs`.

## 1. Inputs

The audio chip in the top bar picks one input. The choice is remembered per machine, not per project.

| Input | What it is |
|---|---|
| **Computer audio** (the default) | Whatever the computer plays: Spotify, a browser, a DJ app. No cables, no virtual device. |
| **Default input** | The system's default input device. |
| **A device** | Any input by name: a mic, an audio interface, a mixer with USB audio, or a virtual device (BlackHole, VB-Cable). |
| **One app** (macOS 14.2+) | Only that app's sound, so notifications and calls stay out. |

Computer audio per platform (through cpal):

| Platform | How |
|---|---|
| Windows | WASAPI loopback of the default output. While nothing plays it sends nothing, and the chip says Silence. |
| macOS 14.6+ | A Core Audio process tap. It needs the system's permission (Screen & System Audio Recording); an unallowed tap delivers silence, not an error. |
| Linux | The PipeWire sink's monitor. |

**One app** (`apps.rs`): the audio menu lists the apps using audio, playing ones first. A process belongs to the outermost `.app` in an Applications folder, so a browser's helpers count as the browser and daemons stay out. Picking one makes a private tap on its processes in a private aggregate device, opened like any input; the app keeps playing out loud. The choice is saved by the app's bundle path: while the app is closed Velo waits for it, and when its processes change the tap is made again. Not yet: Windows (WASAPI process loopback), Linux (PipeWire), Safari (it plays through WebKit's own process).

**Permissions.** On macOS opening an input can block until the permission prompt is answered, so inputs are opened on the audio thread and the chip says "Waiting for permission". An input that delivers only exact zeros for 2 s is flagged, with the Settings pane to open. The app bundle carries `NSAudioCaptureUsageDescription` and `NSMicrophoneUsageDescription`.

**Hot-plug.** A device that goes away is asked for again every second and reconnects by itself; the visuals keep running on silence meanwhile. When the system's default output or input changes, the input that follows it switches too.

The capture is mixed down to mono. Not yet: picking channels on a multichannel interface, several inputs at once.

## 2. Threads and data flow

```
cpal callback               velo audio thread                         readers
+--------------------+ rtrb +---------------------------------+  atomics  +----------------------+
| downmix to mono    | ---> | hops of 256 samples (5.3 ms)    | --------> | render thread:       |
| push into the ring |      | levels, hits, melody, tempo     |           | features, audio tex. |
| no alloc, no lock  |      | features, audio texture, graphs |           | UI: graphs, tempo    |
+--------------------+      +---------------------------------+           +----------------------+
```

- **The callback** only downmixes and pushes into an `rtrb` ring of one second. It never allocates, locks or waits; if the ring is full, samples are dropped.
- **The audio thread** drains the ring into the analyzer, a hop at a time (about 190 hops a second at 48 kHz), so a hit is never lost between frames.
- **Publishing** never blocks a reader: the twelve features the shaders and mappings read are atomics the render thread loads each frame (`SharedFeatures`), the audio texture is another set (`SharedWave`), the Audio In's graphs are atomic maxima the UI samples, and the detection settings come in the other way through atomics.
- When no samples arrive (nothing plays into a loopback, a device stalled) silence is fed in instead, so levels and hits fall back and nothing a mapping moves stays stuck.

The analyzer runs on plain sample slices: tests and the `listen` example run exactly the code the app runs.

## 3. Features

What mappings and shaders read. Every value is 0..1 except the clocks.

| Feature | How |
|---|---|
| **Level, Bass, Mid, High** | Biquad bands: bass under 150 Hz, mid 400 Hz to 5 kHz, high over 6 kHz. Each is its level against a running peak that falls 2 dB a second (auto-gain); a band's reference never sits more than 24 dB under the whole mix's, so a band that goes quiet reads as quiet. Smoothed 5 ms up, 150 ms down. |
| **Kick, Snare, Hat** | An envelope that jumps to 1 on a hit and falls to 1/e in 150 ms. A clap counts as a snare. How hits are found is below. |
| **Melody** | How much of the sound is tune, smoothed 30 ms up, 400 ms down. |
| **Audio time** | `level_time`, `bass_time`, `mid_time`, `high_time` (shader globals): clocks that run at a quarter speed in silence and up to 1.75 times when their band is full. Drive a shader's time with one and its motion pulses with the music without jitter. |
| **Audio texture** | For shaders that read `wave`: 512 columns of the last hops' levels (about 2.7 s), the last samples (a scope) and a spectrum from 30 Hz to 16 kHz in bands spaced by octave. |
| **Silence** | Under -60 dBFS everything falls to 0 instead of auto-gain lifting the hiss. |

Smoothing and thresholds that shape how a parameter moves belong to each mapping (attack, release, amount, invert), not to the feature: one Kick can punch the zoom and nudge the hue softly (graph.md).

### How the hits are found

The Audio In node has two modes.

**Model** (the default): a small network (`drums.rs`, an MLP over 64 mel bands, 246,660 weights) trained on generated electronic drums and real electronic music. It answers three hops (about 16 ms) after a hit starts, having heard its attack whole, for about 4 % of a core, and gives the melody too. A hit fires when its probability rises past its threshold (45, 35 and 55 % for kick, snare and hat to start). Training data, scores and how to train it again: [the drum model](../crates/velo-audio/models/README.md).

**Simple**: a detector per hit, each with its own band, threshold and shortest gap, and a level gate for all three. A hit is its band rising by the threshold (dB) over its last windows; a band under 400 Hz is measured over four hops (about 21 ms), the others over one. It decides in the hop the hit starts, 5 to 10 ms after it, with no lookahead. Defaults: kick 70 to 300 Hz, 3 dB, 100 ms; snare 1 to 8 kHz, 8 dB, 80 ms; hat over 8 kHz, 4 dB, 50 ms; gate -60 dB. A kick's click is broadband, so three rules keep it out of the other two: a snare or a hat must still be there at the end of its hop, a hat within 40 ms of a snare is the snare's noise, and a hat in a hop where a drum's body (130 to 400 Hz) rises 6 dB is a kick's click. A hit must also come near its band's recent loudest, which keeps a rolling bass from passing for a kick. In Simple mode the melody is the energy of the spectrum's peaks between 200 Hz and 4 kHz (`spectrum.rs`, its own radix-2 FFT).

**Tuning.** The calibration is the Audio In's own parameters, in the inspector or dropped down under its card: the mode, the gate, and for each hit a graph of the last 3 s (what its detector watched, its threshold as a line you drag, its hits as dots) with its threshold, band and gap, or in Model mode how sure the model must be. They are saved with the project. The Audio In that sets them is the selected one, else the edited scene's first, else the live scene's (one input, one analyzer). What the model decides carries a small AI badge.

## 4. Tempo

The beat clock's tempo comes from one of: the music (auto-BPM), a tap, a typed value, a MIDI clock or Ableton Link (control.md §4).

**Auto-BPM** (`tempo.rs`). Every half second the last 10 s of onset strength are autocorrelated over 70 to 180 BPM. A belief over tempo keeps those scores, forgetting slowly and blurred a little (tempo drifts), so a steady track gets surer by the second and one odd window can't move it. The onset strength is the bands' rises in Simple mode and the model's kick and snare probabilities in Model mode. Tempos land on the nearest half BPM. The beat is the comb of that period that lines up the most strength, followed halfway each time unless it disagrees twice running (a real change).

In the app, **Follow the music** in the tempo chip's menu shows what it hears and how sure. While on, an estimate at least 30 % sure that agrees with the one before sets the tempo and the beat. It is on by default for computer audio and one app. Setting the tempo by hand (typed, nudged, tapped, resynced) turns it off: that is the lock. A MIDI clock or Link peers lead over it.

Projectors and LED processors add 30 to 100 ms of display latency: the tempo chip's Projector lag draws everything the beat clock drives that much ahead (control.md §4); what the analysis hears can only follow. Not yet: a user tempo range, a regression set of real tracks in CI.

## 5. Latency (48 kHz)

| Stage | Typical |
|---|---|
| Device buffer (256 frames asked for) | 5.3 ms |
| Hit decision | 5 to 10 ms in Simple mode; 20 to 25 ms in Model mode (it waits three hops) |
| Wait for the next frame (60 fps, average) | 8 ms |
| Render and present | 16 ms |
| Display or projector | 10 to 100 ms, out of Velo's hands |

With computer audio Velo hears the sound before the speakers play it; Bluetooth speakers add 100 to 300 ms, and the visuals then run ahead. Not yet: a visual delay setting to match them.

## 6. Testing

- `cargo test -p velo-audio` scores the detectors and the tempo tracker against a synthetic techno loop with known hit times (`synth.rs`), at several sample rates.
- The `listen` example runs the app's analysis on a live input, the test loop or a file, and prints or writes CSV:
  ```sh
  cargo run -p velo-audio --release --example listen                      # computer audio
  cargo run -p velo-audio --release --example listen -- --synth           # the test loop, scored
  cargo run -p velo-audio --release --example listen -- --file take.wav --model
  ```
- `VELO_AUDIO=synth` runs the app on the test loop at 128 BPM, with no device and no permission.

## 7. Crates

`cpal` (device I/O and loopback) and `rtrb` (the callback's ring); on macOS the `objc2` Core Audio bindings for one app's tap. The FFTs, filters, detectors, drum network and tempo tracker are Velo's own code, a few thousand lines with no C dependency.
