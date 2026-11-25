# Outputs

The picture of the live scene goes out through the **Output** chip in the top bar: fullscreen on one display or several, in a window, or to other apps through Spout and NDI.

## A display

Click **Output** and pick a display. Each one is listed with its name, its size and its refresh rate, for example `Display 2  1920×1080 · 60 Hz`.

The picture fills that display in a borderless stage window, above other windows, with no interface and no cursor on it. Its shape is kept: if the display's shape differs from the picture's, you get bars on two sides, never a stretched picture.

Each display in the menu is a switch: pick another and the picture goes there too, as many as you have plugged in. An open one has a tick and a cross: the cross closes it, **Close output** (**Close every output** with several) closes them all. Clicking an open one never closes it, so a misclick on stage can't put a projector out. <kbd>Esc</kbd> on an output closes only that one, and only when it has the keyboard.

Two projectors of the same model have the same name: the menu tells them apart from left to right, as `EPSON PJ` and `EPSON PJ (2)`.

**Window** in the same menu puts an output in an ordinary window too, handy for checking it on one screen.

## Several displays

With two or more outputs open, the **PROJECTION** part of the Output menu sets one of them, named in its title: the one you opened last, until you click another open one in the list.

- **Crop** shows only a part of the picture on this output, in percent: X from the left, Y from the top, W its width and H its height. For two projectors side by side, set the canvas twice as wide (3840×1080), then pick **Left half** for one output and **Right half** for the other: each shows its half, at its full resolution. **Whole** puts all of it back.
- **Rotation** turns the picture a quarter at a time: 90° or 270° for a screen hung on its side, 180° for a projector hung upside down from the ceiling.

The cropped, turned picture keeps its shape on the output, with bars if the display's shape differs. The test pattern isn't cropped or turned: it shows the projector itself. Every output shows the same live scene; the crop picks the part.

Spout, NDI and the recording always take the whole picture, unturned.

## Fitting the picture to the screen

A projector hung off to the side, or a cloth that isn't square, makes the picture land as a slanted shape that spills past the screen. **Adjust the projection** in the Output menu (<kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>K</kbd>, <kbd>Cmd</kbd> on a Mac) puts a handle on each corner of the picture, on the output itself:

- **Drag each corner** to where that corner of the screen is. The picture follows in perspective, black around it.
- For the last pixel, click a corner and nudge it with the **arrow keys**, one pixel at a time (ten with <kbd>Shift</kbd>). <kbd>Tab</kbd> picks the next handle.
- Turn on the **Test pattern** while you do it: it is pinned too, and its circle and square cells show when the corners are right.
- **Draw a mask** (in the same menu) blacks out a shape over the picture, so no light falls on the DJ or past the cloth: click its points on the output, then <kbd>Enter</kbd> or click its first point again. Masks can be any shape. Drag their points to change them; click inside one and press <kbd>Delete</kbd>, or use its bin in the menu, to remove it. **Soft edge** fades the black in from the mask's edge.
- <kbd>Esc</kbd> on the output ends it. **Reset the projection** puts the whole picture back square, unturned, with no masks.

While you adjust, the preview shows the output as it lands, handles and all. The crop, rotation, corners and masks stay with this computer, one set for each display, not with the show: plug into the same projector next week and they are as you left them. Spout and NDI still send the picture square; the app on the other end maps it.

If a projector is unplugged, the picture keeps running; Velo tells you, and recovers when it's back. A GPU driver reset is recovered from the same way.

## The picture's size

With no node selected, the inspector shows the **Canvas**: the size and frame rate every output renders at, Full HD at 60 frames a second unless you change it. Pick a preset or type the width, height and FPS.

## Syphon, Spout and NDI

Under **SHARE** in the Output menu:

- **Syphon** (Mac) sends the picture to other apps on this Mac: Resolume, MadMapper, VDMX, OBS, TouchDesigner, Millumin. It shows up there as a Syphon server called Velo, nothing to install.
- **Spout** (Windows) sends the picture to other apps on this computer: Resolume, TouchDesigner, OBS, MadMapper. It shows up there as a Spout source called Velo, about a frame late.
- **NDI** sends it to any computer on the network with an NDI receiver: OBS, vMix, NDI Studio Monitor. It needs the free NDI runtime installed on this computer.

The share sends the program at full size, whatever the display does.

## Recording

**Record the output** in the Output menu (<kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>R</kbd>, <kbd>Cmd</kbd> on a Mac) records what the outputs show to an MP4 in your Movies folder (Videos on Windows and Linux), under `Velo`, at the canvas's size and frame rate. The chip says **REC** and how long it's been; the same command stops it. Blackout, Freeze and the test pattern are recorded as the outputs show them.

The sound Velo hears goes in too: the computer's, an app's, a microphone's or the audio file's, put as late as **Speaker lag** says, so it lines up with the pictures as it did in the room. Untick **With the sound Velo hears** for a silent movie.

Recording needs [ffmpeg](https://ffmpeg.org). Velo uses the one on the computer if there is one (`brew install ffmpeg` on a Mac, `winget install ffmpeg` on Windows, your package manager on Linux). If there isn't, the first **Record the output** asks before downloading one: about 28 MB on a Mac, 119 MB on Windows, 33 MB on Linux (x86-64), a pinned build checked against its published checksum, kept in Velo's own folder and used from then on. Nothing is downloaded unless you say so. On a Mac it encodes on the GPU. Quitting Velo while it records finishes the movie first.

## On stage

| Key | What it does |
|---|---|
| <kbd>B</kbd> | **Blackout**: the outputs go black, again to come back. The chip turns red while it's on |
| <kbd>F</kbd> | **Freeze**: the outputs hold the last picture while the scene runs on underneath, again to let go |
| <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>T</kbd> | **Test pattern**: square cells, a circle that shows any stretch, the middle crossed, the edge, color bars and a grey ramp, for lining up a projector |

All three are in the Output menu too, and Blackout and Freeze have their own buttons in the top bar. Blackout, Freeze and the tempo can also be bound to a controller or sent by OSC.

**Stage theme.** In a dark club the laptop lights up your face. <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>L</kbd> (<kbd>Cmd</kbd> on a Mac), or the switch in **Preferences → General → Theme**, turns the interface near black: dim grey text, one amber accent, warnings yellow and errors red, and the preview and scene pictures at about two thirds of their brightness. The output isn't touched. It stays on until you flip it back, to the theme you had.

**Projector lag.** A projector shows the picture a little after it's drawn, often 30 to 100 ms, so on the wall the beat lands late. In the Output menu, set **Projector lag** to that delay: everything that moves on the beat (launches, transitions, the Beat, LFOs, sequencers, a Media List's turns) is drawn that much early and lands on time. Clap along with a scene that flashes on the beat and raise it until the flash is on your clap. What the music itself moves (a kick heard, the bass) can't be drawn before it's heard, so it stays a little late. The setting stays with this computer, not with the show.

**Speaker lag.** Bluetooth speakers play the computer's sound 100 to 300 ms after Velo hears it, so the pictures run ahead of the music. In the Output menu, set **Speaker lag** to that delay: what Velo hears of the computer's sound, of one app or of an audio file waits that long, and everything the music moves lands with the sound. Raise it until a kick's flash is on the kick you hear. A microphone hears the room as it is and isn't held back. The setting stays with this computer too.

## The Output's grade

The **Output** node, last in every scene, grades what goes out: the displays, the shares and the preview alike. Select it to see its values in the inspector, or open them on its card with the chevron in its footer.

| Value | What it does |
|---|---|
| **Exposure** | Brighter or darker, in stops (from -3 to +3) |
| **Contrast** | Flatter or harder (1 leaves it) |
| **Saturation** | Grey at 0, richer above 1 |
| **Hue** | Turns every color round the wheel |
| **Colorize** | Tints the picture through a color you pick |
| **Invert** | Negative |
| **Vignette** | Darkens the corners |
| **Fade** | Fades to black |

Each is a value like any other: map the kick onto Exposure, or give Fade to a macro and you have a fader to black on your controller. At its defaults the grade leaves the picture exactly as it was.
