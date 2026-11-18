# Ringmod

## `ringmod`

A ring modulator (nonlinear family, effect): multiplies the input by an
internal carrier oscillator, so every input partial at `f` becomes two
partials at `f - freq` and `f + freq` and the input frequency itself is
removed. It is also the reference node: `crates/sfx_graph/src/nodes/ringmod.rs`
and `crates/sfx_analysis/tests/ringmod.rs` are the files to copy.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | The sound to modulate. Stereo: both channels get the same carrier. |
| `freq` | signal | required | Hz, -sr/2 .. sr/2 (+-24000 at 48 kHz) | Carrier frequency. A node here sweeps or vibratos it. Negative runs the carrier backwards; 0 holds it at its start value (silence for a sine). Must be mono. |
| `shape` | word | `sine` | `sine` `tri` `square` `saw` | Carrier waveform. Square and saw are polyBLEP band-limited. |
| `mix` | signal | `1` | 0 .. 1 | Wet amount: `input * (1 - mix) + input * carrier * mix`. 0 is the input bit for bit. |

A signal field takes a number, `"$param"`, `"=expr"` or the name of another
node. Caps and errors: there is no count, size or factor to cap (the only
buffer is the document length). A number outside a range is an error, a
signal outside it is clamped per sample (a non-finite `freq` sample counts as 0):

- `freq: 30000: give a number between -24000 and 24000 Hz (half the sample rate)`
- `mix: 1.5: give a number from 0 to 1`
- `freq: \`st\` is stereo but this field is mono: add a mix node with "to": "mono" in between`

The node is causal (sample by sample, no lookahead) and not random (no seed).

### Recipe

A falling inharmonic clang: a 660 Hz tone, its sidebands swept by a falling
carrier, cut by a fast decay. Give the human a macro on the carrier ramp's
`to` (how far it falls) and on `mix`.

```json
[
  {"type": "osc", "name": "tone", "freq": 660, "amp": 0.6},
  {"type": "ramp", "name": "carrier", "from": 220, "to": 90, "time": 0.6, "curve": "exp"},
  {"type": "ringmod", "name": "clang", "input": "tone", "freq": "carrier", "mix": 0.8},
  {"type": "decay", "name": "fall", "time": 0.25},
  {"type": "math", "name": "hit", "op": "mul", "a": "clang", "b": "fall"}
]
```

### Measured

Tests in `crates/sfx_analysis/tests/ringmod.rs`, 48 kHz, 1 s renders, input a
1 kHz sine of amplitude 0.5.

| What | Measured | Test |
|---|---|---|
| carrier 300 Hz sine: spectral peaks | 699.99 and 1300.00 Hz | `ringmod_sidebands` |
| same: band level at 700 Hz and at 1300 Hz | -15.05 dB each (expected -15.05: amplitude 0.25) | `ringmod_sidebands` |
| same: band 990..1010 Hz | -120.0 dB (the floor; limit is -60) | `ringmod_sidebands` |
| carrier 300 Hz square: peaks | 100, 500, 700, 1300, 1900, 2500 Hz | `ringmod_square_carrier_adds_odd_harmonic_sidebands` |
| same: 1900 Hz relative to 1300 Hz | 9.55 dB down (1/3: 9.54) | same |
| `mix` 0 | bit-identical to the input | `ringmod_mix_crossfades_dry_and_wet` |
| `mix` 0.5: 1 kHz and 1300 Hz band levels | -15.05 and -21.07 dB (expected -15.05, -21.07) | same |
| carrier ramp 500 -> 100 Hz | upper sidebands -15.1 dB, lower -15.1 dB, 1 kHz -120.0 dB | `ringmod_takes_a_signal_for_freq_and_runs_both_channels` |
| stereo in (pan) | 2 channels, peaks 700 and 1300 Hz, L and R rms both -15.09 dB | same |
| 4 shapes x freq -24000, -1, 0, 1e-6, 24000 x mix 0, 1 at input amplitude 1e6; and a +-1e9 Hz sweep | all finite, peak at most 1.3e6 | `ringmod_is_finite_at_the_extremes` |
| two renders, 4 shapes, one thread vs the pool | same bits | `ringmod_is_bit_identical` |
| recipe above, 0.6 s | 28800 samples, hit rms -26.94 dB | `ringmod_recipe_renders` |

### Honest limits

- A square or saw carrier has harmonics out to Nyquist, so its upper
  sidebands (input + carrier harmonic k) fold back above Nyquist. The carrier
  itself is band-limited; the product is not. This was not measured; keep
  `freq` low, use `sine`, or low-pass after.
- `tri` is the leaky integral of the band-limited square (`dsp::Osc`), so its
  amplitude is near 1 but not exactly 1 and shifts a little with `freq`.
- There is no carrier phase field: the carrier starts at phase 0, so `freq` 0
  with a sine carrier outputs silence, and a `ramp` through 0 passes through
  silence.
- The output can be as large as the input (|carrier| <= about 1); it is not
  normalised.
- Extreme values stay finite (table above). None of this has been heard.
