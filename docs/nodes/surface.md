# Rolling, cloth, footstep

Source file `crates/sfx_graph/src/nodes/surface.rs`, tests
`crates/sfx_analysis/tests/surface.rs`, example `examples/footstep.rs`. None
of these have been heard; every number below is a measurement (48 kHz).

## `rolling`

The force of something rolling over a rough surface (model family, excitation
source): a seeded random height profile with features `size` metres across,
read at `speed` and differentiated along the path. The output is the
excitation for `modal` or `resonators`, not a finished sound.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `speed` | signal | required | m/s, 0 .. 50 | Speed over the surface. 0 is exact silence; negative counts as its magnitude. A number over 50 m/s, or with `speed / size` over a quarter of the sample rate (12000 features/s), is an error; a signal is clamped to those. Mono. |
| `roughness` | number | `0.5` | 0 .. 1 | 0 is one smooth undulation of `size`; more adds grit at 1/3.7 and 1/13.7 of `size`. Outside 0..1 is an error. |
| `size` | number | `0.002` | 0.0001 .. 10 m | Feature size: bigger is lower and slower. Outside the range is an error. |
| `seed` | integer | `0` | | Another stretch of surface (mixed with the document seed and the node name). |

Errors:

- `speed: 500: give a speed under 50 m/s (a signal is clamped to it)`
- `speed: 30: at size 0.002 m that passes 15000 features a second, past 12000 (a quarter of the sample rate): give a speed under 24.000 m/s or a bigger size`

Causal, random (reads the seed). Level is proportional to speed and is not
limited: at 50 m/s the peak is about +34 dB; scale it before a `modal`.

### Recipe

A wheel speeding up over a wooden panel; give the human macros on `size`
(`speed / size` is the grit pitch) and `roughness`.

```json
[
  {"type": "ramp", "name": "v", "from": 0.2, "to": 2.0, "time": 1.0},
  {"type": "rolling", "name": "surf", "speed": "v", "size": 0.004, "seed": 3},
  {"type": "modal", "name": "panel", "excite": "surf", "material": "wood", "size": 0.45}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| speed 0.25 / 0.5 / 1 / 2 / 4 m/s: centroid | 104 / 199 / 408 / 816 / 1624 Hz (x2 per doubling) | `rolling_speed_raises_centroid_and_level` |
| same: rms | -26.4 / -20.1 / -14.3 / -8.2 / -2.2 dB (+6 dB per doubling) | same |
| speed 0 | exact zeros; a 0 -> 2 m/s ramp: rms -21.7 dB then -8.4 dB | `rolling_speed_zero_is_exact_silence` |
| roughness 0 / 0.5 / 1: centroid | 195 / 408 / 1189 Hz | `rolling_roughness_raises_centroid_and_size_lowers_it` |
| size 0.001 / 0.002 / 0.004 / 0.01 m: centroid | 816 / 408 / 199 / 84 Hz; rms -14.2 .. -14.3 dB for all | same |
| seed 1 vs 2 | STFT distance 5.03 dB | `rolling_seeds_differ_and_negative_speed_mirrors` |
| speed -1 vs 1 | bit-identical | same |
| speed +-1e9 sweep (clamped), size 0.0001 and 10, roughness 0 and 1 | finite, peak at most 33.6 dB | `rolling_is_finite_at_the_extremes_and_bit_identical` |
| into `modal` wood 0.45 m | finite; rms 4.1 dB first 0.2 s, 22.8 dB last 0.2 s | `rolling_excites_modal` |

### Honest limits

A smooth surface does not make the force pulses of a real wheel (no wheel
radius, no loss of contact). The profile is value noise through a Catmull-Rom
spline, so features have no preferred shape. Level is not normalized for
`modal`. Not heard.

## `cloth`

Rustling cloth, paper or foliage (model family, source): noise bursts whose
sizes follow a power law, arriving at a rate that follows `motion`, in four
noise bands whose centres rise with `motion` and fall with `weight`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `motion` | signal | required | 0 .. 4 | 0 is still (exact silence), 1 an arm swing. Raises burst rate (70/s x motion at weight 0.5), level and band centres (x sqrt(motion)). A number outside 0..4 is an error; a signal is clamped. Mono. |
| `weight` | number | `0.5` | 0 .. 1 | 0 silk/paper (bright, short, frequent), 1 canvas/coat (dull, long, fewer). Outside 0..1 is an error. |
| `seed` | integer | `0` | | Mixed with the document seed and the node name. |

Burst size `s` has P(size > s) = s^-1.3, cut at 40; level ~ s^0.6, decay time
3 ms x sqrt(s) x (1 .. 2.2 by weight). At most 512 bursts live at once, so
cost is bounded. Errors: `motion: 5: give a number from 0 to 4 (0 is still, 1 a swinging arm)`,
`weight: -0.1: give a number from 0 to 1`. Causal, random.

### Recipe

Foliage brushing past, growing: `ramp` 0 -> 2 on `motion`, weight 0.2.

```json
[
  {"type": "ramp", "name": "m", "from": 0.0, "to": 2.0, "time": 2.0},
  {"type": "cloth", "name": "leaves", "motion": "m", "weight": 0.2, "seed": 4}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| burst-size survival slope, s 1.5 .. 10 | -1.300 (want -1.3) | unit test `cloth_sizes_follow_the_power_law` |
| motion 0.25 / 0.5 / 1 / 2 / 4: centroid | 1606 / 2526 / 3219 / 3953 / 4935 Hz | `cloth_motion_raises_rate_band_and_level` |
| same: rms | -39.3 / -35.8 / -27.1 / -16.7 / -11.4 dB | same |
| same: events/s (density measure) | 13 / 22 / 26 / 22 / 11 (rises to motion 1, then bursts overlap) | same |
| motion 0 | exact zeros | `cloth_motion_zero_is_exact_silence` |
| weight 0 / 0.5 / 1: centroid | 3967 / 3219 / 2167 Hz | `cloth_weight_lowers_centroid` |
| motion ramp 0 -> 2 | rms -39.6 dB first quarter, -15.8 dB last; centroid 2123 -> 4238 Hz | `cloth_follows_a_motion_signal` |
| seed 1 vs 2 | STFT distance 13.35 dB | `cloth_seeds_differ_and_extremes_are_finite_within_budget` |
| 30 s, motion clamped to 4, weight 1 | 0.10 .. 0.18 s render, finite, peak 9.3 dB | same |

### Honest limits

Level grows without a ceiling (peak +9 dB at motion 4): use the output stage.
Four fixed bands, so it is a texture, not a model of a particular fabric. The
density measure saturates above motion 1, so do not use it as a rate check
there. Not heard.

## `footstep`

One footstep (model family, finished recipe like `thunder`): a heel strike and
the toe 70 .. 150 ms later on one of four surfaces. Each preset is built from
its own pulses, noise bursts and resonators (it does not use `modal` or
`contact`).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `surface` | word | required | `concrete` `gravel` `grass` `wood` | The ground. |
| `weight` | number | `0.5` | 0 .. 1 | Continuous. Louder (+6 dB across the range), longer contact, lower colour; on gravel more clusters and stones, on wood a lower body mode (215 -> 105 Hz). Outside 0..1 is an error. |
| `pace` | word or number | `walk` | `walk` 0.5, `run` 0.9, `sneak` 0.1, or 0 .. 1 | Heel-toe gap 140 ms at 0, 105 at 0.5, 70 at 1 (+-7 % by seed); level +-2 dB; gravel crunch spacing. A word and its number give identical bits. |
| `shoe` | word | `sneaker` | `hard` `boot` `sneaker` `bare` | Contact sharpness and click level (concrete centroid: hard 2400, boot 2366, sneaker 2052, bare 1834 Hz). |
| `at` | number | `0` | seconds >= 0 | When the heel lands; the step runs up to about 0.6 s after. Past the end of the sound is silence, not an error. |
| `seed` | integer | `0` | | Another step: gap, levels, tuning and stones move. |

Errors: `field surface: unknown variant lava, expected one of concrete, gravel, grass, wood`,
`pace: "jog": give walk, run, sneak, or a number from 0 to 1`,
`pace: 1.5: give a number from 0 (sneak) to 1 (run), or walk, run, sneak`,
`weight: 2: give a number from 0 (light) to 1 (heavy)`,
`shoe: "clog": give hard (leather sole, heel click), sneaker (default), boot or bare`,
`at: -1: give a number >= 0`. Everything is bounded by the document length.
`weight` and `pace` fade extra clusters/stones in; all random numbers are
drawn for the largest step, so moving them never reshuffles the step. Causal
by the flag, random. The words `surface`, `pace`, `shoe` are declared in
`Describe::WORDS`.

### Recipe

One node is the whole sound; give the human macros on `weight` and `pace`.

```json
[
  {"type": "footstep", "name": "step", "surface": "gravel", "weight": 0.6, "pace": "walk", "shoe": "boot", "seed": 2}
]
```

### Measured

Defaults (weight 0.5, walk, sneaker, node seed 1), through the default output
stage. The target is the round-0 brief's number.

| Surface | Brief | Measured | Test |
|---|---|---|---|
| concrete | 2 onsets 60-150 ms apart; attack under 10 ms; centroid 0.8-3 kHz; 0.15-0.4 s | onsets 0 and 107 ms; attack 2.0 ms; centroid 2027 Hz; audible 0.27 s; no pitch | `footstep_concrete_is_heel_then_toe` |
| gravel | 8+ onsets; high flatness; centroid 2-6 kHz; 0.25-0.6 s | 11 onsets, 13 density events, flatness 0.089 (occupied band 0.61), centroid 3824 Hz, audible 0.54 s | `footstep_gravel_is_many_flat_ticks` |
| grass | attack 10-40 ms; no pitch; centroid 1.5-5 kHz; 0.25-0.6 s | attack 21.0 ms, no pitch, centroid 4013 Hz, audible 0.41 s | `footstep_grass_is_a_soft_pitchless_swish` |
| wood | low body 80-300 Hz under a short tick; 0.2-0.5 s | body mode 153 Hz (decay 0.51 s), strongest peak 154 Hz; 1.5-6 kHz -38.1 dB in the first 15 ms and -120 dB from 130 ms; body band -34.1 dB; audible 0.23 s | `footstep_wood_has_a_low_body_under_a_short_tick` |

| What | Measured | Test |
|---|---|---|
| weight 0 .. 1 in 5 steps: centroid (concrete) | 2288 / 2230 / 2052 / 1938 / 1742 Hz, strictly falling | `footstep_weight_is_continuous_and_darkens` |
| same, gravel / grass / wood | 4310 -> 2933 / 4819 -> 2830 / 1169 -> 406 Hz, strictly falling; rms rises every step (about 6 .. 10 dB over the range) | same |
| pace 0 / .25 / .5 / .75 / 1: measured heel-toe gap (concrete) | 149 / 128 / 107 / 85 / 75 ms | `footstep_pace_sets_the_gap_continuously` |
| gravel vs wood | flatness 0.089 vs 0.013; onsets 11 vs 2 | `footstep_surfaces_differ_in_flatness_and_onsets` |
| 6 document seeds, STFT distance between any two | concrete 5.06 .. 6.18, gravel 8.07 .. 11.50, grass 5.47 .. 5.86, wood 4.81 .. 10.09 dB (bound asserted: above 1, under 14) | `footstep_seeds_make_different_steps` |
| same: rms spread / gap spread over the 6 seeds | 1.1 .. 1.8 dB; 10.7 ms | same |
| node `seed` 1 vs 2 | 6.08 / 10.62 / 5.86 / 9.80 dB | same |
| weight/pace/shoe extremes, 4 surfaces | finite, highest raw peak -0.3 dB | `footstep_is_finite_at_the_extremes_and_bit_identical` |
| default peak through the output stage | concrete -12.8, gravel -13.7, grass -12.3, wood -14.9 dBFS | `footstep_one_liners_render_through_the_output_stage` |

Gravel onsets over 6 seeds at 3 weights and 3 paces (a scratch probe, not a
committed test): 9 .. 12 (default weight and walk: 11). Gravel audible
length 0.37 s (pace 0.9, weight 0) to 0.64 s (pace 0.1, weight 1).

### Honest limits

- The brief's lengths and onset counts are met at the defaults; at weight 1
  with a slow pace gravel runs to about 0.64 s (over the brief's 0.6 s).
- The onset detector merges events under 30 ms, so gravel's 8+ onsets come
  from crunch clusters spaced 31 ms or more, not from single stones; the
  density measure (13 events) counts stones.
- The wood `attack` descriptor can read 0.1 s when the toe is the larger hit
  (a detector quirk, not measured in a test).
- One heel and one toe; there is no stride sequence (use `pattern` /
  `seq` with several nodes, or `sound` instances).
- Levels are set by a fixed trim per surface so a default step peaks near
  -12 dBFS; use `output.lufs` to set loudness.
- Not heard. Whether the presets read as the named ground is the human's call.

## `footstep` `body` and `swish`

Round 2 (footstep used in 4 of 4 sounds): no way to move the body pitch apart
from `weight`, and no balance between the continuous swish and the
stalk / stone-tick layer.

### New fields (optional; omitted = bit-identical to before)

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `body` | number | `1` | 0.5 .. 2 | Multiplies the resonant body frequency, independent of `weight`. Wood: all three body modes (`f0`, 1.74 f0, 2.9 f0). Concrete: the low thud mode (120 Hz / sqrt(tilt)); the mid modes and the scuffs do not move. Gravel: the thump (95 Hz). Grass: the soft low noise band (140 Hz). 0.5 is an octave down, 2 an octave up. |
| `swish` | number | `0.5` | 0 .. 1 | Grass: swish bands x `1 + 2 (swish - 0.5)`, stalk ticks x `1 - 2 (swish - 0.5)` (0 = ticks only at 2x, 1 = swish only). Gravel: stone ticks x `1 - 1.2 (swish - 0.5)` (1.6x at 0, 0.4x at 1) and, above 0.5, a swish bed (the grass band, 0.35 x level x `2 (swish - 0.5)`) fades in. Concrete and wood ignore it. |

"The current value" of `swish` is 0.5: the neutral setting at which both
layers have exactly the old gain (the gains are written `1 +- (swish - 0.5) k`
so 0.5 multiplies by exactly 1.0). The gravel bed is drawn after every other
random number, and only when `swish` > 0.5, so no other part of the step
reshuffles when it moves.

Errors: `body: 3: give a number from 0.5 (an octave down) to 2 (an octave up), default 1`,
`swish: 1.5: give a number from 0 (ticks only) to 1 (swish only), default 0.5`.

### Measured

| What | Measured | Test |
|---|---|---|
| defaults: 9 renders (4 surfaces default, 4 heavy run boot seed 3, wood hard light sneak seed 5) | FNV-1a of the f32 bits unchanged from before the fields existed | `body_and_swish_defaults_are_bit_identical_to_before` |
| wood strongest low mode, body 0.5 -> 2 (ratio) | weight 0.2: 91.6 -> 376.9 Hz (4.11); weight 0.5: 78.3 -> 305.5 Hz (3.90); weight 0.8: 61.9 -> 245.0 Hz (3.96); asserted 4 +- 10 % | `footstep_body_moves_the_wood_mode_by_the_factor` |
| centroid below 400 Hz, body 0.5 / .71 / 1 / 1.41 / 2 | concrete 90 / 96 / 114 / 146 / 194; gravel 64 / 75 / 95 / 127 / 172; grass 148 / 172 / 198 / 225 / 250 Hz, strictly rising | `footstep_body_is_independent_of_weight_and_moves_concrete_gravel_grass` |
| grass energy share above 2 kHz, swish 0 / .25 / .5 / .75 / 1 | 0.104 / 0.302 / 0.561 / 0.701 / 0.771, strictly rising | `footstep_swish_trades_the_swish_for_the_ticks_on_grass` |
| gravel share above 2 kHz, same | 0.902 / 0.901 / 0.898 / 0.891 / 0.882, strictly falling; rms (raw) -35.4 / -37.1 / -39.0 / -37.9 / -34.1 dB (not monotonic: fewer stones, then the bed) | `footstep_swish_on_gravel_fades_stones_and_adds_a_bed` |

Grass: the share above 2 kHz rises with swish because the ticks are quiet
against the 140 Hz body band; at swish 0 the low body dominates, at 1 the
3.1 kHz swish does. Whether it sounds right is the human's call; give them
macros on `body` and `swish`.

## A swish, not ticks

Batch 2 blind listen (docs/history/labels/batch2_findings.md): `cloth` 3/3 heard as
"pellets of something hard flying out of a hopper", "metal objects falling".
Measured cause: each burst was an instant-attack, 3 ms x sqrt(size) decay of
noise through a narrow band-pass (q 1.1): a train of clicky, faintly pitched
ticks.

Change (crates/sfx_graph/src/nodes/surface.rs, no new fields):
- each burst is a difference of exponentials: decay 25 ms x sqrt(size) x
  (1..2.2 by weight), capped at 0.25 s; attack 0.35 x decay, at least 6 ms;
- burst level ~ size^0.35 (was size^0.6), so the power-law large bursts no
  longer stand out as hits;
- a continuous rubbing bed in all four bands, level ~ motion^0.75 with the
  motion smoothed over 40 ms; band centres follow that smoothed motion;
- bands are wider (q 0.6). Motion 0 is still exact silence.

Measured, 2 s, seeds 1-3 (tests/cloth_fix.rs):

| | before (motion 0.5, weight 0) | after (all motion 0.5..2, weight 0..1) |
|---|---|---|
| crest | 25.5-27.0 dB | 13.1-16.5 dB |
| fine onsets (5 ms) | 28.5-31 /s | 0-0.5 /s |
| occupied-band flatness | 0.68-0.80 | 0.77-0.92 |

Motion still raises level and centroid (motion 0.5 / 1 / 4 at weight 0.5:
-42.2 / -33.5 / -17.0 dB rms, 4271 / 4763 / 6568 Hz); weight lowers the
centroid (5960 / 4763 / 3738 Hz at 0 / 0.5 / 1). The `density` claim no
longer fits a cloth knob: the 1 ms counter reads noise grain (375-674 /s).
Not heard: whether it reads as fabric is the listener's call.
