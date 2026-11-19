# Improvement plan (study of 2026-11-19, v0.9.8-beta)

A study of performance, robustness and upkeep, meant to be worked through in order. Every item names
the code it is about; "measured" means it was run, everything else was read in the code and checked
at the line given, not timed.

## What was measured

Release build, Apple M5 Pro, the two real shows (camera nodes removed), canvas 1024x768, synth audio,
`VELO_DEMO_PERF=1`:

| Run | Engine | Render-thread frame | UI frame (avg / worst after start) | Peak memory footprint |
|---|---|---|---|---|
| techno-night scene 3 (FRACTAL) | 58-60 fps | 2.2-4.3 ms | 2.0-2.4 / 4.6 ms | 1.65 GB |
| techno-night scene 11 (MOTHER BUBBLE) | **52-56 fps** | 16-25 ms | 2.1-2.9 / 5.5 ms | 1.77 GB |
| techno-night scene 5 (STROBE, 2 nodes) | 60 fps | 1.7-3.6 ms | 1.9-2.0 / 2.9 ms | 1.55 GB |
| cumple-ta-fe scene 1 (73 nodes) | 60 fps | 1.8-7.3 ms | **3.6-5.8** / 14.6 ms | 2.14 GB |

- Every techno-night run drops to 52-58 fps with a 17-19 ms frame around t = 12 s, even the 2-node
  scene. Not explained yet.
- `scene_costs` (GPU per scene): techno-night 44 ms for all 18 scenes, FRACTAL 11.3 ms, MOTHER BUBBLE
  10.2 ms; cumple-ta-fe "Tutto insieme" 20.7 ms. On a weaker GPU or at 1080p these scenes will miss 60 fps.
- The per-node GPU times look wrong on Apple GPUs: nodes in one chain report nearly the same value
  (1.72 / 1.72 / 1.72, 3.32 / 3.27 / 3.26). Likely the passes overlap and each timestamp pair spans
  the chain, so the per-node split (and the "GPU ms per node" log) can't be trusted there.
- A 2-node scene still runs ~8 movie and 5 media-list reader threads (the other layers are on), and
  CoreMedia threads come and go: a movie that jumps on every kick opens a new `AVURLAsset` and reader
  per jump (`velo-engine/src/video.rs:150`).
- CI: 7 of the last 15 runs on `main` are red, only on Windows, only for timing tests:
  `gpu::tests::a_media_lists_movie_plays_frame_by_frame`, `gpu::tests::hap_movies_play_their_frames_on_the_gpu`,
  `live::tests::kept_pictures_come_back_and_the_least_recently_used_go`. The release workflow doesn't
  wait for CI.

How to measure again: unzip a `.velo`, replace `media: Some("camera:...")` with `media: None` in
`project.ron`, zip it back, then
`VELO_DATA=<tmp> VELO_AUDIO=synth VELO_DEMO_PERF=1 VELO_DEMO_SCENE=<n> VELO_CAPTURE_AT=13 VELO_CAPTURE_TO=<png> target/release/velo <file>`
and `VELO_PROFILE=<file> cargo test -p velo scene_costs -- --ignored --nocapture`.

## Phase 1: live safety and a green CI (small changes, one PR)

Done on branch `fix/live-safety`, except in item 6: `panel` is still what the client says in hello,
and an agent's `undo` isn't checked against the live scene (an agent holding the token can read the
user's files anyway; the guard is against honest mistakes, which the shared-signals and canvas
holes were).

1. **Windows CI flakes.** Make the three tests above independent of wall-clock timing: the two movie
   tests sleep 1/60 s and expect frames to have changed; the shots test sets file times that tie on a
   coarse clock (`velo/src/live.rs:1551`), so `keep_shot` may keep the wrong file.
2. **Autosave stops for the whole session** once the "Restore unsaved work?" dialog is closed
   (`velo/src/files.rs:609,624,700`): a crash then loses everything since the last save. Move the old
   snapshot aside and keep autosaving. Autosave failures are silent (`files.rs:613-619`): show a toast.
3. **Two instances share `session.lock` and `autosave.velo`** (`files.rs:26-28,368-372,508-510,632-636`):
   the second deletes the first's snapshot. Name them per process or refuse a second instance.
4. **OSC listens on every interface and `/velo/op/<id>` runs any operator** with no token
   (`velo/src/osc.rs:159,298-304`): anyone on the venue Wi-Fi can delete the live scene. Bind
   127.0.0.1 unless a "listen on the network" preference is on; put `/velo/op` behind its own preference.
5. **Agent server reads the first line with no size cap or timeout**, one thread per connection
   (`velo/src/agent.rs:119,139-140`). Set a read timeout and read through `take(limit)`.
6. **Agent "may change the live scene" guard has holes** (`agent.rs:149,229-231,358,409-415`): `panel`
   is self-declared in hello, `edit.undo`/`redo` aren't checked, `project.set_canvas` and
   `scene: "shared"` change what the live scene shows.
7. **0 Hz monitor panics** (`velo/src/host.rs:255-256`): `.filter(|&m| m > 0)` before `unwrap_or(60_000)`.
8. **HAP sizes from the file drive allocations** (`velo-engine/src/hap.rs:79,181,248`): a damaged
   `.mov` aborts the app. Cap `count` by the durations, reject a Snappy length above the frame size.
9. **One bad field resets all settings silently** (`velo/src/main.rs:442`) and a bad chats file is
   overwritten (`velo/src/chat.rs:399,409-410`). Keep the old text as `.bak` and say so.
10. **Audio input of a multi-channel interface is quiet**: mono = sum / channel count
    (`velo-audio/src/capture.rs:119-126`), so stereo music on an 8-channel card reads -12 dB against
    absolute gates. Average the first two channels (or let the user pick).
11. **Audio features drop to zero on a device change**: each reopen stores zeros and a new `Tracker`,
    and the old thread writes zeros again about 1 s later over the new one
    (`velo/src/live.rs:1297,1300,1324,1349-1358`). Keep the `Tracker`, skip the zero store when stopping.
12. **Audio clocks stall after ~5-9 h** of one input (f32 accumulation, `velo-audio/src/features.rs:90-92`).
    Accumulate in f64.
13. **File input: a dead output stream freezes the features** while the bar says Live
    (`velo-audio/src/track.rs:128`, `velo/src/live.rs:1461-1481`). Restart the player as `hear` does.

## Phase 2: GPU quick wins (each small, rerun the GPU tests)

Done on `perf/engine`, except item 6 (the Output node's identity pass): it costs ~0.05 ms at
1024x768 here, and skipping it would need its thumbnail and `picture()` rerouted. Item 8: the
frame drops in the first ~12 s are the start-up building and picturing every scene's card (the
render thread waits on the GPU); a normal start does that behind the loading screen, and the rate
holds at 60 after it. `scene_costs` now scales overlapping spans as the live timing does.

1. **Bypassed effects still cost their full shader**: `velo_fs` always runs `main_image`, then mixes
   by `amount` / multiplies by `opacity` (`velo-engine/src/shader.rs:373-376`). Return early at 0.
2. **Media List / AI Image render all 3 slot images every frame** though only the shown one (and the
   fading one) is sampled (`velo-engine/src/compile.rs:394`, `runtime.rs:1177`). Add them to `lazy`.
3. **Shader nodes (all presets) recompile on every structural edit**: no cache by source
   (`compile.rs:315`), and each compile parses twice (`gpu.rs:71,81`). Cache `Program` by code hash.
4. **Mesh MSAA color and depth are not transient** (`gpu.rs:768`): ~100 MB at 1080p per Mesh node.
   Add `TextureUsages::TRANSIENT_ATTACHMENT` (memoryless on Apple).
5. **Particles always dispatch and draw 65,536** whatever `count` is (`gpu.rs:528,567`).
6. **The Output node is an identity pass at defaults** (`compile.rs:164`): skip it then.
7. **A movie jump reopens the file** (`video.rs:150`): keep the `AVURLAsset` on the clip.
8. Find the 12-s frame spike above, and make per-node timing trustworthy on Apple GPUs.

## Phase 3: UI cost (grows with project size)

1. **Repaint policy**: the UI rebuilds every 16 ms while the engine exists, even when nothing moves
   (`velo/src/main.rs:4099`); the chat's prompt beam and empty-chat orb ask for an immediate repaint
   every frame (`chat.rs:826,1329`, also `widgets.rs:1768`), so the UI runs at 120 Hz on ProMotion
   while the chat is open. Repaint at 16 ms only while a preview or meter is shown, slower when unfocused.
2. **Canvas text sizes aren't rounded** (`velo/src/canvas.rs:1292,1364,1539,1627,1635,1648,1800-1835,2254,3085,3559`):
   every zoom frame rasterizes a new glyph set; the atlas grows and is cleared. Route them through `font()`.
3. **Autosave serializes and fsyncs on the UI thread** every 60 s (`files.rs:607-619`). Write on a worker.
4. **The document is copied and compared every frame**, and on every undoable edit:
   `scenes_rail` clones all scenes (`main.rs:1137`), `Session::exec` clones the whole project and
   compares it, on every drag frame too (`velo-core/src/lib.rs:1318,1326`), undo keeps up to 500 full
   copies (`lib.rs:1284`), `dirty()` deep-compares every frame (`velo/src/shell.rs:206`), `Live::sync`
   rehashes and diffs every held slot every frame (`live.rs:364,422,441-485`). One fix covers all:
   `Arc<Scene>` with `Arc::make_mut` in operators, plus a revision counter on `Session` to skip diffs.
   Biggest change in this plan: touches velo-core, needs the operator tests.
5. **Chat lays out and re-parses every message every frame** (`chat.rs:856-891`, `markdown.rs:207,298`).
   Cache parsed blocks per item, skip items outside the viewport.
6. **Node canvas rebuilds layout every frame with linear lookups** (`canvas.rs:1883,1923-1937,2005-2126`,
   `live.rs:672,685,697,713`): give `Live` a node-id index, cache rects and wires per revision.
7. **Code editor re-lexes the whole file every frame** and draws every line number (`code.rs:88-160`).
8. Blocking I/O on the UI thread: chat picture decode (`chat.rs:598-608`), `save_chats` with fsync
   (`chat.rs:404-421`), agent snapshot PNG encode (`agent.rs:505-518`), `keep_shot`
   (`live.rs:876-892`), recent files stat every frame on the start screen (`shell.rs:1114,1217`).

## Phase 4: larger engine work

1. **A rebuild reuses nothing**: new canvas textures for every node, every still re-uploaded, every
   movie reopened, old and new graph both alive until install (2x VRAM), and a text or file change
   costs two rebuilds (`gpu.rs:287-383`, `media.rs:448-503`, `velo/src/live.rs:355-359,949`). Share
   stills in the `Library`; swap text and files with `Runner::set_file` instead of rebuilding.
2. **Video frames go through a fresh wgpu staging buffer** per frame on the render thread
   (`media.rs:584`, 8 MB a frame at 1080p). Fill a mapped buffer on the reader thread, record only the copy.
3. **Thumbnails**: one render pass, bind group and uniform write per node, 10 times a second
   (`runtime.rs:1351,1725-1760`), and the UI copies and uploads the whole 3.5 MB sheet each time
   (`live.rs:517-548`). One pass with viewports; upload only the rows in use, skip when hidden.
4. **Blit and Warp pipelines are built on the render thread** when an output is attached or shared
   (`runtime.rs:641,962,1280,1332`): a hitch on every output. Build them at start per format.
5. **Audio at 96/192 kHz isn't resampled**: hop and windows are in samples, the model was trained at
   44.1/48 kHz (`velo-audio/src/analysis.rs:16`, `drums.rs:6-7`, `spectrum.rs:8`). Decimate first.
6. Smaller: device polling on the analysis thread every 2 s (`live.rs:1340-1361`); `push_spectrum`
   smoothing depends on the device buffer size (`velo-engine/src/runtime.rs:122-148`); one
   `write_buffer` per mapped node per frame (`runtime.rs:1139`); scratch `Vec`s per frame in `needed()`
   and the media list (`gpu.rs:583-594`, `media.rs:827-1044`).

## Upkeep

- `cargo fmt --check` differs in 30 files and CI doesn't run it.
- Very long functions: `node_canvas` 1,832 lines (`canvas.rs:1873`), `Thread::run` 595 lines
  (`runtime.rs:822`), `main.rs` 4,192 lines. Splitting them helps compile time and makes the AI read
  less code per change.
- Release builds don't wait for a green CI.

## Already fine (don't re-check)

One encoder and one submit per frame, nothing allocated in `render_timed`, pipelines and bind groups
built on the compile worker, built-in shaders cached by name, parameter changes are one buffer write,
unconnected and hidden nodes skipped, all readbacks async, decode on reader threads, graph drops on a
worker, wgpu errors and device loss restart the engine, project files have a format number and
migrations, writes are tmp + fsync + rename, i18n lookups don't allocate, MIDI/OSC coalesced, network
calls have timeouts and size limits, audio callback has no lock or allocation, audio reaches the
engine through atomics.
