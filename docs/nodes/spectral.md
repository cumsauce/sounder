# `spectral` modes shift / stretch / smear / hps, `pitch_shift.formant`

All five live in `crates/sfx_graph/src/nodes/spectral.rs` (offline family,
acausal, `causal()` false; not random). Tests: `crates/sfx_analysis/tests/it/spectral.rs`.
Every output is exactly the document's length. Stereo input runs per channel.
The existing modes (`freeze blur gate morph`) and `pitch_shift` / `time_stretch`
without `formant` are bit-identical to before: `existing_modes_are_bit_identical_to_before_x1`
pins an FNV-1a hash of nine renders (recorded on the commit before the change).

## `spectral` (new modes)

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `mode` | word | required | adds `shift` `stretch` `smear` `hps` | The four new modes. |
| `hz` | signal | required for `shift` | Hz, -sr/2 .. sr/2 (+-24000 at 48 kHz) | Moves every component by this many Hz (single sideband). A node here sweeps the shift. Mono. |
| `inharmonicity` | number | `0.01` (`stretch`) | 0 .. 2 | b in `f' = f sqrt(1 + b (f / f0)^2)`: partial n of a series at `f0` lands at `n f0 sqrt(1 + b n^2)`. |
| `f0` | number | `100` (`stretch`) | Hz, 1 .. 0.49 sr (23520 at 48 kHz) | The series' base frequency in that law. |
| `time` | number | required for `smear` | seconds, -2 .. 2 | Group delay at Nyquist; 0 at DC, linear between. Negative: highs first. |
| `kernel` | number | `17` (`hps`) | odd, 3 .. 101 | Median length in frames (for the harmonic estimate) and in bins (percussive). |
| `window` | number | 2048 | power of two 64 .. 16384 | `stretch` and `hps` only (as the other STFT modes). An error with `shift` and `smear`, which run on the whole signal. |

A field for another mode is an error (`field `hz` is only for mode Shift, not Gate: remove it`).
Errors name field and value:

- `hz: 30000: give a number between -24000 and 24000 Hz (half the sample rate)`
- `inharmonicity: 5: give a number from 0 to 2`
- `f0: 0: give a frequency from 1 to 23520 Hz (0.49 of the sample rate)`
- `time: 3: give seconds from -2 to 2`
- `kernel: 8: give an odd number from 3 to 101 (default 17)`
- `field `window` is not used by mode Shift (it runs on the whole signal): remove it`

Work caps: `shift` and `smear` take one real FFT of the next power of two at or
above 2 x the document length (plus 2 s of samples for `smear`); `hps` costs
frames x bins x `kernel` x 2 median selects per channel; the other counts have
the limits above.

### Ports

`shift`, `stretch`, `smear`: `out`. `hps`: `out` (= `harmonic`), `harmonic`,
`percussive`; reference them as `"name.harmonic"`, `"name.percussive"`. The two
parts use complementary soft masks, so `harmonic + percussive` equals the input.

### What each mode does

- `shift`: the analytic signal (FFT Hilbert) times `exp(j 2 pi integral(hz))`, real part. Components
  that would leave 0 .. Nyquist (up past it, or down past 0 Hz) are removed
  before the shift, not folded. A harmonic series becomes inharmonic (a
  200/400/600 Hz tone shifted +50 Hz is 250/450/650 Hz).
- `stretch`: per frame, spectral peaks (local maxima over +-2 bins) move to
  the warped frequency, each keeping its phase advance at the new frequency
  and its neighbouring bins locked to it (the `pitch_shift` machinery); bins
  move by whole-bin shifts and the larger magnitude wins a collision. Peaks
  warped past Nyquist are dropped.
- `smear`: an allpass with group delay rising linearly from 0 Hz to Nyquist,
  so a click becomes a downward-to-upward chirp of length `time`. Magnitudes
  are untouched. What lands past the document's end is cut: give a `duration`.
  Negative `time` that would land before 0 is lost.
- `hps`: Fitzgerald median-filter separation: median over time keeps steady
  partials, median over frequency keeps clicks, soft masks `h^2 / (h^2 + p^2)`.

### Recipe

Stiff-string inharmonicity on a struck sound, plus the click split off and
layered back dry. Give the human macros on `inharmonicity` and on the gain of
the clicks.

```json
[
  {"type": "osc", "name": "saw", "shape": "saw", "freq": 180, "amp": 0.4},
  {"type": "decay", "name": "fall", "time": 0.4},
  {"type": "math", "name": "note", "op": "mul", "a": "saw", "b": "fall"},
  {"type": "spectral", "name": "split", "input": "note", "mode": "hps"},
  {"type": "spectral", "name": "bent", "input": "split.harmonic", "mode": "stretch",
   "inharmonicity": 0.03, "f0": 180},
  {"type": "mix", "name": "both", "inputs": ["bent", "split.percussive"]}
]
```

### Measured (48 kHz)

| What | Measured | Test |
|---|---|---|
| `shift` +300 / -300 / +7.5 Hz on 1 kHz sine: peak | 1300.00 / 699.99 / 1007.50 Hz | `shift_moves_a_sine_by_hz_and_keeps_its_level` |
| same: band level (input -9.03 dB) | -9.03 dB each; 1 kHz band -120.0 dB for +-300 | same |
| `shift` +50 Hz on 200/400/600 Hz | 250.005, 450.004, 650.002 Hz | `shift_makes_a_harmonic_series_inharmonic` |
| `shift` ramp 0 -> 600 Hz over 1 s on 1 kHz | 1060.0 Hz at 0.05-0.15 s, 1540.0 Hz at 0.85-0.95 s (expected 1060, 1540) | `shift_takes_a_signal` |
| 20 kHz +6 kHz, 5 kHz -8 kHz (leave the band) | rms -58.3 dB and -60.0 dB (input -9 dB; test limit -50: the onset of the truncated sine leaks through the cut) | `shift_removes_what_leaves_the_band_instead_of_folding_it` |
| `stretch` b 0.02, f0 200, 8 harmonics of 200 Hz | 202.0 415.7 651.8 919.1 1224.7 1573.8 1970.0 2415.9 Hz, equal to the law to 0.1 Hz; total level -14.18 dB vs -13.98 dB | `stretch_moves_partials_by_the_stiff_string_law` |
| `stretch` b 0 | 200.0 ... 1600.0 Hz | `stretch_with_zero_inharmonicity_leaves_the_partials_alone` |
| `smear` 0.05 s on a click at 0.1 s | energy -6.02 dB (click -6.02); 10-90 % of the energy in 105.0-145.0 ms; peak 0.015 vs 0.5; low band leads (-40.4 / -120 dB at 0.100-0.1125 s, -120 / -39.8 dB at 0.1375-0.15 s); 1-3 kHz band within 1 dB | `smear_spreads_a_click_into_a_chirp_and_keeps_the_energy` |
| `hps` 440 Hz sine + 3 clicks: 440 Hz band | harmonic -13.5 dB, percussive -96.2 dB (gap 82.7 dB; card: 20) | `hps_puts_the_sine_in_harmonic_and_the_clicks_in_percussive` |
| same: 3-20 kHz band | percussive -45.1 dB, harmonic -120.0 dB (gap 74.9 dB; card: 20) | same |
| same: harmonic + percussive vs input | error rms -151.8 dB | same |
| all modes, one thread vs the pool | same bits | `new_modes_are_bit_identical` |
| all modes, stereo in | 2 channels, both non-silent | `new_modes_run_on_both_channels` |
| noise at amplitude 1e6 and silence; hz +-24000/0/1e-6, b 2 with f0 1, b 0 with f0 23520, window 64, time +-2/0, kernel 3 and 101 | all finite, peak at most 1.03e6 | `extremes_stay_finite` |

## `pitch_shift.formant`

`"formant": true` (default false) divides the shifted sound, frame by frame, by
its own spectral envelope and multiplies by the input's. The envelope is a
cepstral "true envelope" (the log magnitude smoothed to the lowest sr/500
quefrency samples, 96 at 48 kHz, then 10 refinements that pull the curve up to
the peaks), and the correction is limited to +-30 dB per bin. Costs about 44
extra FFTs per frame per channel.

### Recipe

```json
[
  {"type": "osc", "name": "raw", "shape": "saw", "freq": 150, "amp": 0.5},
  {"type": "filter", "name": "voice", "input": "raw", "mode": "bp", "freq": 1200, "q": 3},
  {"type": "pitch_shift", "name": "up", "input": "voice", "semitones": 7, "formant": true}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| envelope peak (parabola through the strongest harmonic and its neighbours): input / +7 st plain / +7 st with `formant` | 1169 / 1751 (+49.8 %) / 1189 Hz (+1.7 %; card: within 5 %) | `pitch_shift_formant_keeps_the_envelope_peak` |
| pitch of the formant output | 224.7 Hz (wanted 224.7) | same |
| level, formant vs plain | -15.0 vs -15.4 dB | same |
| `formant` absent vs `false` | same hash | `formant_false_is_the_default` |

## Honest limits

- None of this has been heard. All numbers are measurements of test signals.
- The first formant measurement (peak of a spectrum smoothed over +-450 Hz)
  read +7.0 %, but it snapped to the shifted harmonic grid (225 Hz spacing, a
  harmonic at 1123 Hz); the test now measures between harmonics with a
  parabola. The first envelope estimator (plain cepstral smoothing, no
  refinement) also over-boosted the level by 8.7 dB; the refinement fixed it.
  The tolerance was not changed.
- `shift` is a single-sideband shift of the whole signal; a signal that starts
  abruptly leaks a little (see the -58 dB row). `hz` as a signal is band-limited
  by its extreme values, so a sweep to a large shift removes content for the
  whole sound.
- `stretch` places bins by whole-bin shifts and warps only peaks, so noise and
  dense material get bin-sized smearing; the level of 8 clean partials stayed
  within 0.2 dB, other material was not measured. B limited to 0..2 (negative
  values would make the warp non-monotonic).
- `smear` is one fixed law (group delay linear in frequency); there is no
  other shape. The tail past the document is cut.
- `hps` with the default kernel (17 frames = 181 ms at the default window)
  treats a sustained noise or a very short tone burst as a mixture; both
  outputs are masks of one STFT, so a transient and a partial that share a
  bin and frame are split by the masks, not separated exactly.
- `formant` assumes the input's envelope has features wider than about 500 Hz;
  a sound with a pitch above about 400 Hz (quefrency under 120 samples) is
  likely to have its harmonics partly followed by the envelope. Not measured.
