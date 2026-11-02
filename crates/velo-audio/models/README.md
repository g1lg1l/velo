# The drum model

`drums.bin` is the network behind Velo's Model mode: kick, snare (and clap) and hat hits, and how
much of the sound is melody. A small MLP (832 inputs, 256 and 128 hidden, 4 outputs; 246,660
weights, little-endian f32 in PyTorch's Linear layout) over 64 mel bands of a 1024-point spectrum
every 256 samples. `src/drums.rs` runs it live and matches the Python that trains it
(`training/drumfeat.py`) hop by hop.

It answers for the hop three before the newest (~16 ms at 48 kHz), so it hears each attack whole:
by then a kick's click has left its first window and a clap's noise has not, which is what tells
them apart.

## Data

The weights are trained on generated drums and on a private library of electronic music
(techno, hard techno, schranz, drum and bass, house; about 2500 tracks), whose owner allowed the
weights to ship with Velo. The music itself is not in the repo.

- **Real minutes** (`training/library.py`): one minute from the middle of each track (up to six
  from a long set), split by [Demucs](https://github.com/facebookresearch/demucs) (htdemucs, MIT)
  into drums, bass and the rest (other and vocals: the melody). A tenth of the tracks, picked by a
  hash of their path, are kept for testing: 2221 minutes train, 262 test.
- **Generated clips** (`training/drumgen.py`, `training/teacher.py make`): electronic kits drawn
  at random (909, 808, techno, hard and DnB kicks with clicks and distortion; snares, big and
  bright snares, claps; closed and open hats, rides, shakers; rims, zaps and risers playing
  unlabelled) in patterns from four on the floor to breaks, over a real minute's bass and melody
  stems, sidechained and soft-clipped, then split by Demucs again. Every hit is exact. 6500 clips,
  the last 1500 leaning on drum and bass (`DNB_HEAVY`).
- **The teacher** (`training/teacher.py`): a bigger network that hears the whole clip (a CNN and
  a bidirectional GRU over the drums stem and the mix, 80 mel bands) trained on the first 5000
  clips. Its hits on each real minute are that minute's labels. (A teacher trained on all 6500
  missed most snares, house claps falling from 43 a minute to 9, so the first one labels.)
- **The melody** target is the melody stem's level against the mix's (150 Hz to 8 kHz): 1 when
  the tune is as loud as the rest, 0 when it is 18 dB under or the mix is silent.

The live model learns from the clips (exact hits) and the real minutes (the teacher's hits and the
melody target), half of each resampled to 48 kHz first, the rate a computer's own output usually
runs at.

## Scores

F of Velo's own analyzer (the `listen` example, the app's code) on the 262 test minutes, against
the teacher's hits, a hit counting from 10 ms early to 40 ms late:

| | kick | snare / clap | hat |
|---|---|---|---|
| Simple mode, its bands tuned on the library | 0.53 | 0.30 | 0.45 |
| The model before (Slakh and generated mixes) | 0.61 | 0.36 | 0.58 |
| This model | 0.77 | 0.46 | 0.70 |

Techno and house kicks score 0.74 to 0.92, hard techno and schranz 0.68 to 0.83; drum and bass is
the hardest (kicks 0.65 to 0.69, neurofunk snares 0.28). The melody, as the app smooths it,
follows what it learned (how much of the sound is melody) with a Spearman correlation of 0.35 per
minute; against the melody stem's own level it reaches 0.12, the spectrum's peaks it replaces 0.10.

## Making it again

```bash
python training/library.py <music folder> <library>
python training/teacher.py make <library>/train <clips> 5000
python training/teacher.py train <clips> 20 teacher.pt
python training/teacher.py label teacher.pt <library>/test <library>/train
DNB_HEAVY=1 python training/teacher.py make <library>/train <clips> 6500
python training/livedata.py <library>/train <live>/real
python training/livedata.py <library>/test <live>/test
python training/livedata.py --clips <clips> <live>/clips
python training/livetrain.py <live>/clips,<live>/real 12 <out> <live>/test
python training/score.py <listen.exe> <library>/test model
```

`livetrain.py` writes `<out>.bin` (the weights, as here) and `<out>.json` (each drum's threshold,
tuned on a tenth of the real minutes; `src/analysis.rs` uses them). `score.py` runs Velo's own
analyzer (the `listen` example) over the test minutes and matches its hits from 10 ms early to
40 ms late. After the model's shape changes, `fixtures/drums/make.py` remakes the fixtures
`drums.rs` is tested with. Each track trains on a random stretch (`CROP`, in hops), which keeps
the whole set in 16 GB of memory.
