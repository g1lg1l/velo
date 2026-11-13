# Velo: Rendering

How the picture is made and where it goes. The code is in `crates/velo-engine` (`runtime.rs`, `gpu.rs`, `compile.rs`, `media.rs`, `share.rs`) and the output window in `crates/velo/src/host.rs`.

## 1. Decisions

| Question | Decision | Why |
|---|---|---|
| GPU API | **wgpu** (Metal, Vulkan, DX12) | One API, three native backends, WGSL validated by naga, compute |
| Who renders | **A render thread** owns the frame loop and the output surfaces | The UI thread can block (a file dialog, a heavy layout); the output must not |
| One GPU device or two | **Two**: the engine's own on the render thread, eframe's for the UI | With one shared device wgpu 30 fails a surface reconfigure whenever the other thread submits, and eframe reconfigures on every resize. Two devices also keep a UI fault away from the output (ARCHITECTURE.md D16) |
| Working format | **`Rgba16Float`** textures | Feedback trails and glow without banding |
| Color | **Display-referred values**, no implicit conversion; the output surface takes a non-sRGB format so values go out untouched | What VJ, Shadertoy and ISF shaders assume |
| Pacing | **The output's vsync** (`Fifo`, `desired_maximum_frame_latency = 1`); a 60 Hz tick when no output takes frames | Lowest latency without tearing; `Fifo` works everywhere |
| Shader language | **WGSL**; ISF's GLSL translated by naga | wgpu-native and validated |

## 2. Threads

```
main thread (winit, egui)        render thread                     compile thread
┌───────────────────────┐ Command ┌───────────────────────────┐    ┌──────────────────────┐
│ UI window             │ ──────► │ frame loop (§3)           │    │ shaders, pipelines,  │
│ creates the output    │         │ graphs, output surface,   │    │ textures: built with │
│ window and surface    │ ◄────── │ preview, shares           │    │ the engine's device  │
│ shows the preview     │ Status  └───────────────────────────┘    └──────────────────────┘
└───────────────────────┘
```

- Windows **and their surfaces** are created on the main thread; the render thread configures and presents them. The output window's resize and occlusion come to the engine as commands.
- **The main thread never waits on the render thread.** On macOS, winit window calls hop to the main thread synchronously, so a main thread blocked on the render thread would deadlock. Everything from main to render is a fire-and-forget message.
- Built graphs come back to the UI on a channel and go to the render thread with `Command::Install`.

## 3. Frame loop

```
loop {
    drain commands: graphs installed, values, launches, the output attached or resized
    pace: the output's next frame, else sleep to a 60 Hz tick
    step launches on the beat clock; read the audio features; advance the clock
    signal nodes, then macros and mappings → each node's parameter values
    one render pass per node of the live graph (and of a fading or edited one)
    blit the program to the output (fit, blackout, freeze, test pattern), the shares and the preview
    submit and present; at most two frames on the GPU
    publish status: frame time, late frames, stalls, each node's GPU cost
}
```

**No compiling, no pipeline creation, no pixel allocation in the loop.** That happens on the compile thread (§7). The loop binds, draws and writes small buffers: the globals `g` once a frame and each node's parameters.

**Preview:** the program is drawn at 640×360 into a preview texture, copied into one of three `MAP_READ` buffers and mapped with `map_async`. The render thread polls without blocking and the UI thread copies the newest into an egui texture, so the render thread never waits and never copies pixels.

## 4. Media

Pictures from files feed Image, Video, Media List, Text and Particles nodes. Each file loads once on the media thread; a graph built while a file loads shows black there and is built again once it's in. A Media List loads on a thread of its own instead, the most pressing file first: three Image nodes of its own are its slots, turn k's file on slot k % 3. Streamed (its Load parameter, the default), the next turn's file loads during this one and only the turn before (fading out), this one and the next are in memory; each file plays from its start on its turn. In memory, every file loads and stays, so no turn waits, and each plays on from where its last turn left it. A rebuilt graph takes over what the old one had in. The list counts its turns on the CPU, a movie's lasting Movie every and a picture's Every: a round (each file once, or there and back) lasts as long whichever it is, so the turn at any time is found at once, and the list's shader is only told which slot shows and how much of the one before still does (its `_now` and `_fade`, parameters the inspector doesn't show). With its clock on Hits no clock counts: the turn is how many times its `hit` parameter has risen past a half. A Text List's turns are counted the same way and its line picked from the same deck, then told to its shader (`_now`, `_before`, `_turn`): a shuffle can't be dealt in a shader, where every pixel works alone. On Windows Media Foundation decodes movies on the GPU's video decoder where there is one (an iPhone's 1080p HEVC: about 70 fps, against under 30 on the CPU), the frames copied back for the upload.

| Media | How |
|---|---|
| Stills, animated GIFs, image sequences | Decoded into RGBA frames in memory (2 GB at most). GIFs play at their own delays (under 20 ms plays at 100 ms, as browsers do); a folder of numbered images is a sequence at 30 fps. A still is turned as its EXIF orientation says (a phone's photo is stored as the sensor saw it). HEIC and HEIF are decoded by the system on macOS only (ImageIO: upright, in sRGB, at most 8192 px a side); Windows and Linux don't read them. The render thread uploads a frame when it's due (`media.rs`), in any order: reverse, ping-pong and scrubbing cost nothing more |
| HAP movies in `.mov` (Hap, Hap Alpha, Hap Q, Hap R) | Velo's own MOV reader and HAP unpacker (stored, Snappy, chunked). A reader thread per movie reads the frame due next, any frame as cheaply as any other (reverse and scrubbing too); it goes to the GPU as a BC texture, with no CPU decoding. Hap Q's YCoCg turns to RGB in the Image shader (`hap.rs`) |
| Other movies: H.264, HEVC, ProRes on macOS | The system decodes them to BGRA on the movie's reader thread: AVFoundation on macOS (`video.rs`), Media Foundation on Windows (`video_windows.rs`). A movie whose track says it was shot turned (a phone's: a quarter or half turn in its preferred transform) has each frame turned upright as it's copied out, and is as wide as it shows, on macOS; on Windows it still plays as stored. Frames come in order: a frame behind the last read (the loop starting over, a cue, reverse, scrubbing back) or far ahead opens the reader again there, which decodes from the keyframe before it; cheap for ProRes, a stutter going backwards through long-GOP H.264 or HEVC. A `.mov` is tried as HAP first |
| Live (NDI, Spout, camera) | A Live node's `media` is `ndi:<name>`, `spout:<name>` or `camera:<name>`, not a file: the library opens it on a thread of its own and waits up to 10 s for a first frame, which makes the texture's size (BGRA8; later frames of another size are fitted to it, nearest pixel). A reader thread per source (`input.rs`) keeps the newest frame in a mailbox the render thread swaps out and uploads without waiting; it ends when nobody holds the feed. NDI: `share::ndi::receive` from the installed runtime, its frames asked for as BGRA. Spout (Windows): the sender's texture opened by its shared handle on a DX11 device of our own, copied to a staging texture and read about every 4 ms, BGRA or RGBA, with no frame counter in Spout's protocol (a frame can come twice); one CPU copy a frame (about 8 MB at 1080p). Camera (Windows): Media Foundation's capture devices (a capture card too), converted to RGB32 by the source reader at the device's default mode; Windows' camera privacy settings can refuse a desktop app. Camera (macOS): an AVFoundation capture session at its default preset, its frames asked for as BGRA and copied out on the capture queue; macOS asks for the camera the first time (`NSCameraUsageDescription` in the bundle), and no frame comes until the answer. `program` is no feed and opens nothing: in a layer on top it is what plays under the layer ([scenes.md](scenes.md) §3), a canvas-sized picture of the graph's own that the render thread copies the stage into as it puts the layers together |
| Text | Every glyph the words need, in Inter, JetBrains Mono or the node's own font file, as a signed distance field in an atlas made on the media thread; the shader lays out and draws the words (`text.rs`) |

Not yet: system video on Linux (HAP only there), HEIC stills off macOS, turned movies upright on Windows, screen capture, Syphon in on macOS, a choice of a camera's mode. A source that goes away keeps its last frame on the Live node.

## 5. Canvas and outputs

- The project's **canvas** (width, height, frame rate; 1920×1080 at 60 by default) is the size every node renders at.
- **One output**: a plain window, for trying things on one screen, or a **stage window** on a display picked by name (borderless, covering the display, above other windows unless it's the main display). Not the OS fullscreen, which on macOS is a Space with an animation. Esc in the output window closes it.
- The picture **keeps its shape**: bars on two sides when the display's shape differs, never stretched.
- The **Output node** grades what goes out: exposure, contrast, saturation, hue, colorize, invert, vignette, fade to black.
- **Blackout**, **Freeze** and the **test pattern** are styles of the output blit, free while off. Freeze copies the program once and shows it on the outputs, the shares and the preview until it's let go; the graphs keep running underneath.
- An output whose **display is unplugged** closes, and the UI says which display.

### Spout and NDI
The program is blitted full size into a BGRA texture, read back like the preview (three staging buffers) and handed to the share thread, so the frame loop never waits on it (`share.rs`).
- **Spout** (Windows): a D3D11 device of its own, a texture shared by handle, and Spout 2's shared memory (sender names, the sender's info, the active sender) written from its protocol, with no SDK linked. About one frame of latency.
- **NDI**: the NDI runtime the user installs, loaded when sharing starts and kept loaded; each frame goes out as BGRA.

Not yet: Syphon on macOS, several outputs, crop and rotation.

## 6. Node rendering

- Most nodes are **one fullscreen-triangle pass** with a generated pipeline ([plugins.md](plugins.md) §2), into a texture of their own. An input with nothing wired reads black.
- **Feedback** keeps two textures and reads the one it wrote last frame.
- **Mesh** draws the triangles of the node's `.obj` or `.ply` (`model.rs`: smooth normals by area, positions fitted into a unit ball, y down; at most 600,000 triangles, one vertex buffer of position, normal and place on a picture, 36 bytes a corner, made when a graph is built, in `Runner::set_mesh`). It is the one node with a depth buffer: a `Depth32Float` and an `Rgba16Float` of `MSAA` (4) samples, which wgpu guarantees for those formats, drawn into with a depth test (nearer wins, in any triangle order) and resolved into the node's picture, neither kept past the pass; so it costs about six canvas textures of video memory (`live.rs` counts them), over the vertex buffer. Its shader (`mesh.wgsl`) is a pinhole camera as Particles' Orbit view, then one light fixed in the world with diffuse, a Blinn shine, a studio reflection (a sky, a floor, a softbox), a rim and a hemisphere ambient, seen from either side of a surface; `facets` takes the normal from the screen derivatives; `bulge` pushes the vertices along their normals with the bass. The picture is premultiplied and see-through where nothing is drawn.

  The picture wired into `skin` is the surface's color under all of that: the tint times the picture, as far as `picture` (0 to 1) lets it show and the picture itself covers, so a see-through picture, or nothing wired, leaves the bare tint, to the bit what it was. A model whose file has texture coordinates (an .obj's `vt`, a .ply's `s t`; `Model::uv`, v flipped to count from the top) reads the picture there, repeated past its edges. Any other gets it triplanar, in the model's own space so it turns and breathes with the surface: three reads, along x, y and z, each weighted by how much the surface faces that way (to the sixth power, a narrow blend) and flipped on the far side so that nothing reads backwards; the picture keeps its shape, its height the unit ball's at `picture_scale` 1, and comes back mirrored past its edges, so no edge shows. A spherical wrap was the other choice: one read, but a seam down the back and a pinch at each pole, on any model. The reads are of the picture as it is, canvas size with no mipmaps, so a picture scaled far down on the model shimmers. With `picture` at 0, or nothing wired (the 1 × 1 black an unwired input reads, told apart by its size), the shader reads nothing, so an unwired Mesh costs what it did. No shadows, materials from the file, animation or more than one mesh a node.
- A shader with **buffers** keeps two textures a buffer (at the buffer's scale of the canvas) and draws one pass a buffer into the one of this frame's parity, in order, before its picture; each pass reads the buffers before it as drawn this frame and itself and the ones after as drawn last frame. The scene's memory budget counts them (`live.rs`), and the node's cost on the graph is its first pass to its last.
- **Particles** keep 65,536 particles in a buffer of their own: a compute pass emits, one thread moves each particle, then a sprite each is added up over a clear picture. `count` caps them without a rebuild. With shape Picture they are born where the picture wired into `emit` is lit (its brightness times its alpha is the odds, guessed up to 512 times each, in the compute pass through `textureLoad`), so a lit logo, an edge map or a movie is where they come from; a picture lit on under a hundredth of it spawns fewer, a black one none. A file, or what is wired into `sprite`, is what each particle looks like.

  A particle is 48 bytes: a position and a velocity in 3D (z is 0 for a flat system). The shape Sphere is born in a ball; the shape Model on the points of the node's `.obj` or `.ply` file (`model.rs`, no crates: an .obj's faces are sampled by area, an .ply's vertices thinned to 131,072; both fitted into -1..1, y down), held in a read-only buffer of their own at binding 16, written by `Runner::set_points` after every build. The view Orbit projects them through a pinhole camera of 50 degrees turning round the middle of the canvas (distance, orbit, tilt, turn), each sprite scaled by depth and at most a third of the picture; there is no depth buffer, the sprites' light adds up, so nothing is solid, sorted or lit. The view Flat is the canvas as it was, z ignored.
- **GPU cost**: where the GPU has timestamp queries, each pass of the edited graph is timed and shown on its node.
- A **Probe** reads a node's picture back as a color, a signal like any other.

## 7. Shader compilation

All of it on the compile thread, with the engine's device:
1. Parse the JSON header: name, category, parameters, inputs.
2. Wrap the body: the globals, a variable per parameter, the texture bindings, the entry points ([plugins.md](plugins.md) §2).
3. Validate with **naga**. Errors come back at the author's line numbers; **the node keeps its last good program**, else passes its input through.
4. Build the pipeline. Built-in shaders compile once and are reused by every later graph.

When edits come faster than builds, only the newest build of each scene is made. In a bundle, shader files are checked twice a second, and an outside editor's save becomes an undoable `node.set_code` ([project-format.md](project-format.md) §2).

## 8. Device loss and failure

- GPU errors (out of memory, validation) are caught with error scopes when a graph is built, never per frame.
- **The runtime is disposable.** When the device is lost (a driver reset, a shader past the system's time limit) or the render thread stops drawing for 2 s, the event loop starts a new engine from the project, reopens the output where it was and puts the scene that was out back out.
- The adapter asked for is the high-performance GPU (laptops with two GPUs).
