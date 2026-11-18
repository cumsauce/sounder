# Motion and stereo

Three nodes in `crates/sfx_graph/src/nodes/motion.rs`; tests in
`crates/sfx_analysis/tests/motion.rs`. All take a mono `input` (`doppler` also
takes stereo when `pan` is false) and are causal. Only `decorrelate` is random.

## `doppler`

A source passing the listener on a straight line (placement family, effect).
The sound heard at time t was emitted at te with te + r(te) / 343 = t, where
r is the distance at emission; the node solves that per sample (Newton, in
f64) and reads the input at te through a third-order Lagrange delay, so the
pitch bends by exactly the radial speed: f / (1 - v_r / 343) approaching.
The level is 1 / max(r, 1), an optional 12 dB per octave low-pass follows
foley's `absorption_cutoff` (the same as `air`: 50 % humidity, 15 C, none
under about 10 m), and an equal-power pan follows the bearing x / r. The
source moves from the left to the right.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono audio | The emitted sound. Stereo is refused when `pan` is on. |
| `speed` | number | `30` | 0 .. 300 m/s | Source speed. Not with `distance`. |
| `closest` | number | `5` | > 0 .. 5000 m | Distance of the path from the listener at its nearest. Level is flat at its 1 m value below 1 m. Not with `distance`. |
| `at` | number | half the sound's length | seconds, 0 or more | When the sound emitted at closest approach reaches the listener: the loudest instant, where the pan crosses the centre. Not with `distance`. |
| `distance` | signal | none | 0 .. 10290 m | Drives the effect directly: delay = distance / 343 as heard at that moment, level 1 / max(d, 1), mono out. A distance falling at u m/s shifts the pitch by 1 + u / 343. A number outside the range is an error; a signal is clamped. |
| `pan` | bool | `true` (with `speed`); false with `distance` | | Stereo out from the bearing. `true` with `distance` is an error (no bearing). |
| `air` | bool | `true` | | The absorption low-pass. |

Caps and errors: `speed` and `closest` are bounded as above; the delay line
is bounded to 30 s (`at` large with a high `speed` starts the source farther
away than that, and errors naming `at`):

- `speed: 400: give a number from 0 to 300 m/s (sound travels at 343)`
- `closest: 0: give a distance above 0 and at most 5000 metres`
- `at: 60: with speed 300 m/s the source starts 143547 m away (418.5 s of travel, the limit is 30 s): give a smaller at or speed`
- `pan: true needs a bearing, and distance has none: remove pan or give speed and closest instead of distance`

### Recipe

A car-like pass: filtered brown noise, 25 m/s, 4 m from the listener, closest
at 2 s. Give the human macros on `speed` and `closest`.

```json
[
  {"type": "noise", "name": "engine", "color": "brown", "amp": 0.5},
  {"type": "filter", "name": "growl", "input": "engine", "mode": "bp", "freq": 300, "q": 2},
  {"type": "doppler", "name": "pass", "input": "growl", "speed": 25, "closest": 4, "at": 2.0}
]
```

## `haas`

A mono sound made stereo: one channel is the input bit for bit, the other
the input delayed (precedence effect). `delay` 0 gives two equal channels.

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono audio | The sound. |
| `delay` | signal | `0.012` | 0 .. 0.05 s | Delay of the late channel (Lagrange, exact at whole samples). A number outside the range errors; a signal is clamped. Above ~30 ms it is an echo. |
| `side` | word | `right` | `left` `right` | The channel that gets the delayed copy. |

- `delay: 0.2: give a number from 0 to 0.05 seconds`

## `decorrelate`

A mono sound made two channels with the same spectrum and no zero-lag
correlation: each channel is the input through its own all-pass FIR (2048
taps at 48 kHz), built as the inverse FFT of a unit-magnitude spectrum whose
group delay is a seeded power of frequency, rising in the left channel and
falling in the right, over a fixed 3.4 ms lead. The convolution is one FFT
over the whole sound.

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono audio | The sound. |
| `amount` | number | `1` | 0 .. 1 | Scales the group delay (at 1: 3.4 ms plus 7 to 11 ms that rises with frequency on the left and falls on the right). 0 gives two bit-identical copies of the input. |
| `seed` | integer | `0` | any | Another seed, another pair of filters. Mixed with the document seed and the node name. |

- `amount: 1.5: give a number from 0 to 1`

Output is delayed by the filters' group delay (3.4 to about 14 ms), so a
transient moves a few milliseconds later and its energy spreads over that
span.

### Measured

48 kHz. Tests in `crates/sfx_analysis/tests/motion.rs`.

| What | Measured | Test |
|---|---|---|
| 1 kHz at 34.3 m/s, closest 1 m: pitch 0.1 to 0.4 s (before) and 1.6 to 1.9 s (after) | 1111.07 Hz (1111.1 expected), 909.20 Hz (909.1) | `doppler_bends_the_pitch_by_the_radial_speed` |
| level curve at closest 1 / 5 / 0.2 m (constant input): peak value, time from `at` | 1.0000 at 0.00 ms; 0.2000 at 0.08 ms; 1.0000 (flat inside 1 m) | `doppler_level_peaks_at_the_closest_approach` |
| 1 kHz sine, loudest 10 ms RMS window | centred 1.0 ms from `at` | same |
| pan crossing the centre | 0.02 ms from `at`; start L 0.044 / R 0.000, end L 0.000 / R 0.044 | `doppler_pan_crosses_the_centre_at_the_closest_approach` |
| air on a 200 m pass (noise, 8 to 16 kHz around `at`) | 21.6 dB lower with air; 200 to 800 Hz unchanged (-77.8 dB both) | `doppler_air_takes_the_highs_off_a_far_pass` |
| `distance` 343 m: impulse at 0.1 s | arrives at sample 52800 (expected 52800), height 0.00292 (0.00292) | `doppler_follows_a_distance_signal` |
| `distance` ramp 100 to 0 m in 1 s on 1 kHz | 1291.5 Hz (1291.5) | same |
| speed 0..300 x closest 1e-6..5000 x at 0..0.5 x pan, input 1e6, and a +-1e9 m sweep | all finite, peak at most 1.3e6 | `doppler_is_finite_at_the_extremes` |
| `haas` 12 ms right / 25 ms left | late channel 576 / 1200 samples after the early one; early channel is the input bit for bit | `haas_delays_one_side_by_the_stated_time` |
| `haas` 12.3456 ms on a 1 kHz sine | both channels -9.031 dB rms | same |
| `decorrelate` on white noise, 4 s: L/R correlation | 0.035 | `decorrelate_splits_noise_into_uncorrelated_flat_channels` |
| same: octave bands 62.5 Hz to 16 kHz, each channel against the input | within 0.01 dB | same |
| same: (L + R) / sqrt 2 against the input per octave band | within 0.77 dB (largest, at 62.5 to 125 Hz) | same |
| kernels as impulse responses | energy 1.0000 each; per-bin magnitude 100 Hz to 16 kHz -0.02 to +0.03 dB, third-octave mean power -0.00 to +0.01 dB | `decorrelate_kernels_are_flat_all_pass_filters` |
| brown noise L/R correlation by `amount` 0.02 / 0.1 / 0.25 / 0.5 / 1 | 0.854 / 0.439 / 0.131 / 0.007 / -0.003 | `decorrelate_amount_zero_is_two_copies_and_one_pulls_apart_smoothly` |
| all three, two renders, one thread vs the pool | same bits | `*_is_bit_identical*` |
| recipe above (first / last 0.5 s of 4 s) | L minus R rms +49.9 dB then -48.2 dB (hard left then hard right) | `motion_recipe_renders` |

### Honest limits

- The geometry is a straight line at constant speed past a point listener in
  still air at 343 m/s, fixed (the `celsius` of `air` is not a field here).
  No reflections, ground effect, wind or source directivity.
- `pan` is the bearing alone: the same equal-power law as `pan`, with no
  head shadow or inter-aural delay, so L and R are never delayed against each
  other.
- The delay line reads through a third-order Lagrange interpolator: highs
  lose a little at fractional delays, and at 300 m/s the up-shifted content
  folds back above Nyquist. Not measured.
- `distance` is read as the distance heard at each moment; a physical source
  speed v is the same as a distance falling at v / (1 - v / 343) m/s.
- The first sound a `doppler` plays arrives after the delay of the starting
  distance, so the start is silent for that long (the line starts empty).
- `decorrelate` does not preserve a mono sum: for uncorrelated channels
  (L + R) / 2 reads 3 dB under the input; only (L + R) / sqrt 2 matches it. It
  smears transients over about 14 ms; do not use it on a click you want sharp.
  Its filters are not minimum phase and add a delay.
- None of this has been heard.
