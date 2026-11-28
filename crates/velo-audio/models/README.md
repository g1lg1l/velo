# The drum model

`drums.bin` is the network behind Velo's Model mode: kick, snare (and clap) and hat hits, and how
much of the sound is melody. A small recurrent network (1,086,724 weights, little-endian f32 in
PyTorch's layout) over 64 mel bands of a 1024-point spectrum every 256 samples: a layer of 256
over the last six hops, two GRUs of 256, one over the other, that carry what they have heard so
far, and an output layer over the first layer and the memory. `src/drums.rs` runs it live, hop by
hop, for about 3 % of a core, and is tested against a NumPy copy of it (`fixtures/drums/make.py`,
which remakes the fixtures after the model's shape changes).

It answers for the hop three before the newest (~16 ms at 48 kHz), so it hears each attack whole:
by then a kick's click has left its first window and a clap's noise has not, which is what tells
them apart. The memory is what the model before it lacked: 35 ms of sound cannot tell a clap on
the 2 from a kick's click on the 1, the bar so far can.

## How it was trained

On generated drums and on a private library of electronic music (techno, hard techno, schranz,
drum and bass, house; about 2500 tracks), whose owner allowed the weights to ship with Velo.
Neither the music nor the training scripts are in this repo.

1. **Real minutes.** One minute from the middle of each track (up to six from a long set), split
   by [Demucs](https://github.com/facebookresearch/demucs) (htdemucs, MIT) into drums, bass and
   the rest (the melody). A tenth of the tracks, picked by a hash of their path, are kept for
   testing: 2221 minutes train, 262 test.
2. **Generated clips.** 6000 clips of electronic kits drawn at random (kicks, snares, claps, hats,
   rides and shakers; rims, zaps and risers playing unlabelled) in patterns from four on the floor
   to breaks, over a real minute's bass and melody stems, sidechained and soft-clipped. Every hit
   is exact.
3. **The teacher.** A bigger network that hears a whole stretch at once, the drum stem beside the
   mix (a CNN and a bidirectional GRU, 80 mel bands), so it cannot run live. It learns from the
   clips and from real minutes whose labels no model made: kicks from DJ beat grids (some 900
   minutes), backbeats the library's owner marked by ear on 8 s excerpts (some 270), and backbeats
   read off the grid alone (219: where the 2 and the 4 sound unlike the 1 and the 3 in the drum
   stem there is a clap or a snare, right 99 times in 100 against the ear). Its hits on every real
   minute are that minute's labels.
4. **The live model** learns from the clips (exact hits) and the real minutes (the teacher's hits,
   and the melody: the melody stem's level against the mix's), read in order, 12 s at a time, half
   of the material resampled to 48 kHz first, for 40 epochs. A tenth of the real minutes is held
   back to pick the kick's and the hat's thresholds.

## Scores

On the test tracks, with Velo's own analyzer (the `listen` example, the app's code).

| | The model before | This model | The teacher |
|---|---|---|---|
| Snare and clap, backbeats heard | 0.19 | 0.84 | 0.87 |
| Snare and clap, hits that are one | 0.53 | 0.90 | 0.93 |
| Kick, F against DJ beat grids | 0.86 | 0.92 | 0.95 |

- **Snare and clap**: 66 excerpts whose backbeats were marked by ear, 396 beats with a hit to
  hear and 832 with none. The second row is the share of the hits on those beats that fall on a
  backbeat. The snare's threshold was picked on these same excerpts. A model with half the
  memory (one GRU of 128, the same training) heard 0.72 of the backbeats, and 0.84 of its hits
  were one.
- **Kick**: the 13,203 kicks DJ beat grids place in the four-on-the-floor test minutes
  (precision 0.97, recall 0.87).
- **Hat**: nobody marked hats by ear. Against the teacher's hits on the 262 test minutes, F 0.84
  (precision 0.82, recall 0.87).

The snare is still a little under the teacher, which hears the drum stem and the bars ahead; the
live model hears the mix, up to now.
