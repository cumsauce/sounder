# Filter slope, ladder, eq

Tests: `crates/sfx_analysis/tests/it/filters.rs`. Code: `crates/sfx_graph/src/nodes/filters.rs`
(the `Filter` arm of `crates/sfx_graph/src/feedback.rs` rejects a slope in a loop).

## `filter` (new field `slope`)

The state-variable filter gains steeper lp and hp: 24 and 48 dB an octave are
two and four cascaded 12 dB trapezoidal sections (the same `Svf` as the 12 dB
case, so `freq` and `q` stay safe to modulate at audio rate).

### Fields (added)

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `slope` | number | 12 | 12, 24 or 48 dB an octave | Steepness. Anything else errors `slope: 18: give 12, 24 or 48`. |

Only `lp` and `hp` take a slope other than 12 (other modes error and say
so). The sections use the Butterworth Q spread (24 dB: 0.541 and 1.307; 48 dB:
0.510, 0.601, 0.900, 2.563). `q` rescales the last (highest-Q) section by
`q / 0.7071`, so the default `q` is the maximally flat Butterworth and `q = 4`
adds a resonance peak. At slope 12 the node is the old filter, bit for bit.
Inside a feedback loop a slope other than 12 errors (`slope: 24: a loop runs
one 12 dB section ...`).

### Recipe

```json
[
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "filter", "name": "lp", "input": "n", "mode": "lp", "freq": 1200, "slope": 48}
]
```

A brick-wall-like low-pass for taming a noise source; give the human a macro
mapped to `lp.freq` (exp curve) named `brightness`.

### Measured

| What | Measured | Test |
|---|---|---|
| lp 1 kHz, white noise, level at 2 kHz, slope 12 / 24 / 48 | -12.37 / -24.22 / -48.35 dB (Butterworth: -12.3 / -24.1 / -48.2) | `slope_sets_the_level_an_octave_above_the_cutoff` |
| passband 200-400 Hz, same | -0.04 / 0.00 / 0.00 dB | same |
| hp 1 kHz, level at 500 Hz, slope 24 / 48 | -23.95 / -47.66 dB | `slope_on_a_highpass_and_the_q_of_the_last_section` |
| 24 dB lp at the cutoff, q 0.707 / q 4 | -3.01 / +12.04 dB | same |
| slope 48 at freq 1 Hz and 23 kHz, q 0.025 and 1000 (lp, hp) | finite; peaks 0.00 to 3.83 for a unit-amplitude noise | `filter_slope_is_deterministic_and_finite_at_the_extremes` |

### Honest limits

Steep slopes ring longer near the cutoff with a high `q`. Slopes above 12 are
not available inside a feedback loop. Single `f32` state per section: a 48 dB
filter at a very low cutoff (a few Hz) has a noise floor set by `f32`
rounding; it stayed finite in the extreme test but was not measured for
accuracy there. None of it has been heard.

## `ladder`

A four-pole nonlinear low/band/high-pass filter in the transistor-ladder
topology (filter family).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | Stereo runs each channel alone. |
| `freq` | signal | required | Hz, clamped to 1 .. 0.49 x sample rate | Cutoff. |
| `resonance` | signal | 0 | 0 .. 1.2 (clamped) | Feedback. About 1 self-oscillates at `freq`. |
| `drive` | number | 1 | 0.01 .. 100 (errors outside) | Gain into the first stage. |
| `mode` | word | `lp` | `lp` `bp` `hp` | `lp` and `hp`: 24 dB an octave; `bp`: 12 dB an octave each side. |

The four stages are trapezoidal (zero-delay) one-poles with a `tanh` in front
of each; the feedback loop is solved from the linear chain, so the cutoff is
exact at small signal (the prewarped bilinear map) and the loop needs no unit
delay. `hp` and `bp` are mixed from the stage outputs (`1,-4,6,-4,1` and
`4 x (y2 - 2 y3 + y4)`). The passband gain falls as `1 / (1 + 4 x resonance)`,
as in the analog circuit; there is no make-up gain. The output is not
compensated for `drive`.

### Recipe

```json
[
  {"type": "noise", "name": "n", "color": "white", "amp": 0.3},
  {"type": "ramp", "name": "sweep", "from": 6000, "to": 300, "time": 0.6, "curve": "exp"},
  {"type": "ladder", "name": "acid", "input": "n", "freq": "sweep", "resonance": 0.85, "drive": 2}
]
```

A falling resonant filter sweep. Give the human macros for `resonance` and
for the sweep's end value.

### Measured

| What | Measured | Test |
|---|---|---|
| impulse into resonance 1.1, ring frequency at freq 500 / 1000 / 2000 / 4000 Hz | 500.1 / 1000.4 / 2001.3 / 4005.6 Hz (+0.03 .. +0.14 %) | `ladder_self_oscillates_at_freq_after_an_impulse` |
| same, level of the last 0.1 s of 0.5 s | -22.4 dB rms (still ringing) | same |
| resonance 0.8, impulse: first 0.1 s / last 0.1 s | -55.3 / -120 dB (rings down) | `ladder_below_resonance_one_rings_down` |
| lp 1 kHz, res 0: 100 Hz / 2 kHz / 4 kHz | -0.26 / -28.11 / -49.95 dB | `ladder_rolls_off_and_resonance_costs_passband_gain` |
| res 0.5 at 100 Hz (1/(1+2) = -9.54 dB) | -9.45 dB | same |
| res 0.9 at the cutoff | +1.21 dB | same |
| hp 1 kHz: 100 Hz / 8 kHz | -51.8 / -0.24 dB | `ladder_modes_pass_the_right_band` |
| bp 1 kHz: 100 Hz / 1 kHz / 8 kHz | -28.2 / -0.04 / -26.0 dB | same |
| freq 20 Hz, 20 kHz, 30 kHz (clamped), res 0 / 1 / 1.2 / 5, drive 20, all modes | finite; peaks 0.04 to 2.44 for a 1.0-amplitude noise | `ladder_is_deterministic_and_finite_at_the_extremes` |

### Honest limits

The `tanh` stages are not oversampled: with a high `drive` or a loud
self-oscillation, harmonics above Nyquist fold back (no alias level was
measured). Self-oscillation needs an excitation (any input, even one
impulse); a silent input from rest stays silent. The oscillation amplitude is
set by the `tanh`, not by a parameter. Within-loop use (a feedback loop in
the graph) is not available: `ladder` has no per-sample loop form. None of it
has been heard.

## `eq`

A chain of up to 16 fixed filter bands (a parametric equaliser), RBJ biquads
in `f64`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | |
| `bands` | list of bands | required | at most 16 (errors above) | Applied in order. `[]` is the input, bit for bit. |

A band is `{mode, freq, [gain_db], [q]}`:

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `mode` | word | required | `lp` `hp` `bp` `notch` `peak` `lowshelf` `highshelf` | As `filter`. |
| `freq` | number | required | 1 .. 0.49 x sample rate Hz | Cutoff or centre. |
| `gain_db` | number | 0 | -60 .. 60 dB | peak and shelves only. |
| `q` | number | 0.707 | 0.025 .. 1000 | Higher is a narrower peak, notch or bp; the shelf Q for shelves. |

Band values are plain numbers (a `$param` or `=expr` is resolved first), not
signals: for a swept band use `filter`. Out-of-range values error with the
band index (`bands[1].freq: 30000: give 1 to 23520 Hz ...`).

### Recipe

```json
[
  {"type": "noise", "name": "n", "color": "pink"},
  {"type": "eq", "name": "voice", "input": "n", "bands": [
    {"mode": "hp", "freq": 80},
    {"mode": "peak", "freq": 1000, "gain_db": 6, "q": 2},
    {"mode": "highshelf", "freq": 6000, "gain_db": -4}
  ]}
]
```

A band-shaped noise. Band fields are not signals and a macro `map` cannot
address a field inside a band, so for a knob use `filter` nodes (their `freq`
and `gain_db` can be mapped) or a `mix` of two `eq` nodes.

### Measured

| What | Measured | Test |
|---|---|---|
| peak +6 dB at 1 kHz, q 4: at 1 kHz / 250 Hz / 4 kHz | +6.000 / +0.029 / +0.027 dB | `eq_peak_measures_its_gain_and_leaves_far_frequencies_alone` |
| same peak at the default q 0.707: 250 Hz / 4 kHz | +0.78 / +0.75 dB (a wide bell) | same |
| lowshelf +5 dB at 200 Hz, 40 Hz | +4.99 dB | `eq_bands_chain_and_shelves_and_cuts_work` |
| highshelf -8 dB at 5 kHz, 16 kHz | -7.99 dB | same |
| notch 1 kHz q 5, at 1 kHz | -97 dB | same |
| empty `bands` | identical bits, mono and stereo | `eq_with_no_bands_is_the_input_bit_for_bit` |
| every mode at freq 1 Hz / 23 kHz, gain +-60, q 0.025 / 1000 | finite | `eq_is_deterministic_and_finite_at_the_extremes` |

### Honest limits

At the default `q` a peak is wide, so "0 dB two octaves away" holds for `q`
of about 4 or more (measured above), not for 0.707. The biquad coefficients
are fixed per node, so there is no audio-rate sweep and no macro mapping of a
band's field. High frequencies near Nyquist are warped by the bilinear
transform as in any RBJ filter (not measured). None of it has been heard.
