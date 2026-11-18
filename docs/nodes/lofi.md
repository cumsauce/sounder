# `crossfade_loop`, resample, tape, stutter

All four live in `crates/sfx_graph/src/nodes/lofi.rs`; the tests are in
`crates/sfx_analysis/tests/it/lofi.rs`. A signal field takes a number,
`"$param"`, `"=expr"` or the name of another node; the fields below that are
plain numbers (`fade`, `wow`, `flutter`, `drive`, `hiss_db`, `tone`, `slice`,
`repeats`) take `"$param"` and `"=expr"` but not a node.

## `crossfade_loop`

An offline node (loops family): makes the wrap of a loop continuous by
crossfading the material on both sides of the seam. It is not causal (it
reads the end of the sound to write the start).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | The sound to make loopable (a loop's whole length). Stereo: both channels use the same ramps. |
| `fade` | number | `0.25` | seconds, more than 0 (at least 2 samples), at most half the sound | Length of each of the two crossfades. |

What it does: let `x` be the input, `n` its length, `F` the fade in samples.
`y` is `x` except in the first `F` and the last `F` samples, where `x` is
blended with `x` read `F` samples earlier (circularly): `y[i] = sin(t) x[i] +
cos(t) x[(i - F) mod n]`, with `t` going from 0 at the wrap to pi/2 at the
end of the ramp. So `y[0] = x[n - F]` and `y[n - 1] = x[n - F - 1]`: the two
sides of the wrap are neighbouring samples of the input. The output is the
input's length. The middle (further than `F` from both ends) is the input
bit for bit; the ramps are equal power, so uncorrelated material keeps its
level.

Errors: `fade: 0: give seconds more than 0`; `fade: 0.6: the sound is 1.000 s
long and each crossfade takes fade: give at most 0.500 s (or make the sound
longer with duration)`; `fade: 0.00001: that is under 2 samples: give at
least 0.000041666666 s`.

### Recipe

A wind bed that loops: pink noise through a band-pass whose centre moves by
`wander`, in a `loop` document with a `duration` of 2. Give the human macros
on the filter `q` and the `wander` range; `fade` is a plain number, so it
can be a `"$param"` they set too.

```json
[
  {"type": "noise", "name": "air", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 0.3, "range": [300, 2500], "seed": 2},
  {"type": "filter", "name": "band", "input": "air", "mode": "bp", "freq": "sweep", "q": 3},
  {"type": "crossfade_loop", "name": "seamless", "input": "band", "fade": 0.4}
]
```

### Measured

Tests in `crates/sfx_analysis/tests/it/lofi.rs`, 48 kHz, 2 s loops (`kind:
"loop"`), `Expect::defaults_for` (so `loop_seam` is in the checks).

| What | Measured | Test |
|---|---|---|
| brown noise, 40 seeds, default fade | 21 fail `loop_seam` before, 0 after; whole-sound rms moves by at most 0.16 dB | `crossfade_loop_makes_noise_loops_pass_loop_seam` |
| white noise and pink noise, 60 seeds each | 0 fail before (they already pass) | scan while writing the test |
| slow wander filter noise (rate 0.3, 200 to 4000 Hz, q 3, seed 2) | before: wrap 2.66x, spectral 4.3 dB (FAIL); after: wrap 0.27x, spectral 2.3 dB (PASS); all default checks pass | same |
| same: level away from the seam | middle (further than 0.25 s from both ends) identical bit for bit; whole-sound rms -14.93 -> -14.83 dB | same |
| 24 wander loops (q 3 and 8, 12 seeds) at 0.3 targets/s, fade 0.25 | 18 fail before, 0 after | same |
| same at 0.5 targets/s, fade 0.25 / 0.5 | 14 fail before; 2 / 1 after | same |
| same at 1 target/s, fade 0.5 | 14 fail before; 2 after | same |
| 72 wander loops (0.5, 1, 2 targets/s), fade 0.1 / 0.25 / 0.5 / 0.75 / 1.0 | 23 / 11 / 3 / 8 / 2 fail | scratch scan |
| white noise, fade 0.25: rms of the middle, first 0.25 s, last 0.25 s | -4.77, -4.78, -4.81 dB | `crossfade_loop_is_equal_power_and_wraps_into_the_neighbouring_sample` |
| `y[0]`, `y[n-1]` for fades 0.001, 0.05, 0.25, 1.0 | exactly `x[n-F]`, `x[n-F-1]` | same |
| recipe above | `loop_seam` passes (wrap 0.49x, spectral 1.8 dB); without the node the same loop also passed (spectral 5.8 dB, limit 6) | `lofi_recipes_render` |
| stereo, fades 0.5 s (maximum) and 2 samples; input amplitude 1e6; two renders | finite; bit-identical | `crossfade_loop_stereo_is_bit_identical_finite_and_errors_explain` |

### Honest limits

- The spectral half of `loop_seam` compares the first and last 0.34 s of the
  loop. The node makes the two sides of the wrap neighbouring samples of the
  input, but a sound whose spectrum changes faster than that (a filter
  centre moving more than about once every 2 s over 200 to 4000 Hz) can
  still fail the spectral half: at 1 target per second and above, 2 to 5 of 24
  loops did, and at 3 per second 12 of 24. A longer `fade` helps (table
  above); a faster modulation needs to be periodic in the duration.
- The wrap-jump half passed in every case measured (all failures after the
  node were the spectral half).
- It changes the sound within `fade` of both ends: those 2 x `fade` seconds
  hold a blend of material from elsewhere in the sound. It shortens nothing.
- Tonal material that is correlated with itself `fade` earlier (a steady
  drone) does not keep its level through an equal-power ramp: the sum of two
  equal in-phase copies is up to 3 dB louder in the middle of the ramp,
  out-of-phase copies cancel. This was not measured.
- A mean (DC) offset can move: brown noise, which is mostly sub-audio, newly
  failed the `dc` check on 2 of 40 seeds. Follow it with a high-pass.
- None of this has been heard.

## `resample`

A lo-fi node (filter family): sample-rate reduction with hold. The input is
sampled `rate` times a second and each sample is held until the next.
Causal, not random.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | The sound to reduce. |
| `rate` | signal | required | Hz, 1 .. sample rate | Samples a second. A node sweeps it; a signal is clamped to the range per sample (a non-finite sample counts as 1 Hz). The sample rate passes the input through unchanged. Must be mono. |
| `antialias` | bool | `false` | | Low-pass the input before sampling and the held signal after it, 8th order Butterworth at 0.45 x `rate` (four trapezoidal state-variable sections, stable under a fast `rate`). Skipped while `rate` is the sample rate. |

The held sample is the first of each hold interval (a hold of `rate` 12000 at
48 kHz repeats the input's sample 0, 4, 8, ...). A non-integer ratio holds
for the nearest whole samples (6 or 7 at `rate` 7000).

Errors: `rate: 0: give a number from 1 to 48000 Hz (the sample rate)`;
`rate: 60000: give a number from 1 to 48000 Hz (the sample rate)`.

### Recipe

A crunchy hold of a sawtooth. Give the human a macro on `rate` (its claim is
`centroid`, direction `1`: a higher rate is brighter).

```json
[
  {"type": "osc", "name": "tone", "shape": "saw", "freq": 220, "amp": 0.5},
  {"type": "resample", "name": "crunch", "input": "tone", "rate": 6000}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| 1 kHz sine, rate 8000, no antialias: images at 7 and 9 kHz relative to 1 kHz | -16.6 and -18.6 dB | `resample_hold_makes_images_and_antialias_removes_them` |
| same with `antialias`: 7 and 9 kHz | -66.6 and -89.7 dB (50.0 and 71.1 dB lower; the card asks 30); 1 kHz level changes -0.00 dB | same |
| 7 kHz sine, rate 8000, no antialias: alias at 1 kHz | -9.2 dB; with `antialias` -59.3 dB (50.0 dB lower) | same |
| rate 12000 at 48 kHz | every group of 4 output samples is the first of the input's 4, bit for bit | `resample_holds_exactly_and_takes_a_swept_rate` |
| rate 48000 | the input bit for bit | same |
| rate 7000 | held runs are 6 or 7 samples (48000 / 7000 = 6.86) | same |
| stereo input, `rate` swept 20000 -> 300 Hz with `antialias`; two renders | 2 channels, finite, bit-identical | same |
| noise at amplitude 1e6, `antialias`, rate 1 / 48000 / signal at +-1e9 | finite; peak 0 (rate 1: only the first, filtered, sample is held) / 9.999e5 / 9.994e5 | `resample_is_finite_at_the_extremes_and_errors_explain` |

### Honest limits

- The hold is a zero-order hold, so its images are the hold's own sinc
  roll-off (-17 dB at 7 kHz of an 8 kHz rate); `antialias` removes them with
  a filter after the hold, so it also rounds the steps: it is a band-limited
  decimator, not a bit-exact hold.
- With `antialias` the input low-pass and the held signal's low-pass add a
  group delay of several samples near the corner. Not measured.
- With `antialias` the first held sample is the input filter's first
  output, which is near 0 even for a signal that starts loud (measured: a
  1e6 amplitude noise at rate 1 gives peak 0).
- Stepping `rate` per sample with `antialias` retunes both filters each
  sample; the trapezoidal sections stay finite (table above) but the output
  is not a clean decimation while it moves.
- None of this has been heard.

## `tape`

A lo-fi effect (modulation / nonlinear family): speed wobble (wow and
flutter) by a modulated delay, soft saturation, hiss and a tone roll-off.
Causal; random (the seed picks the wobble frequencies and phases and the
hiss).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | Both channels share one speed wobble; the hiss is separate per channel. The output is the input delayed by 1 sample plus the wobble's mean delay (about 0.5 ms at the defaults). |
| `wow` | number | `0.2` | percent of speed, 0 .. 5 | Peak speed deviation of the slow wobble: one sinusoid at a seeded 0.4 to 1.2 Hz. 1 percent moves a 1 kHz tone by +-10 Hz. |
| `flutter` | number | `0.1` | percent of speed, 0 .. 5 | Peak speed deviation of the fast wobble: two sinusoids (0.65 and 0.35 of the depth) at seeded 5 to 9 Hz and 9 to 15 Hz. |
| `drive` | number | `1` | 0 .. 40 | Saturation `tanh(drive x) / drive`: the input for small signals, limited to `1 / drive`; 0 (and under 0.001) is off. Not oversampled. |
| `hiss_db` | number | `-60` | dBFS RMS, -120 .. 0 | White noise at this RMS level added after the delay and before the tone filter; -120 is off. |
| `tone` | number | `8000` | Hz, 20 .. 0.45 x sample rate (21600 at 48 kHz) | Corner of a 12 dB/oct low-pass (Q 0.707) on the whole output, hiss included. |
| `seed` | integer | `0` | | Mixed with the document seed and the node name. |

Order: saturation, then the wobbling delay, then hiss, then the tone filter.
The delay is `amp (1 + cos)` seconds per wobble, so it is never negative and
its slope is minus the speed deviation. It is read with third-order Lagrange
interpolation.

Errors: `wow: 6: give percent of speed from 0 to 5` (and the same for
`flutter`); `drive: 100: give a number from 0 to 40`; `hiss_db: 5: give dBFS
from -120 (off) to 0`; `tone: 30000: give Hz from 20 to 21600`.

### Recipe

An old cassette on a held chord. Give the human macros on `wow`, `hiss_db`
and `tone` (a claim of `centroid`, direction `1`, holds for `tone`; `wow` has
no measure to claim).

```json
[
  {"type": "osc", "name": "a", "freq": 330, "amp": 0.3},
  {"type": "osc", "name": "b", "freq": 495, "amp": 0.3},
  {"type": "mix", "name": "chord", "inputs": ["a", "b"]},
  {"type": "tape", "name": "old", "input": "chord", "wow": 0.6, "flutter": 0.3, "drive": 2, "hiss_db": -50, "tone": 5000, "seed": 3}
]
```

### Measured

Pitch is the frequency of each cycle of a 1 kHz sine (rising zero crossings,
interpolated), fitted with a least-squares sinusoid.

| What | Measured | Test |
|---|---|---|
| wow 1 percent, seeds 1 / 2 / 3: strongest component, amplitude | 1.20 Hz 9.99 Hz; 0.72 Hz 9.97 Hz; 0.50 Hz 9.96 Hz (want 10.0; the card allows 30 %) | `tape_wow_modulates_the_pitch_by_the_set_depth_below_2_hz` |
| wow 0.5 / 2 percent | 4.99 Hz at 0.94 Hz (want 5.0); 19.96 Hz at 0.70 Hz (want 20.0) | same |
| wow only: component between 3 and 20 Hz | under 1 Hz (limit 10 % of the depth) | same |
| flutter 1 percent: pitch deviation rms | 5.22 Hz (want 5.22); strongest component 6.50 Hz at 8.84 Hz; under 2 Hz 0.14 Hz | `tape_flutter_is_fast_and_wow_flutter_off_is_clean` |
| wow, flutter, drive, hiss off, tone 21600 | level change 0.000 dB; output within 0.0073 of the input delayed by 1 sample | same |
| drive 4: 0.01 amplitude 1 kHz tone | -0.003 dB | `tape_drive_hiss_and_tone` |
| drive 4: 0.9 amplitude tone | peak 0.2506 (`tanh(3.6) / 4` = 0.2496; the tone filter overshoots 0.4 %); 3rd harmonic -11.7 dB re fundamental | same |
| hiss -40 dB on silence: rms with tone 21600 / 2400 Hz | -40.50 / -49.55 dB (want about -40.5 / -49.5) | same |
| tone 2000: level at 200 / 2000 / 8000 Hz | 0.00 / -3.01 / -25.69 dB | same |
| same seed twice; seeds 1 and 2 | identical; different | same |
| all fields at their ends (wow 5, flutter 5, drive 40, hiss 0, tone 20 / 21600), input amplitude 1e6 | finite; peaks 6.4e-2 (drive 40 limits to 0.025 and the tone filter at 20 Hz) / 1.65e6 (everything off) | `tape_is_bit_identical_stereo_finite_at_the_extremes_and_errors_explain` |
| stereo input; two renders | 2 channels that differ (separate hiss); bit-identical | same |
| recipe above, 2 s | finite, rms -12.30 dB | `lofi_recipes_render` |

### Honest limits

- The output is delayed: 1 sample plus the mean wobble delay (`amp` of each
  wobble, up to about 40 ms at wow 5 percent and 0.4 Hz). A transient lands
  late by that much. Not corrected, so layering a dry copy needs an offset.
- Wow and flutter are fixed sinusoids with seeded frequency and phase, not
  the random drift of a real transport, so a long render repeats its
  pattern exactly.
- The hiss is white (uniform noise) before the tone filter, not shaped like
  tape hiss. Both channels' hiss is separate, the wobble shared.
- `drive` is not oversampled: at drive 4 and a 1 kHz tone the 3rd harmonic
  is at -11.7 dB; the aliasing of high drives on bright material was not
  measured. Put a `shape` with oversampling first for that.
- None of this has been heard.

## `stutter`

A lo-fi effect (time family): at each event the last slice of the sound
repeats. Causal unless `reverse`; not random.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | The sound. |
| `events` | events | required | event stream | `pattern`, `poisson`, `burst`, ... Each event starts a stutter at its time. Events past the end are ignored; an event at 0 has no earlier slice and does nothing. A later event takes over from an earlier one still repeating, and repeats what is then playing. |
| `slice` | number | `0.05` | seconds, 0.0001 .. 10 | Length of the repeated slice. An event nearer the start than this repeats the sound so far. |
| `repeats` | integer | `4` | 1 .. 256 | Copies played after the event. The sound resumes after `repeats` x `slice` seconds. |
| `reverse` | bool | `false` | | Play the copies backwards. The slice is then the one that follows the event (the material the stutter replaces), reversed, so the node reads ahead and is **not causal** (`summary` reports it). |

Each copy fades in over 2 ms (or half the slice if shorter) from the sound's
own continuation after the event, and the last copy fades back out the same
way, so the joins have no jump. The fades are linear: the two sides are
usually the same material.

Work cap: events x min(`repeats` x `slice`, sound length) in samples must
not pass 400 million; the error says `events: 10001 events of 48000 samples
is over the limit of 400000000 samples: use fewer events, a shorter slice or
fewer repeats`. Other errors: `slice: 20: give seconds from 0.0001 to 10`;
`repeats: 0: give a whole number from 1 to 256`.

### Recipe

A skipping record: the last 80 ms repeat 3 times at 0.6 s and again at
1.3 s. Give the human macros on `slice` and `repeats`.

```json
[
  {"type": "ramp", "name": "pitch", "from": 220, "to": 440, "time": 2, "curve": "exp"},
  {"type": "osc", "name": "tone", "freq": "pitch", "amp": 0.5},
  {"type": "pattern", "name": "skips", "times": [0.6, 1.3]},
  {"type": "stutter", "name": "skip", "input": "tone", "events": "skips", "slice": 0.08, "repeats": 3}
]
```

### Measured

The source is a linear ramp 0 to 1 over 1 s, so each output sample's value
says which input sample it was.

| What | Measured | Test |
|---|---|---|
| event 0.5 s, slice 0.05 s, 4 repeats: samples before the event and after repeats + fade | identical to the input bit for bit | `stutter_repeats_the_last_slice_and_leaves_the_rest_alone` |
| same: repeated samples (after each 2 ms fade-in) against the slice that played before | differ by at most 0 | same |
| same: largest sample step in the region | 101 x the ramp's own step (a hard cut would be about 2400 x); the join is a 2 ms linear blend over 0.05 to 0.2 of level | same |
| `reverse`, 3 repeats | copies equal the next slice reversed, difference 0; `causal()` false (true without `reverse`) | `stutter_reverse_plays_the_next_slice_backwards_and_is_acausal` |
| event at 0 / at 5 s (past the end) | output equals the input bit for bit | `stutter_edges_stereo_bit_identical_and_errors_explain` |
| event at 10 ms, slice 50 ms | repeats the 10 ms there is, difference 0 | same |
| stereo input with `poisson` events; two renders | 2 channels; bit-identical | same |
| slice 0.0001 / 10, repeats 256 / 1, `reverse`, 200 events a second, input amplitude 1e6 | finite | same |
| recipe above, 2 s | finite, rms -9.04 dB | `lofi_recipes_render` |

### Honest limits

- The stutter ends with a 2 ms fade back into the original, which is at its
  later level: a decaying tail jumps to where it has got to over those 2 ms.
- With overlapping events the slice is taken from the already stuttered
  output (forward) or the input (`reverse`).
- The fade length is fixed at 2 ms.
- None of this has been heard.
