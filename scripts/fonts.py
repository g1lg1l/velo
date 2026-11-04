"""Rebuilds Velo's three fonts in crates/velo/assets/fonts (needs `pip install fonttools`).
From Ravune (bd10814), trimmed for Velo.

    python scripts/fonts.py <egui-phosphor crate dir> [<Inter variable .ttf> <JetBrainsMono variable .ttf>]
    (with only the first argument, just the icons)

- Phosphor (MIT): the regular weight, subset to the icons listed below; also writes
  crates/velo/src/icons.rs. Add a name here, rerun, and use `icons::NAME`.
  The crate dir is egui-phosphor's source (e.g. from ~/.cargo/registry/src/*/egui-phosphor-*).
- Inter and JetBrains Mono (SIL OFL): pinned to the one weight Velo draws (480 / 500)
  and subset to Latin, punctuation, arrows and a few symbol blocks.
"""
import os
import re
import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ICONS = """ARROW_CLOCKWISE ARROW_SQUARE_OUT ARROW_U_UP_LEFT ARROW_U_UP_RIGHT CARET_DOWN CARET_LEFT CARET_RIGHT CHATS CHECK CIRCLE_NOTCH CORNERS_OUT CROSSHAIR DOWNLOAD_SIMPLE FILE_PLUS FLOPPY_DISK
FOLDER_OPEN GEAR_SIX INFO KEYBOARD LAYOUT LIGHTNING LINK MAGNIFYING_GLASS METRONOME MONITOR_PLAY PALETTE PENCIL_SIMPLE PIANO_KEYS PICTURE_IN_PICTURE
PLAY PLUS POWER PROJECTOR_SCREEN SIDEBAR_SIMPLE SLIDERS SPARKLE SPEAKER_HIGH SQUARE_SPLIT_HORIZONTAL STACK TIMER TRASH TREE_STRUCTURE WAVEFORM
WAVE_SINE X
APP_WINDOW ARROW_LEFT ARROW_RIGHT ARROWS_CLOCKWISE ARROWS_LEFT_RIGHT BROADCAST CODE COPY CURSOR_CLICK EYE FILM_STRIP FRAME_CORNERS
GRID_FOUR HAND_GRABBING HAND_TAP IMAGE IMAGE_SQUARE IMAGES LINK_BREAK LIST_BULLETS MICROPHONE MINUS MONITOR PATH PLAY_CIRCLE PLUGS
PLUGS_CONNECTED ROBOT SELECTION_PLUS SHUFFLE SLIDERS_HORIZONTAL SPEEDOMETER TERMINAL_WINDOW TEXT_AA TEXT_T TRANSLATE WARNING
WIND GAUGE AIRPLANE_TILT SNOWFLAKE
SQUARES_FOUR MAGIC_WAND INTERSECT ARROWS_SPLIT FUNCTION CLOUD GRADIENT SHAPES CONFETTI ARROWS_OUT_CARDINAL FLOWER DROP WAVES
CIRCLE_HALF CHART_BAR CIRCLE_HALF_TILT POLYGON SWATCHES CROP TARGET MAP_TRIFOLD EYEDROPPER SELECTION_INVERSE GIT_MERGE
TOGGLE_RIGHT CIRCLES_THREE CLOCK DICE_FIVE ARROWS_DOWN_UP MATH_OPERATIONS WAVE_TRIANGLE HASH DOTS_NINE PUSH_PIN TOGGLE_LEFT
SCALES CHART_LINE_UP STAIRS CLOCK_COUNTER_CLOCKWISE SPIRAL TREND_UP SCISSORS HOURGLASS DROP_HALF""".split()

TEXT = [(0x20, 0x7E), (0xA0, 0xFF), (0x2000, 0x206F), (0x20AC, 0x20AC), (0x2190, 0x21FF), (0x2200, 0x22FF),
        (0x25A0, 0x25FF), (0x2600, 0x26FF)]

ROOT = os.path.join(os.path.dirname(__file__), "..")
FONTS = os.path.join(ROOT, "crates/velo/assets/fonts")


def save_subset(font, unicodes, out, features=()):
    opts = subset.Options()
    opts.layout_features = list(features)
    opts.hinting = False
    opts.notdef_outline = True
    opts.name_IDs = ["*"]
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=unicodes)
    sub.subset(font)
    font.save(out)
    print(f"{os.path.basename(out)}: {os.path.getsize(out) // 1024} KB")


def main(phosphor_dir, inter=None, jetbrains=None):
    table = dict(re.findall(r'pub const (\w+): &str = "\\u\{([0-9A-F]+)\}";',
                            open(os.path.join(phosphor_dir, "src/variants/codepoints.rs"), encoding="utf-8").read()))
    names = list(dict.fromkeys(ICONS))
    missing = [n for n in names if n not in table]
    if missing:
        sys.exit(f"not in Phosphor: {missing}")
    lines = ["//! Phosphor icons (MIT, phosphoricons.com), the regular weight, subset to the glyphs below.",
             "//! From Ravune (bd10814), trimmed for Velo.",
             "//! Regenerate with scripts/fonts.py after adding a name there.",
             "#![allow(dead_code)]", ""]
    lines += [f'pub const {n}: &str = "\\u{{{table[n]}}}";' for n in sorted(names)]
    open(os.path.join(ROOT, "crates/velo/src/icons.rs"), "w", encoding="utf-8", newline="\n").write("\n".join(lines) + "\n")
    save_subset(TTFont(os.path.join(phosphor_dir, "res/Phosphor.ttf")), [int(table[n], 16) for n in names],
                os.path.join(FONTS, "Phosphor.ttf"))

    if not (inter and jetbrains):
        return  # icons only
    text = [c for a, b in TEXT for c in range(a, b + 1)]
    for src, out, pin in [(inter, "Inter.ttf", {"wght": 480, "opsz": 14}), (jetbrains, "JetBrainsMono.ttf", {"wght": 500})]:
        font = TTFont(src)
        axes = [a.axisTag for a in font["fvar"].axes] if "fvar" in font else []
        if axes:
            font = instancer.instantiateVariableFont(font, {k: v for k, v in pin.items() if k in axes})
        save_subset(font, text, os.path.join(FONTS, out), ["kern", "liga", "calt", "tnum"])


if __name__ == "__main__":
    if len(sys.argv) not in (2, 4):
        sys.exit(__doc__)
    main(*sys.argv[1:])
