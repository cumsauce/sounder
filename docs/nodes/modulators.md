# Chaos, fbm, curve, switch, pitch_follow, centroid_follow

Six control-family nodes in `crates/sfx_graph/src/nodes/modulators.rs`, tested
in `crates/sfx_analysis/tests/it/modulators.rs`. `chaos` and `fbm` make control
signals; `curve` reshapes one; `switch` picks between sounds; `pitch_follow`
and `centroid_follow` turn audio into a Hz control signal.

A signal field takes a number, `"$param"`, `"=expr"` or the name of another
node. Errors name the field and the value and say what to give. A number
outside a range is an error; a signal outside it is clamped per sample.

## `chaos`

A chaotic control signal (control family, source): the logistic map, the
Henon map or the Lorenz system, mapped onto -1..1 or `range`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `model` | word | `logistic` | `logistic` `lorenz` `henon` | The system. |
| `rate` | number | required | logistic / henon: steps a second, 0 < rate <= sample rate. lorenz: time units a second, 0 < rate <= 2000 | How fast it runs. Map values are joined linearly between steps. |
| `param` | number | 3.9 / 28 / 1.4 | logistic r 0..4, lorenz rho 0..100, henon a 0..1.5 | The model's parameter. |
| `range` | [lo, hi] | -1..1 | finite numbers | Maps the output onto [lo, hi]; the output is clamped, never outside. |
| `seed` | integer | 0 | | Picks the start state (mixed with the document seed and the node name). |

Scaling: logistic x -> 2x - 1; henon x / 1.3; lorenz x / 20; each clamped to
-1..1. Lorenz is RK4 with sub-steps of at most 0.01 time units, so at the
rate cap of 2000 it takes 5 sub-steps a sample. Errors:

- `rate: 99999: a step per sample at most (48000 a second): lower it`
- `rate: 3000: lorenz integrates at most 2000 time units a second: lower it`
- `param: 5: the logistic r is 0 to 4 (3.9 is chaotic)`

Causal, random (reads the seed).

### Recipe

A wobbling pitch: a Lorenz control signal, shaped by a `curve` into Hz, plus a
slow `fbm` drift, driving a saw (the document in the `fbm` and `curve` recipe
below). Give the human a macro on the chaos `rate` and on the `curve` points.

## `fbm`

Fractal random motion (control family, source): `octaves` layers of smooth
random points (Catmull-Rom through seeded points, as `wander`), octave k at
`rate * 2^k` points a second with amplitude `gain^k`, summed and divided by
the sum of the amplitudes.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `rate` | number | required | > 0, and `rate * 2^(octaves-1)` <= sample rate | Points a second in the slowest octave. |
| `octaves` | integer | 4 | 1 .. 12 | Layers stacked. |
| `gain` | number | 0.5 | 0 .. 1 | Amplitude of each octave relative to the one below. |
| `range` | [lo, hi] | -1..1 | finite numbers | Maps the output onto [lo, hi]; never outside it. |
| `seed` | integer | 0 | | Each octave has its own stream, mixed with the document seed and the node name. |

Errors: `octaves: 13: give 1 to 12`; `rate: 40000 with octaves 4: the fastest
octave would run at 320000 points a second, past the sample rate 48000: lower
rate or octaves`. Causal, random.

### Recipe (with `chaos` and `curve`)

```json
[
  {"type": "fbm", "name": "drift", "rate": 1.5, "octaves": 4, "range": [-1, 1], "seed": 5},
  {"type": "chaos", "name": "wobble", "model": "lorenz", "rate": 6, "range": [0, 1], "seed": 2},
  {"type": "curve", "name": "pitch", "input": "wobble", "points": [[0, 180], [0.5, 240], [1, 520]], "curve": "smooth"},
  {"type": "math", "name": "bend", "op": "mul", "a": "drift", "b": 30},
  {"type": "math", "name": "hz", "op": "add", "a": "pitch", "b": "bend"},
  {"type": "osc", "name": "voice", "shape": "saw", "freq": "hz", "amp": 0.4},
  {"type": "osc", "name": "air", "freq": 1100, "amp": 0.2},
  {"type": "pitch_follow", "name": "heard", "input": "voice", "min": 100, "max": 800},
  {"type": "ramp", "name": "pick", "from": 0, "to": 1, "time": 1.0},
  {"type": "switch", "name": "choose", "inputs": ["voice", "air"], "select": "pick", "fade": 0.02}
]
```

## `curve`

A lookup table (control family, shaper): the input is x, the output is y read
from the breakpoints, clamped at both ends. The segment is found by binary
search, so an x equal to a breakpoint returns that breakpoint's y exactly.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono | The x signal. A non-finite sample reads as the first y. |
| `points` | [[x, y], ..] | required | 2 .. 4096 pairs, x strictly increasing, all finite | The table. |
| `curve` | word | `linear` | `linear` `smooth` | Between points: straight, or a cosine ease (flat slope at every point). |

Errors: `points[1]: x 0 is not above the previous x 0: sort the points by x,
each x different`; `points: 1 pairs: give 2 to 4096 [x, y] pairs`. Causal, not
random.

## `switch`

Picks one of several sounds by a control signal (mix family).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `inputs` | [signal] | required | 2 .. 64, mono or stereo | The sounds. If any is stereo the output is stereo, mono ones on both sides. |
| `select` | signal | required | mono | Index of the input to play: rounded to the nearest, clamped to 0..n-1; 0 is the first. A non-finite sample reads as 0. |
| `fade` | number | 0.005 | 0 .. 10 s | Linear crossfade (gains always sum to 1) when `select` changes. 0 switches at once. A change during a fade turns it toward the new input. |
| `blend` | bool | false | | `true`: `select` blends the two inputs either side of it by its fraction (1.25 is 75 % of input 1, 25 % of input 2). `fade` is ignored. |

Decision: the card says fractional `select` blends "when `fade` is 0 and
`blend: true`"; here `blend: true` alone is enough and overrides `fade`
(the default `fade` is 0.005, so requiring both would make `blend: true` an
error for no reason). Errors: `inputs: 1 inputs: give 2 to 64`. Causal, not
random.

## `pitch_follow`

Follows the fundamental of the input (analysis family): a YIN-style detector
on the whole buffer, output in Hz.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono | The sound to follow. |
| `min` | number | 50 | Hz, >= 20 | Lowest pitch looked for; the window is one period of it (20 ms at 50 Hz). |
| `max` | number | 2000 | Hz, > min and <= sample rate / 4 | Highest pitch looked for. |
| `smooth` | number | 0.02 | seconds, 0 .. 10 | One-pole smoothing run forwards and backwards (no lag); 0 is unsmoothed. |

Method: frames every 5 ms; the difference function by FFT cross-correlation;
cumulative mean normalisation; the first lag under 0.15, walked down to its
minimum, refined by a parabola; Hz = sample rate / lag. A frame is confident
only if a lag is found under 0.15, the window is louder than -80 dBFS rms,
and the two windows it compares lie inside the signal. The frame's time is
the middle of the two windows. Unconfident frames are skipped, so the track
**holds the last confident value** (and the first before it). At the two ends
the track continues the slope (log Hz, at most a factor of 2) of the four
frames nearest the end, because the first and last frames sit about half a
window inside the signal. A signal never confident (noise, silence, shorter
than `min` period + shortest period) gives 0 Hz everywhere. Errors:
`min: 10: give a number of 20 Hz or more`; `max: 20000: give a number above
min (50) and at most 12000 Hz (a quarter of the sample rate)`.

Acausal (`causal()` is false): frames look ahead of their time. Not random.

## `centroid_follow`

Follows the spectral centroid of the input (analysis family), in Hz.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono | The sound to follow. |
| `window` | number | 0.02 | seconds; 32 .. 32768 samples (0.00067 .. 0.683 s at 48 kHz) | Hann window; frames every window / 4, centred on the frame time, joined linearly. |

The centroid is sum(f P) / sum(P) over the bins above DC of the windowed power
spectrum. A frame under -90 dBFS rms (windowed) is skipped and the track holds;
a signal that is always silent gives 0. Error: `window: 5: give between
0.00066666666 and 0.68266666 seconds (32 to 32768 samples)`. Acausal, not random.

### Measured

Tests in `crates/sfx_analysis/tests/it/modulators.rs`, 48 kHz.

| What | Measured | Test |
|---|---|---|
| `fbm` rate 2, 6 octaves, range 100..300, 20 s: min / max | 123.55 / 269.81 (inside) | `fbm_spectrum_falls_and_stays_in_range` |
| same: spectral density dB/Hz in 0.2-1, 1-4, 4-16, 16-64, 64-256 Hz | 29.3, 17.2, -1.4, -21.0, -65.7 | same |
| `fbm` seeds 1 vs 2 / seed 1 twice | max difference 1.295 / identical | `fbm_seeds` |
| `fbm` mean absolute step, 1 octave vs 6 octaves gain 1 | 2.79e-5 vs 1.75e-4 | `fbm_octaves_and_gain_shape_it` |
| `lorenz` range -3..5, 30 s, rate 0.5 / 10 / 200 / 2000 | min -2.01 / -2.66 / -2.84 / -2.88, max 4.93 / 4.93 / 4.92 / 4.89 (all inside), std 1.0 to 1.6 | `chaos_lorenz_stays_in_range_for_30_s_at_the_rate_cap` |
| logistic (30 and 48000 steps/s) and henon (100 and 48000), 10 s | all inside -3..5, std 1.6 to 2.4 | `chaos_maps_are_bounded_and_seeded` |
| logistic seeds 1 vs 2 | max difference 6.92 | same |
| param 0 / 2 / 4 (logistic), 0 / 1.5 (henon), 0 / 100 (lorenz) at the rate cap | finite, inside range | same |
| `curve` linear and smooth at every breakpoint | exact (0 error, same bits) | `curve_hits_each_breakpoint_exactly` |
| `curve` at -0.625 (between -1/0 and -0.25/0.8), both curves; clamps | 0.4; 0 below, 1 above | same |
| `switch` 200 -> 800 Hz at 0.4123 s: `clicks()` / max sample step | fade 0.005: 0 clicks, 0.0523; fade 0: 1 click, 0.5339 | `switch_between_two_sines_does_not_click` |
| same: pitch before / after | 200.0 / 800.0 Hz | same |
| `switch` blend, select 0 -> 2 over 1 s of inputs 1, 3, 5 | 1.000 at 0, 3.000 at 0.5 s, 4.000 at 0.75 s | `switch_blend_and_stereo` |
| `switch` with a hard-left stereo input | stereo out, rms L -13.6 dB, R -38.3 dB | same |
| `pitch_follow` 200 -> 800 Hz exponential sweep, 1 s | worst error 0.09 % from 50 ms to the last sample (0.07 % at 20 ms, -0.01 % at 0.999 s); card limit 2 % | `pitch_follow_tracks_an_exponential_sweep` |
| `pitch_follow` 300 Hz tone then silence | 300.01 Hz at 0.1 s, 300.93 Hz at 0.9 s (held; 0.3 % off) | `pitch_follow_steady_tone_holds_through_silence` |
| `pitch_follow` white noise | 0 of 24000 samples non-zero | same |
| `centroid_follow` sine 500 / 1000 / 4000 Hz | 500.000 / 1000.000 / 4000.000 Hz | `centroid_follow_reads_tones_noise_and_a_sweep` |
| same: white noise | 12455 Hz | same |
| same: 200 -> 3200 Hz exponential sweep | worst error 0.0028 % (0.05 s to 0.95 s) | same |
| every node at the ends of each field, input amplitude 1e6, +-1e9 Hz sweep as signal | all finite, exactly the document length | `modulators_are_finite_at_the_extremes` |
| all six in one document, one thread vs the pool | same bits | `modulators_are_bit_identical` |
| recipe above, 1 s | voice 195..525 Hz, `pitch_follow` median error 1.5 % (0.1 to 0.9 s), output rms -14.4 dB | `modulators_recipe_renders` |

### Honest limits

- `chaos`: the logistic map at `rate` near the sample rate is close to white
  noise; at low rates the linear joins between steps show as corners. Lorenz
  `rho` under about 24.7 settles to a constant (the output then stops moving).
  Henon `a` above about 1.42 can escape; the state is then restarted from a
  seeded value, so the output jumps. The logistic map restarts from a seeded
  value if floating point lands on 0 or 1. None of these outputs were
  compared with a reference integrator; the tests check bounds and motion.
- `fbm`: the amplitude normalisation is by the sum of the layer amplitudes, so
  the typical level is well under the range ends (the measured 20 s run used
  123..270 of 100..300).
- `curve`: no extrapolation; a table with a flat end clamps.
- `switch`: the crossfade is linear (gains sum to 1), which dips in level
  when the two inputs are uncorrelated; it is not equal power.
- `pitch_follow`: monophonic, one period of `min` of latency inside each
  frame; octave errors are possible on sounds with a weak fundamental (only
  the clean sweep, a steady tone and a saw recipe were measured); on a pitch
  change faster than about 2 octaves a second the window averages over it. The
  slope extension at the ends is a guess from four frames. It reads the
  fundamental of a signal, not of a chord.
- `centroid_follow`: a window shorter than a few periods of the lowest
  component smears it. Frames near the two ends see a window half full of
  zeros.
- None of this has been heard.
