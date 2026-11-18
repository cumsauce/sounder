# Crumple

## `crumple`

A crumpled-paper source (surface family, generator): a population of short
band-passed noise bursts whose count follows `density` all the way to 400 per
second, plus sparse louder snaps. It is the dense counterpart of `cloth`, whose
burst rate saturates (its fine onset count stops rising from `motion` 1).
Code: `crates/sfx_graph/src/nodes/crackle.rs`; tests:
`crates/sfx_analysis/tests/it/surface.rs`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `density` | signal | `40` | bursts per second, 0 .. 400 | Burst count per second. 0 is exact silence. A number outside the range is an error; a signal is clamped (a non-finite sample counts as 0). |
| `size` | number | `0.5` | 0 .. 1 | How much paper. Larger: burst centres lower (x 2^-1.5 per unit of size, so 0.75 octave from 0.5 to 1), decays longer (x 0.5 at size 0 to x 2 at 1) and a heavier tail of big bursts (Pareto exponent 3 at 0, 2 at 1, amplitude capped at 2.5x the smallest). |
| `hardness` | number | `0.5` | 0 .. 1 | How stiff. Burst centres are log-uniform in 1.5 x 2^h .. 4.5 x 2^h kHz (1.5-4.5 kHz soft, 3-9 kHz hard); the amplitude decay time constant is 12 ms x (1/12)^h (12 ms soft, 3.5 ms at 0.5, 1 ms hard), both at size 0.5 and density 40 or less. |
| `seed` | integer | `0` | | Mixed with the document seed and the node name. |

Each burst is white noise through a band-pass (Q 1.4) at its centre, times a
0.2 ms attack and an exponential decay. 6 % of bursts are snaps: centre at
1.3 x the top of the band, Q 0.8, a decay 0.4x as long, and a level of 2 times
the square root of the burst amplitude. A snap replaces a burst, so it does not
change the count.

Arrivals are a jittered grid, not a Poisson process: each burst falls at the
centre of its slot (1 / density seconds) +- 0.2 slots, so gaps run from 0.6 to
1.4 slots. Over 1 s there are `density` bursts give or take one, and a
detector with a 5 ms dead time does not lose pairs that a Poisson process would
put 1 ms apart. `density` as a signal moves the slot width continuously.

Above `density` 40 a burst's decay time constant is multiplied by 40 / density
and its level stays at the density-40 value, so the energy per second does not
grow with the count and a tail never buries the next burst. Below 40 the decay
is unchanged and the level per burst rises as sqrt(40 / density) (at most 2x).

Caps and errors: at most 128 bursts ring at once (7 at density 40, fewer
above; the cap is never reached with the limits above, a further burst would be
skipped); the one buffer is the document length.

- `density: 500: give a number from 0 to 400 bursts per second (40 is a slow crumple, 160 a fast one)`
- `size: 1.5: give a number from 0 to 1`
- `hardness: -0.1: give a number from 0 to 1`

The node is random (it reads `seed`) and causal (sample by sample, no
lookahead).

### Recipe

A hand squeezing foil: the density wanders between 30 and 140 bursts per
second, small and hard. Give the human macros on `hardness` (paper to foil), on
`size` and on the wander `range`.

```json
[
  {"type": "wander", "name": "squeeze", "rate": 3, "range": [30, 140], "seed": 2},
  {"type": "crumple", "name": "foil", "density": "squeeze", "size": 0.4, "hardness": 0.85, "seed": 3},
  {"type": "output", "name": "out", "input": "foil", "lufs": -20}
]
```

### Measured

Tests in `crates/sfx_analysis/tests/it/surface.rs`, 48 kHz. "Fine onsets" are
`measure::fine_onsets(audio, 5.0)`, which is what `expect onsets {spacing_ms: 5}`
counts.

| What | Measured | Test |
|---|---|---|
| fine onsets per s at density 20, 40, 80, 160 (3 seeds, 2 s each) | 20.0 / 20.0 / 20.0; 40.0 x3; 80.0 x3; 157.0, 155.0, 152.5 (limit: within 25 %) | `crumple_fine_onset_count_tracks_density` |
| fine onsets at 2 ms dead time, density 50 / 100 / 200 | 67 / 127 / 177 per s, rising | `crumple_has_no_saturation_to_400` |
| rms at density 50 and at 400 | -28.2 and -30.2 dB (limit: within 6 dB) | same |
| density ramp 20 -> 180 linear over 2 s: bursts in the first and second second | 60 and 138 per s (expected 60 and 140) | `crumple_density_signal_and_silence` |
| centroid by hardness 0 / .25 / .5 / .75 / 1 (mean of 4 seeds, 2 s) | 4459 / 5086 / 5568 / 6216 / 7223 Hz (each step > +3 %) | `crumple_hardness_raises_the_centroid` |
| centroid by size 0 / .25 / .5 / .75 / 1 | 8061 / 6644 / 5568 / 4801 / 3884 Hz (each step < -2 %) | `crumple_size_lowers_the_centroid` |
| seeds 1 vs 2 | 1733 of 48000 samples equal, relative difference 1.67 | `crumple_seeds_differ` |
| 1.0 s, density 40, `output lufs -20`, seeds 1-6 | 40 fine onsets each; -20.41, -20.17, -20.33, -20.08, -20.27, -20.42 LUFS; no warning | `crumple_into_lufs_minus_20` |
| density 400 at the four size / hardness corners: peak | 0.51, 0.07, 0.56, 0.29 (no non-finite sample) | `crumple_is_finite_at_the_extremes` |
| density 0 | exact zeros | `crumple_density_signal_and_silence` |
| two renders, default and density 160 / size 0.9 / hardness 0.1, one thread vs the pool | same bits | `crumple_is_bit_identical` |
| recipe above, 1.5 s | 72000 samples | `crumple_recipe_renders` |

### Honest limits

- None of this has been heard. The numbers show a count, a centroid and a
  level, not that a person would call it paper.
- The fine onset detector (1 ms chunks, a burst must be twice the loudest of
  the 3 chunks before it) cannot count bursts closer than about 3 ms, so it
  reads below `density` from about 200 per second; the 160 row above already
  reads 3 % low. At 400 per second the generator still makes 400 bursts; the
  count was checked only to 200 (2 ms dead time).
- The count tracking relies on the density-dependent shortening above 40: with
  an unshortened decay the same test read 81 per second at density 160.
  The card's "1-12 ms decays" holds at density 40 or less; at 160 a burst's
  time constant is 0.9 ms at hardness 0.5.
- Level per burst is not a physical model, only a count-and-spectrum one.
  Amplitudes span only 8 dB (a wider range failed `output lufs -20` at 40 per
  second: crest 25 dB, -21.1 LUFS) so the sound is not a heavy-tailed one.
- A short, sparse render (a few bursts a second at hardness 1) is too peaky
  for `output lufs`: use `peak_db`.
- Extreme values stay finite (table above).
