# MIDI and OSC

Any knob, fader or pad can drive Velo, and so can any app or phone that sends OSC.

## MIDI learn

Velo finds MIDI controllers by itself: a controller plugged in while it runs connects within a second, and one unplugged comes back when you plug it in again.

**Learn mode** binds many controls in a row:

1. Press <kbd>L</kbd>, or click the **MIDI** chip in the top bar. Every control you can learn gets an outline.
2. Click a control: a macro knob, a slider in the inspector, a scene card, a variation, the preview's **Launch**, the **Blackout** chip or the tempo chip. It breathes while it waits.
3. Move a knob, hit a pad, or press a key on the computer's keyboard. Bound: the control shows a badge such as `CC 21`, `Note C1` or `Key 4`.
4. Click the next control, move the next knob. <kbd>L</kbd> or <kbd>Esc</kbd> leaves.

For a single control, right-click it and choose **Learn MIDI or a key**, then move a knob, hit a pad or press a key. **Forget** on the same menu removes its binding.

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

### Hold instead of flip

A pad on a macro, a value, blackout or freeze flips it with each press. Make it **Momentary** in the bindings list and it holds instead: on while you keep the pad down, off the moment you let go. A pad held on Blackout is a flash to black; one on a macro that drives a strobe is a strobe for as long as you hold it.

### Soft takeover

When a scene launches, its macros jump to where that scene keeps them, but your knobs stay where they are. A knob bound to a macro waits until you turn it to where the macro is, then takes over, so nothing jumps. A knob bound to a value takes over at once. Learn sets this for you.

### Encoders and fine knobs

Endless encoders work too. Learn spots the two common kinds; the MIDI menu switches any CC binding between a knob and the three encoder kinds (two's complement, binary offset and sign magnitude). A pair of CCs that a controller sends for one fine, 14-bit knob is learned as one control.

## The bindings list

Click the arrow beside the **MIDI** chip: it lists the open ports and every binding, with its control, channel and target. A binding learned on one port can be made to listen on any port, or removed there. Its menu switches what the control is: a knob, one of the encoder kinds, a pad that flips (**Toggle**) or one you hold (**Momentary**). Keys and OSC addresses are in the same list, by their name.

Bindings are saved with the project, by port name. Unplug a controller and its bindings stay; plug it back and they work again. A project with a Momentary or an OSC binding doesn't open in an older Velo: update that one first.

## Controller lights

Velo can light your controller to match what it's doing. Turn on **Light the controllers** in **Preferences → Control**:

- a pad bound to a macro, a value, blackout or freeze is lit while it's on;
- the pad of the scene that's playing is lit, and goes dark when another one takes over;
- a knob or an encoder gets its value back, so an LED ring or a motor fader shows where the macro is, even when the mouse, a variation or the AI moved it.

Velo sends to the controller's output with the same name as its input, and only when something changes. It never sends a control back what you just did with it. Set your controller's pads to momentary (not toggle) in its own editor: Velo keeps the on and off.

## OSC

Turn OSC on in **Preferences → Control**. It shows this computer's address and the port to send to (9000 unless you change it) and the last address it heard. Velo listens on every network interface, so a phone with TouchOSC on the same network can drive it.

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
| `/velo/op/<operator>` | the operator's settings as JSON | Runs any operator, once allowed |

`/velo/op/` reaches every command in Velo: the same operators the command palette (<kbd>Ctrl</kbd> <kbd>K</kbd>) runs. That makes OSC the simplest way to script Velo from another language or a live-coding setup.

Velo listens for OSC from the whole network, so anyone on the venue's Wi-Fi can send to it. The other addresses only play the show, but `/velo/op/` can change or delete anything in it, so it stays off until you turn on **Run any command** in Preferences → Control.

### Your own addresses

A layout you already have doesn't need Velo's addresses: learn its own. In learn mode, click a control, then move a fader or press a button on the phone. Velo binds that address (`/1/fader1`, say) as it would a MIDI knob or pad: a fader sets the value, a button flips it, or holds it once you make it Momentary in the bindings list. Bind one to a macro and it moves everything the macro drives; bind it to a value of a signal node (a Math's **value**, a Gain's **gain**) and an app on the network shapes how the scene moves.

### Sending OSC

Velo can send OSC too, to another app or back to the phone. Turn on **Send OSC** in Preferences → Control, give it a computer's name or address and a port, and list the values to send, one address a line:

| Address | Sends |
|---|---|
| `/velo/scene/live` | The name of the scene that's playing |
| `/velo/tempo/bpm` | The tempo |
| `/velo/tempo/beat` | The beat, counted |
| `/velo/scene/macro/1` to `/8` | A macro of the scene that's playing, 0 to 1 |
| `/velo/param/<node>/<param>` | A value, 0 to 1 along its range |
| `/velo/signal/<node>` | A signal node's output, from the shared signals or the scene you're editing |

Each goes out as it changes, or all of them so many times a second if you set **Times a second**. The macro addresses are the ones Velo listens to, so a TouchOSC fader on `/velo/scene/macro/1` follows the macro when something else moves it.

## Keys

Every command has a key on the computer's keyboard too. The stage keys (scenes, blackout, freeze, tap, learn) are listed in [Shortcuts](./shortcuts).

### Keys as pads

No controller at hand? A key works as a pad. In learn mode, click a control and press a letter, a digit or <kbd>F1</kbd> to <kbd>F12</kbd>, alone: from then on that key flips the macro or the value between its two ends, launches the scene, recalls the variation, taps the tempo, or turns blackout or freeze on and off.

- A key you bind does that **instead of** its usual command, in this project only: bind <kbd>4</kbd> to a macro and <kbd>4</kbd> no longer launches scene 4 here. Remove the binding (**Forget**, or the bindings list) and the key is back to its command.
- <kbd>Esc</kbd> and <kbd>L</kbd> can't be learned: they leave learn mode. Nor can <kbd>Space</kbd>, <kbd>Tab</kbd>, <kbd>Enter</kbd>, the arrows, <kbd>Delete</kbd>, or a key held with <kbd>Shift</kbd>, <kbd>Ctrl</kbd>, <kbd>Alt</kbd> or <kbd>Cmd</kbd>.
- Nothing fires while you type in a field or the code editor, or while a dialog is open.
- Key bindings are saved with the project, like the others. A project saved by this version doesn't open in an older Velo: update that one first.
