# Chorus, flanger, phaser, freq_shift

Four modulation effects in `crates/sfx_graph/src/nodes/modfx.rs`, tests in
`crates/sfx_analysis/tests/it/modfx.rs`. All four are sample by sample with no
lookahead (causal) and read no seed (not random). Delay reads use
`DelayLine::read_lagrange3`; `freq_shift` uses `dsp::Hilbert`.

A signal field takes a number, `"$param"`, `"=expr"` or the name of another
node. For every signal field below, a number outside the range is an error and
a signal is clamped to the range per sample (a non-finite sample counts as the
field's neutral value). Mix everywhere is `dry * (1 - mix) + wet * mix`, so
`mix` 0 is the input (the tests compare bits).

## `chorus`

Several copies of the input, each read from a delay line swung by a sine LFO
(the copies are evenly spread over one LFO cycle), mixed with the dry input
(modulation family, effect).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono; stereo only when `spread` is 0 | Stereo with `spread` above 0 is an error naming the fix. With `spread` 0 each channel runs on its own. |
| `rate` | signal | `0.8` | Hz, 0 .. 20 | LFO rate. |
| `depth` | signal | `0.003` | seconds, 0 .. `delay` | Peak swing of the delay either side of `delay`. |
| `delay` | number | `0.02` | seconds, 0.001 .. 0.1 | Centre delay. |
| `voices` | number | `3` | whole number, 1 .. 8 | Taps on the one delay line. |
| `spread` | number | `1` | 0 .. 1 | Voices panned evenly from `-spread` to `+spread`, equal power. 0: mono result. |
| `mix` | signal | `0.5` | 0 .. 1 | Wet amount. |

Caps and errors: one delay line of at most 2 x 0.1 s per channel (voices are
taps on it, so cost grows with `voices` only by the reads).

- `voices: 9: give a whole number from 1 to 8`
- `delay: 0.5: give a number from 0.001 to 0.1 seconds`
- `depth: 0.05: give a number from 0 to 0.02 seconds (up to delay)`
- `input: \`st\` is stereo but spread 1 makes a stereo result from a mono input: add a mix node with "to": "mono" in between, or set spread to 0`

Each voice has gain 1 / sqrt(voices); in stereo it is multiplied by sqrt(2)
times the equal-power pan gains, so one channel carries about the level of
the mono result. One voice has nothing to spread: both channels are equal.

## `flanger`

The input plus a copy delayed by a few milliseconds, the delay swung by a sine
LFO or driven by `manual`, with feedback (modulation family, effect).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo | Stereo: each channel on its own with the same LFO. |
| `rate` | signal | `0.25` | Hz, 0 .. 20 | LFO rate. |
| `depth` | signal | `0.002` | seconds, 0 .. `delay` | Peak swing either side of `delay`. Ignored with `manual`. |
| `delay` | number | `0.003` | seconds, 0.0001 .. 0.02 | Centre delay. Ignored with `manual`. |
| `feedback` | signal | `0` | -0.95 .. 0.95 | Delayed copy back into the line. Negative moves the notches to multiples of 1 / delay. |
| `mix` | signal | `0.5` | 0 .. 1 | Wet amount. |
| `manual` | signal | none | seconds, 2 samples .. 0.02 | Replaces the LFO: the delay itself. A number is a fixed comb filter. |

Caps and errors: one line of at most 0.02 s per channel.

- `feedback: 1.5: give a number from -0.95 to 0.95`
- `manual: 0.5: give a number from 0.000041666666 to 0.02 seconds`

The delay is clamped to at least 2 samples (the interpolator needs it).

## `phaser`

The input plus a copy through a chain of first-order allpass filters whose
corner frequency is swung by a sine LFO; `stages / 2` notches (modulation
family, effect). All stages share one corner frequency.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo | Stereo: each channel on its own with the same LFO. |
| `rate` | signal | `0.5` | Hz, 0 .. 20 | LFO rate. |
| `stages` | number | `4` | `4` `6` `8` `12` | Allpass stages. |
| `center` | number | `800` | Hz, 20 .. 0.45 x sample rate | Centre of the allpass corner sweep. |
| `depth` | signal | `1` | octaves, 0 .. 4 | Sweep either side of `center`; 0 holds still. |
| `feedback` | signal | `0` | -0.95 .. 0.95 | Chain output back to its input (one sample late). |
| `mix` | signal | `0.5` | 0 .. 1 | Wet amount. |

Caps and errors: at most 12 one-pole states per channel.

- `stages: 5: give 4, 6, 8 or 12`
- `center: 5: give a number from 20 to 21600 Hz`
- `depth: 9: give a number from 0 to 4 octaves`

The corner of the allpass is `center * 2^(depth * lfo)`, clamped to 20 Hz ..
0.45 x sample rate. The lowest notch sits below the corner.

## `freq_shift`

Moves every partial by `shift` Hz (single sideband modulation of the
analytic signal), so partial spacing is kept and harmonic sounds turn
inharmonic (modulation family, effect).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo | Stereo: each channel on its own with the same shift. |
| `shift` | signal | required | Hz, -sr/2 .. sr/2 (+-24000 at 48 kHz) | Positive up, negative down. Must be mono. |
| `mix` | signal | `1` | 0 .. 1 | Wet amount. |

- `shift: 30000: give a number between -24000 and 24000 Hz (half the sample rate)`

Partials shifted below 0 Hz mirror, and shifted past Nyquist alias.

### Recipes

A swirling pink-noise wash, then a four-voice stereo chorus on its mono sum.
Give the human macros on the phaser `rate` and `depth`, and on the chorus
`mix`.

```json
[
  {"type": "noise", "name": "wash", "color": "pink", "seed": 3, "amp": 0.4},
  {"type": "phaser", "name": "swirl", "input": "wash", "rate": 0.3, "stages": 8, "center": 600, "depth": 1.5, "feedback": 0.5, "mix": 0.6},
  {"type": "chorus", "name": "wide", "input": "swirl", "spread": 0, "rate": 0.5, "mix": 0.4},
  {"type": "mix", "name": "summed", "inputs": ["wide"], "to": "mono"},
  {"type": "chorus", "name": "wide2", "input": "summed", "spread": 1, "voices": 4, "mix": 0.5}
]
```

A metallic inharmonic shimmer: a `freq_shift` of a harmonic tone by +37 Hz
(`{"type": "freq_shift", "name": "shim", "input": "tone", "shift": 37, "mix": 0.5}`)
keeps the partial spacing; summed with the dry tone each partial is paired
with one 37 Hz above it (not measured). A flanger sweep by hand: `ramp` from 0.0005 to 0.004 into
`manual`.

### Measured

Tests in `crates/sfx_analysis/tests/it/modfx.rs`, 48 kHz.

| What | Measured | Test |
|---|---|---|
| flanger `manual` 0.001, mix 0.5, feedback 0, white noise: 24 notches 500 .. 23500 Hz | shallowest 34.1 dB under the median level (limit 15), largest position error 2.0 Hz | `flanger_manual_1ms_notches_every_khz_from_500` |
| flanger lfo 0.1 Hz, 3 +- 2 ms, impulse echo | 144 samples (3.000 ms) at t = 0, height 0.880; 240 samples (5.000 ms) at t = 2.5 s, height 1.000 | `flanger_lfo_swings_the_delay_and_feedback_repeats_it` |
| flanger fixed 1 ms, feedback 0.5, mix 1: echoes at 48, 96, 144 samples | 1.0000, 0.5000, 0.2500 | same |
| phaser 4 stages, depth 0, impulse response | 2 notches at 329 and 1893 Hz (allpass maths 332, 1923) | `phaser_n_stages_make_n_over_2_notches_where_the_allpass_says` |
| phaser 6, 8, 12 stages | 3, 4, 6 notches; 6 stages at 214, 796, 2908 Hz (maths 215, 800, 2951); 12 stages at 105, 332, 614, 1040, 1915, 5710 Hz (maths 105, 332, 614, 1042, 1923, 5788) | same |
| phaser lfo 0.5 Hz, +-1.5 oct about 800 Hz, 100 ms of white noise at each extreme | lowest notch 961 Hz at the crest, 164 Hz at the trough | `phaser_lfo_sweeps_the_notches` |
| chorus 1 voice, 440 Hz sine, depth 3 ms, rate 0.8 Hz | 98 % of the power within +-7.24 Hz of 440; f x depth x 2 pi x rate = 6.64 Hz (ratio 1.09, limit 0.7 .. 1.3) | `chorus_spreads_a_sine_by_depth_rate_and_frequency` |
| chorus 3 voices, spread 1, mix 0.5, 440 Hz sine | L / R correlation 0.732 (limit 0.9), rms L -11.50 dB, R -11.80 dB; spread 0 gives 1 channel | `chorus_stereo_spread_decorrelates` |
| chorus, flanger, phaser, freq_shift with a stereo input | stereo out (chorus with spread 0); chorus `mix` 0 bit-identical to the input | `chorus_mix_zero_is_the_input_and_stereo_input_runs_per_channel` |
| freq_shift +100 on a 1 kHz sine | peak 1100.00 Hz; 900 Hz image 48.5 dB under the peak (limit 40); 1 kHz line -120 dB (floor) | `freq_shift_moves_a_sine_and_rejects_the_image` |
| freq_shift -100 on a 1 kHz sine | peak 900.00 Hz; 1100 Hz image 48.5 dB under the peak | same |
| freq_shift 0, white noise, 8 octave bands 100 Hz .. 20 kHz; 1 kHz sine | at most 0.001 dB from the input; 0.000 dB (limit 0.1) | `freq_shift_zero_keeps_the_spectrum` |
| freq_shift ramp 0 -> 2000 Hz over 1 s on a 1 kHz sine | peak 1250 Hz at 0.05 .. 0.2 s, 2850 Hz at 0.9 .. 0.95 s | `freq_shift_takes_a_signal_and_mixes` |
| freq_shift +100, mix 0.5 | dry line and shifted line both -15.05 dB (expected -15.05) | same |
| all four, field limits at input amplitude 1e6; every signal field fed a +-1e9 ramp | all finite, peak under 1e9 | `modfx_are_finite_at_the_extremes` |
| all four, one thread vs the pool | same bits | `modfx_are_bit_identical` |
| recipe above, 2 s | 2 channels, 96000 samples, rms L -21.78 dB, R -21.75 dB | `modfx_recipe_renders` |

### Honest limits

- `freq_shift` uses `dsp::Hilbert`, fitted for 48 kHz: the 90 degree band
  (20 Hz .. 20 kHz within 0.6 degrees) scales with the sample rate. Below
  about 20 Hz and above about 20 kHz the image is not rejected as well; the
  measured 48.5 dB was at 1 kHz only.
- `freq_shift` with `mix` between 0 and 1 blends the dry input with a path
  that went through the allpass chains (different delay per frequency); the
  dry path is not delay-matched. Not measured beyond the 1 kHz mix 0.5 line
  levels above. `mix` 1 with `shift` 0 changes only phase (the magnitude
  test above).
- The chorus bandwidth was measured with a single sine and one voice only;
  several voices were measured for L / R correlation and level, not for
  bandwidth.
- A chorus voice reads the line before the current sample is written, so
  the shortest delay is 2 samples; `delay` 0.001 with `depth` equal to it
  clamps the swing at that floor.
- With the LFO running, the 3 ms echo above reads 0.880 instead of 1.0 (the
  read position is moving while the impulse passes; the cause was not
  isolated further). At a fixed delay the echoes are exact (table).
- The phaser shares one coefficient across stages, so notches follow the
  allpass maths above and not the staggered corners of an analogue
  phaser. The measured notch positions at high frequency are up to 2 %
  below the maths (the notch finder smooths 7 bins; the cause was not
  isolated).
- A time-varying allpass and a feedback delay are not guaranteed lossless:
  stability was checked at the extremes in the table, not proved.
- Extreme values stay finite (table above). None of this has been heard.
