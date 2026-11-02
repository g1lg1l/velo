# Velo: MIDI and live control

How controllers, OSC and outside clocks drive Velo. The code is `crates/velo/src/midi.rs`, `osc.rs` and `link.rs`; the binding model is `velo_core::Binding`.

## 1. One path for every input

```
MIDI (midir, its own thread) --+
                               +--> stamped events --> UI thread, once a frame --+--> operators: macro.set, node.set_param, variation.recall
OSC (UDP, its own thread) -----+    (queued)           bindings / OSC addresses  +--> engine commands: launch, blackout, freeze, tap
MIDI clock, Ableton Link ------------------------------------------------------------> the beat clock
```

- Messages are stamped when they arrive and queued for the UI thread, which drains the queue once a frame without waiting. Continuous controls coalesce to their last value per frame; every press is counted, none is lost.
- Bindings act through the same operators as the mouse, so undo and saving work as usual. A controller sweep is one undo step; a second's pause starts the next.
- Launches, blackout, freeze and taps are engine commands. A tap carries the time it arrived, so tap tempo isn't late.
- Values reach the render thread with the next sync: at most one UI frame (16 ms at 60 Hz) after they arrived. A UI stalled by a dialog holds control meanwhile; the output keeps running.
- Keys are the keymap's (product.md §7.6), not bindings.

## 2. Bindings

A binding is project data, the same in every scene:

```rust
pub struct Binding { pub control: Control, pub target: Target, pub behavior: Behavior }

pub enum Control  { Midi { port: Option<String>, channel: u8, msg: MidiMsg } } // port None: any port
pub enum MidiMsg  { Cc(u8), Note(u8), ProgramChange, Cc14(u8) }
pub enum Target   { Macro(u8), Param { node, param }, LaunchScene(usize), LaunchSelected,
                    Variation(usize), Blackout, Freeze, TapTempo }
pub enum Behavior { Absolute { takeover: Takeover }, Relative { mode: RelMode, step: f32 }, Trigger, Toggle }
pub enum Takeover { Jump, Pickup }
```

`binding.add` replaces the old binding of the same control and of the same target (one each); `binding.remove` takes an index or a target. Both are undoable.

| Behavior | Control | What it does |
|---|---|---|
| Absolute | knob, fader | Sets the value along its travel. A 14-bit pair (CC n coarse, CC n + 32 fine, n under 32) is one fine control, learned whole. |
| Relative | endless encoder | Turns the value by `step` of its travel a click (1/128 when learned). Three dialects: two's complement (1 / 127), binary offset (65 / 63), sign-magnitude (1 / 65). A frame's clicks add up. |
| Trigger | pad, key | Fires on a press: a note-on, a CC rising past 64, a program change. |
| Toggle | pad, key | Each press flips it. |

On a press, continuous targets (a macro, a parameter, blackout, freeze) flip between their ends and the rest fire. A latching pad (127 on, 0 off) flips only on its 127. A **program change** launches scene `index + program`: `LaunchScene(0)`, what learn makes, maps program 0 to the first scene.

**Soft takeover.** After a scene launch a physical knob at 20 % may drive a macro now at 70 %. `Pickup` moves the value only once the knob is within 1.5 steps of it or has crossed it since its last message; a knob that set the value last time is on it, so it keeps driving. `Jump` applies at once. Learn makes macro knobs Pickup and parameter knobs Jump.

**Ports.** A binding stores the MIDI port's name; the bindings list can make it answer any port, so a project moves between machines.

Not yet: Momentary, Scale takeover, a range or curve on a binding, pitch bend, MIDI out and LED feedback, controller profiles, a remap dialog for unknown ports, a warning when a controller goes away (its bindings stay and work again once it's back).

## 3. MIDI learn

1. Press `L` or click the MIDI chip for **learn mode**: macro knobs, parameter sliders, scene cards, variation buttons, the preview's Launch, the tempo, Freeze and Blackout get an accent outline. Click one: it is picked (it doesn't act) and breathes while it waits.
2. Move a knob or hit a pad. The first meaningful message binds it: a CC 2 steps from where it was, a note-on, a program change, or a few small steps around 0 or 64 (an endless encoder, learned as Relative). That message does nothing else.
3. The control shows a badge: `CC 21`, `Note C1` (60 is C3, as in Ableton) or `Program`.

Right-click a control → **Learn MIDI** binds that one without learn mode; **Forget** removes its binding. `Esc` leaves. The MIDI chip's menu lists the open ports and every binding (control, channel, target), switches a CC binding between knob and the three encoder dialects, makes one answer any port, or removes it.

MIDI runs on a `velo midi` thread that rescans the ports every second: a new port connects, a gone one is dropped, one another app holds is retried. Clock, sysex and active sensing are not bound.

## 4. Tempo and clock sources

One beat clock on the render thread drives everything on the beat. Its tempo comes from, in order of who leads:

| Source | How |
|---|---|
| **MIDI clock in** | 24 ticks a beat from any port. The tempo is measured over up to four beats of ticks, the phase locks on every beat, and Start puts the downbeat; joined mid-song it locks to the nearest beat. Stop and Continue change nothing. The tempo chip says MIDI while its beats come. |
| **Ableton Link** | Tempo and the place in a 4-beat bar shared with Live, DJ apps and phones on the network. On from the tempo chip's menu (saved). A tempo set in Velo (typed, tapped, heard, a MIDI clock's) goes to the session; alone on the network, the session starts where Velo's clock is. The chip says LINK and the number of peers, and auto-BPM steps back while there are any. In builds with the `link` feature (the default). Not yet: start and stop sync. |
| **The music** | Auto-BPM (audio.md §4): on by default for computer audio. |
| **By hand** | Tap (`T`, Tap in the chip's menu, or a pad): the first tap after a 2-s pause is a downbeat, the tempo is the average gap of the last 8 taps, to the half BPM. Typed or nudged ±1 in the chip's menu; Resync puts the downbeat on now. Setting it by hand stops following the music. |

The clock gives `bpm`, `beat_phase` and `bar_phase` to shaders, and `beat` and `bar_phase` as sources for mappings. Not yet: Pioneer Pro DJ Link and Denon StageLinq, a latency offset to lead a slow projector.

## 5. Threading and timing

- midir delivers MIDI on its own thread, OSC has a socket thread; both stamp and queue for the UI thread (`std::sync::mpsc`).
- MIDI clock ticks have their own queue; the UI thread hands the beats to the beat clock with their arrival times.
- Bindings apply on the UI thread, not the render thread: every target is document data or needs the UI's scene builds. Revisit if measured latency or stalls matter on stage.

## 6. OSC

OSC in, on a UDP port (9000 by default), off until Preferences → Control turns it on. That page shows where to send (this computer's address and the port) and the last address heard. It listens on every interface, so a phone on the same network can drive it. The decoder is Velo's own (`osc.rs`, no dependency): messages and bundles with `i`, `f`, `h`, `d`, `s`, `T`, `F` arguments; blobs and the rest are skipped. Messages go through the same actions as MIDI.

```
/velo/scene/macro/<1-8>        f 0..1    a macro of the scene on the output
/velo/scene/launch             i | s     a scene by number (from 1) or name
/velo/scene/select             i | s     the scene shown in the preview
/velo/variation/<n>                      recall variation n (1 is A)
/velo/param/<node>/<param>     f 0..1    along the parameter's range; the node by id or name
/velo/tempo/tap
/velo/tempo/bpm                f
/velo/tempo/resync                       the downbeat is now
/velo/blackout                 [i|f|T|F] on or off; no argument flips it
/velo/freeze                   [i|f|T|F] the same
/velo/op/<operator>            [s]       any operator, its props as JSON
```

`/velo/op` makes OSC the plugin path for outside tools in any language (plugins.md). Not yet: `/velo/source/<name>` as a modulation source, learning custom addresses, OSC out, OSCQuery.
