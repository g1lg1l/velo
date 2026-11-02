# MIDI and OSC

Any knob, fader or pad can drive Velo, and so can any app or phone that sends OSC.

## MIDI learn

Velo finds MIDI controllers by itself: a controller plugged in while it runs connects within a second, and one unplugged comes back when you plug it in again.

**Learn mode** binds many controls in a row:

1. Press <kbd>L</kbd>, or click the **MIDI** chip in the top bar. Every control you can learn gets an outline.
2. Click a control: a macro knob, a slider in the inspector, a scene card, a variation, the preview's **Launch**, the **Blackout** chip or the tempo chip. It breathes while it waits.
3. Move a knob or hit a pad. Bound: the control shows a badge such as `CC 21` or `Note C1`.
4. Click the next control, move the next knob. <kbd>L</kbd> or <kbd>Esc</kbd> leaves.

For a single control, right-click it and choose **Learn MIDI**. **Forget** on the same menu removes its binding.

The first message that means something wins: a knob has to move two steps, a pad has to be hit. That message only binds; it doesn't also act.

## What a binding does

| Bound to | A knob or fader | A pad or key |
|---|---|---|
| A macro or a value | Sets it along its travel | Flips it between its two ends |
| A scene card | | Launches that scene |
| The preview's Launch | | Launches the selected scene |
| A variation | | Recalls it |
| Blackout | | Turns it on or off |
| The tempo chip | | Taps the tempo |

A **program change** launches scenes too: program 1 the first scene, program 2 the second, and so on.

### Soft takeover

When a scene launches, its macros jump to where that scene keeps them, but your knobs stay where they are. A knob bound to a macro waits until you turn it to where the macro is, then takes over, so nothing jumps. A knob bound to a value takes over at once. Learn sets this for you.

### Encoders and fine knobs

Endless encoders work too. Learn spots the two common kinds; the MIDI menu switches any CC binding between a knob and the three encoder kinds (two's complement, binary offset and sign magnitude). A pair of CCs that a controller sends for one fine, 14-bit knob is learned as one control.

## The bindings list

Click the arrow beside the **MIDI** chip: it lists the open ports and every binding, with its control, channel and target. A binding learned on one port can be made to listen on any port, or removed there.

Bindings are saved with the project, by port name. Unplug a controller and its bindings stay; plug it back and they work again.

## OSC

Turn OSC on in **Preferences → OSC**. It shows this computer's address and the port to send to (9000 unless you change it) and the last address it heard. Velo listens on every network interface, so a phone with TouchOSC on the same network can drive it.

| Address | Arguments | Does |
|---|---|---|
| `/velo/scene/launch` | a number from 1, or a name | Launches a scene |
| `/velo/scene/select` | a number from 1, or a name | Selects a scene |
| `/velo/scene/macro/1` to `/8` | 0 to 1 | Sets a macro of the selected scene |
| `/velo/variation/<n>` | | Recalls a variation |
| `/velo/param/<node>/<param>` | 0 to 1 along its range | Sets a value; the node by its id or its name |
| `/velo/tempo/tap` | | Taps the tempo |
| `/velo/tempo/bpm` | the BPM | Sets the tempo |
| `/velo/tempo/resync` | | Puts the downbeat on now |
| `/velo/blackout` | on or off, or nothing to flip | Blackout |
| `/velo/freeze` | on or off, or nothing to flip | Freeze |
| `/velo/op/<operator>` | the operator's settings as JSON | Runs any operator |

`/velo/op/` reaches every command in Velo: the same operators the command palette (<kbd>Ctrl</kbd> <kbd>K</kbd>) runs. That makes OSC the simplest way to script Velo from another language or a live-coding setup.

## Keys

Every command has a key on the computer's keyboard too. The stage keys (scenes, blackout, freeze, tap, learn) are listed in [Shortcuts](./shortcuts).
