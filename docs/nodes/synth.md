# `fm`, `wavetable`

## `fm`

Two-operator FM (phase modulation): one modulator, optionally with
self-feedback, modulates one sine carrier. Family: source.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `freq` | signal | required | Hz | base frequency; the carrier runs at `freq * ratio`, the modulator at `freq * mod_ratio` |
| `ratio` | number | 1 | any finite | carrier multiple (negative runs backwards) |
| `index` | signal | 0 | radians | peak phase deviation of the carrier; sideband k has amplitude J_k(index) |
| `feedback` | number | 0 | >= 0 (0..2 useful) | modulator self-feedback, radians per unit of the average of its last two outputs |
| `mod_ratio` | number | 1 | any finite | modulator multiple |
| `mod_shape` | word | `sine` | `sine` `saw` `square` `tri` | modulator wave (saw / square are polyBLEP band-limited) |
| `amp` | signal | 1 | linear | output gain |

Negative `feedback` errors: `feedback: -1: give a number >= 0`. Phase
accumulators run in f64; per-sample step is clamped to +-0.5 cycle.

### Recipe

```json
[
  {"type": "ramp", "name": "idx", "from": 6, "to": 0.5, "time": 0.4, "curve": "linear"},
  {"type": "decay", "name": "env", "time": 0.3},
  {"type": "fm", "name": "bell", "freq": 440, "mod_ratio": 1.4, "index": "idx", "feedback": 0.2},
  {"type": "math", "name": "hit", "op": "mul", "a": "bell", "b": "env"}
]
```

An index that falls over the note gives a spectrum that narrows with time;
give the human macros on `index` and `mod_ratio`.

### Measured

| What | Measured | Test |
|---|---|---|
| index 0, 1 kHz: THD+N | -291 dB (floor of the f32 render) | `fm_index_zero_is_a_pure_sine` |
| fc 2000, fm 200, index 2: sidebands 0, +-1, +-2, +-3 vs J_k(2) | -16.01, -7.79, -12.06, -20.80 dB; all equal to Bessel to 0.01 dB | `fm_sidebands_match_bessel` |
| 1:1, index 2: harmonics 1..4 vs folded sum J_{n-1} + (-1)^n J_{n+1} | -20.80, -6.04, -12.94, -20.34 dB; all within 0.01 dB | `fm_one_to_one_harmonics_follow_the_folded_bessel_sum` |
| 1000 / 1400, index 2: strongest 8 peaks | 400, 2400, 3800, 1800, 1000, 3200, 5200, 6600 Hz, all |fc + k fm| within 0.01 Hz | `fm_inharmonic_ratio_puts_peaks_at_carrier_plus_minus_k_modulator` |
| modulator alone (ratio 0, index 0.01), feedback 1.5 | H2 -7.3 dB, H3 -11.8 dB relative to the fundamental (feedback 0: under -70 dB) | `fm_feedback_turns_the_modulator_into_a_saw_like_wave` |

### Honest limits

At ratio 1:1 the sidebands land on harmonics and the negative ones fold back
(the Bessel check uses the folded sum, and a clean J_k check needs a ratio
like 10:1). No oversampling: a large index at a high `freq` puts sidebands
past Nyquist and they alias (not measured). Finite at freq 0 / 1e7 / negative,
index +-1000, feedback 100 (bounded: output is a sine). Not heard.

## `wavetable`

Additive single-cycle oscillator from harmonic amplitudes, morphing to a
second set. Family: source.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `freq` | signal | required | Hz | fundamental |
| `harmonics` | list | required | 1..256 amplitudes | amplitudes of harmonics 1, 2, 3, ... (not normalized) |
| `harmonics_to` | list | none | up to 256 | second set; shorter list padded with zeros |
| `morph` | signal | 0 | 0..1 (clamped) | blend from `harmonics` to `harmonics_to`; error without `harmonics_to` |
| `phases` | list | all 0 | cycles | per-harmonic phase; at most as long as the longer set |
| `amp` | signal | 1 | linear | output gain |

Errors: `harmonics: 257 entries: give at most 256`, empty list, `morph:
needs harmonics_to`, `phases` longer than the sets.

Implementation (the card left it open): direct additive, no tables. Per
sample the harmonics are summed with a rotating phasor (one sin/cos of the
phase, then complex multiplies; f64), so `freq` and `morph` can be audio-rate
signals exactly. Harmonic k at frequency k|freq| has weight 1 up to 0.45
sample rate, fades linearly to 0 at Nyquist, and is dropped above; this is
decided per sample, so a sweep through the top never clicks. Cost is
proportional to harmonics kept per sample (at most 256).

### Recipe

```json
[
  {"type": "ramp", "name": "m", "from": 0, "to": 1, "time": 1.0, "curve": "linear"},
  {"type": "wavetable", "name": "pad", "freq": 110, "harmonics": [1],
   "harmonics_to": [1, 0.5, 0.33, 0.25, 0.2, 0.17, 0.14, 0.12], "morph": "m", "amp": 0.4}
]
```

Sine opening into a saw-like spectrum; give the human the `morph` end value
as a brightness macro.

### Measured

| What | Measured | Test |
|---|---|---|
| `[1,0,0.5]` at 1 kHz: f0 vs 3 f0 | -3.01 and -9.03 dB (6.02 apart); loudest other bin -120 dB | `wavetable_one_zero_half_has_two_peaks_six_db_apart` |
| 5 kHz, 32 harmonics (1/k): alias_db | -63.6 dB | `wavetable_drops_harmonics_above_nyquist_without_aliasing` |
| 11 kHz, `[1,1,1]`: 22 kHz harmonic | -4.59 dB, equal to the fade weight 1 - 400/2400 | `wavetable_fades_harmonics_near_nyquist` |
| 2nd harmonic swept 20 to 25 kHz | largest change of the 10 ms peak 0.025; ends at 0 | `wavetable_drops_harmonics_above_nyquist_without_aliasing` |
| morph 0, .25, .5, .75, 1 (sine to 16-harmonic 1/k) centroid | 200.0, 221.7, 278.4, 352.1, 426.8 Hz | `wavetable_morph_moves_the_centroid_monotonically` |
| morph ramp, centroid per quarter | 206, 247, 315, 390 Hz | same |
| phase 0.25 on harmonic 1 | first sample 1 (a cosine) | `wavetable_phases_shift_each_harmonic` |

### Honest limits

Harmonics between 0.45 sample rate and Nyquist are attenuated (21.6 to 24 kHz
at 48 kHz). Output can exceed 1 when amplitudes sum past 1. Morph is a linear
blend of the two spectra, not of waveforms' timbre as heard. Finite at freq
0, 1, 20, 24000, 1e7, negative, with 256 harmonics. Not heard.
