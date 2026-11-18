# Filterbank, vocoder, multiband

Three band-split nodes in `crates/sfx_graph/src/nodes/bands.rs`; tests in
`crates/sfx_analysis/tests/bands.rs`. All are causal (no lookahead) and not
random (no seed). Non-finite input samples count as 0.

## `filterbank`

A bank of log-spaced band-pass filters, each scaled by a gain and summed
(filter family): a graphic equalizer, or a way to carve noise into bands.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | Stereo: each channel through its own bank. |
| `bands` | whole number | required | 1 .. 64 | The first band is centred on `low`, the last on `high`, the rest log-spaced between; one band sits at their geometric mean. |
| `gains` | list of numbers | all 1 | each -1000 .. 1000, linear | One per band, lowest first. A shorter list leaves the rest at 1; a longer one is an error. 0 removes a band, a negative gain flips its sign. |
| `q` | number | neighbours cross at -3 dB | 0.1 .. 100 | Quality factor of every band-pass (2nd order, 0 dB at the centre, 6 dB an octave skirts). |
| `low` | number | 80 | Hz, 20 .. 0.45 x sample rate | Centre of the lowest band. |
| `high` | number | 12000 (or 0.45 x sample rate if lower) | Hz, above `low`, up to 0.45 x sample rate | Centre of the highest band. |

The default Q for `n` bands over `o` octaves is `sqrt(r) / (r - 1)` with `r =
2^(o / (n - 1))` (one band: `r = 2^o`). The fields are plain numbers (no
signals). Caps and errors:

- `bands: 65: give a whole number from 1 to 64`
- `gains: 5 values for 4 bands: give at most one gain per band`
- `gains[i]: 5000: give a linear gain between -1000 and 1000`
- `low: 5000: give a frequency below high (1000)`
- `high: 30000: give a frequency between 20 and 21600 Hz (0.45 x the sample rate)`
- `q: 0: give a number from 0.1 to 100`

### Recipe

A comb-like hollow noise: white noise, ten bands from 100 Hz to 5 kHz with
every third one kept. Give the human a macro on the `gains` entries
(`index` maps) to tilt or open the holes.

```json
[
  {"type": "noise", "name": "n"},
  {"type": "filterbank", "name": "shaped", "input": "n", "bands": 10, "low": 100, "high": 5000,
   "gains": [1, 0, 0, 1, 0, 0, 1, 0, 0, 1]}
]
```

### Measured

48 kHz, 1 s of white noise (amplitude 0.3), `crates/sfx_analysis/tests/bands.rs`.

| What | Measured | Test |
|---|---|---|
| 12 bands, all gains 1: octave-band levels (200 Hz .. 6.4 kHz) against the input | +2.80, +2.98, +2.68, +2.97, +2.46, +2.38 dB; spread 0.61 dB | `filterbank_gains_scale_and_sum` |
| same, all gains 0.5 against all gains 1 | -6.021 dB | same |
| 8 bands 100 Hz .. 6.4 kHz, band 1 alone (181 Hz): level at 0.9-1.1x, 0.5x, 2x, 4x, 0.25x its centre | -44.0, -54.8, -48.9, -54.4, -64.3 dB | `filterbank_one_band_passes_around_its_centre` |
| same, bands 1 .. 6: worst 4x-away window against the band | 9.2 dB down (the skirt of one 2nd-order band-pass) | same |
| 1 band, q 4: spectral peak (geometric mean of 80 and 12000 is 980 Hz) | 1011 Hz | `filterbank_stereo_and_single_band` |
| 64 bands, 10 s at 48 kHz | 0.05 s render (optimized test profile) | `render_time_is_stated` |

### Honest limits

The bank is not a constant-power splitter: with all gains 1 the summed
bands raise white noise by about 2.4 .. 3.0 dB (the bands overlap and add in
phase near their crossings), and the sum is not flat for tones; the 0.61 dB
spread was measured on noise octave bands only. Single band-passes have 6 dB
an octave skirts, so a narrow cut (`gains` 0) leaves the neighbours' skirts
inside it: a band alone is only 9 .. 11 dB down in a window 4 times its
centre frequency away. Finite at 1 and 64 bands, `q` 0.1 and 100, gains +-1000, `low` 20 /
`high` 21600 and an input of 1e6. None of it has been heard.

## `vocoder`

A channel vocoder (filter family): the modulator's level in each log-spaced
band scales the same band of the carrier, so the carrier takes on the
modulator's spectral shape over time.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `carrier` | signal | required | mono or stereo audio | The sound that is shaped. Stereo: each channel through its own synthesis bank, sharing the modulator's envelopes. Needs energy in every band the modulator has. |
| `modulator` | signal | required | mono audio | The sound whose band levels are imposed. Stereo is an error. |
| `bands` | whole number | 16 | 1 .. 64 | Log-spaced from `low` to `high` as in `filterbank`. |
| `attack` | number | 0.01 | seconds, 0 .. 10 | Rise time constant of the band envelope (applied to its power). |
| `release` | number | 0.04 | seconds, 0 .. 10 | Fall time constant. Short times follow the modulator closely but ripple in the low bands. |
| `q` | number | neighbours cross at -3 dB | 0.1 .. 100 | The -3 dB width of one 2nd-order band-pass of this Q; each band is three in a row with their Q scaled to keep that width (18 dB an octave skirts). |
| `low` | number | 80 | Hz, 20 .. 0.45 x sample rate | Centre of the lowest band. |
| `high` | number | 12000 (or 0.45 x sample rate if lower) | Hz | Centre of the highest band. |

Per band: the modulator goes through the band's filter, its power through a
follower with `attack` / `release`, and `sqrt(2 x power)` (the amplitude of the
sine with that power) multiplies the same band of the carrier. A sine
modulator of amplitude A at a band's centre therefore scales that band of the
carrier by A: the output is as quiet as the modulator (A = 0.1 is -20 dB), so
follow it with a `gain` or `normalize`. Errors:

- `bands: 0: give a whole number from 1 to 64`
- `release: 11: give seconds between 0 and 10`
- `modulator: \`st\` is stereo but this field is mono: add a mix node with "to": "mono" in between`

### Recipe

A talking buzz: a 110 Hz saw as the carrier, pink noise swept through a moving
band-pass as a stand-in for speech (use a recording through `sample` for the
real thing). Give the human macros on `release` (how smeared the words are)
and `bands`.

```json
[
  {"type": "osc", "name": "buzz", "freq": 110, "shape": "saw", "amp": 0.5},
  {"type": "noise", "name": "speech", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 3, "range": [300, 2500]},
  {"type": "filter", "name": "formant", "input": "speech", "mode": "bp", "freq": "sweep", "q": 4},
  {"type": "vocoder", "name": "voice", "carrier": "buzz", "modulator": "formant", "bands": 16, "attack": 0.005, "release": 0.03}
]
```

### Measured

48 kHz, `crates/sfx_analysis/tests/bands.rs`.

| What | Measured | Test |
|---|---|---|
| 16 bands, white carrier, modulator white noise low-passed at 1.5 kHz (12 dB an octave): octave levels of the modulator at 250 .. 8000 Hz | -36.5, -33.3, -31.4, -33.9, -41.3, -51.2 dB | `vocoder_output_bands_track_the_modulator` |
| same: (output - carrier) - modulator per octave, mean removed (card limit 3 dB) | -0.73, -1.16, -0.60, -0.11, +0.58, +2.02 dB; worst 2.02 dB | same |
| same: 250 Hz against 8 kHz octave, output minus carrier / modulator | 12.0 / 14.7 dB | same |
| sine at one band's centre as modulator (bands 1, 3, 5, 6 of 8): amplitude 0.2 against 0.1 | +6.02 dB in all four (expected 6.02) | `vocoder_band_level_follows_the_modulator_level` |
| same: that band against a window two octaves away (4x wider in Hz, white carrier) | 19.1, 19.4, 20.2, 26.7 dB | same |
| modulator gated off after 0.5 s: output rms while on / 0.25 s .. 1 s after | -43.5 / -83.5 dB | `vocoder_follows_the_modulator_in_time` |
| silent modulator | output at or below -100 dB | same |
| 16 / 64 bands, 10 s at 48 kHz | 0.13 / 0.35 s (optimized test profile) | `render_time_is_stated` |

With two sections per band instead of three the same noise test measured a
worst octave error of 3.96 dB (the loud low bands leaked into the quiet high
ones), which is why each band is three sections.

### Honest limits

The tracking was measured with noise and a sine modulator through a white
noise carrier; with a harmonic carrier (a buzz) the energy in a band is
whatever harmonics fall in it, so low bands with no harmonic in them stay
quiet. The modulator's loud bands still leak into far quiet ones through the
18 dB an octave skirts (a 20 dB tilt over three octaves gave the 2 dB
error above; a steeper tilt will give more). There is no unvoiced/sibilance
path: noisy consonants in the modulator only come through where the carrier
has energy. Latency: none added, but the band filters ring for a few
milliseconds. Finite with 1 and 64 bands, attack and release 0 and 10 s, and
inputs of 1e6. None of it has been heard.

## `multiband`

A multiband processor (dynamics family): Linkwitz-Riley 4th-order crossovers
split the sound into bands, each band can be driven, compressed and scaled,
and the bands are summed again. Ports: `out` (the sum) and `band0` ..
`band7` (each processed band alone).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | Stereo channels are split separately and share each band's compressor detector (one gain for both). |
| `crossovers` | list of numbers | required | Hz, 1 .. 7 of them, ascending, each 20 .. 0.45 x sample rate | N crossovers make N + 1 bands, lowest first. |
| `bands` | list of objects | all neutral | at most one per band | Settings per band, lowest first. A shorter list leaves the rest neutral; a longer one is an error. |

Each `bands` entry (every field optional; all left out passes the band
through unchanged, bit for bit):

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `drive` | number | 0 (off) | 0 .. 100 | Saturation `tanh(drive x) / tanh(drive)`: full scale stays full scale, peaks round off, quiet material rises by about `drive`. Not oversampled. |
| `threshold_db` | number | none (compressor off) | dBFS, -120 .. 24 | Giving it turns the compressor on: feed-forward, peak detector, hard knee. |
| `ratio` | number | 4 | 1 .. 100 | Each dB over the threshold comes out `1 / ratio` dB over. Needs `threshold_db`. |
| `attack` | number | 0.01 | seconds, 0 .. 10 | Detector rise time constant. Needs `threshold_db`. |
| `release` | number | 0.1 | seconds, 0 .. 10 | Detector fall time constant. Needs `threshold_db`. |
| `gain_db` | number | 0 | dB, -60 .. 60 | Applied last, after drive and compression. |

The order inside a band is drive, compressor, gain. Every field is a plain
number. Errors:

- `crossovers: 0 values: give from 1 to 7 frequencies, ascending`
- `crossovers[1]: 500: give frequencies in ascending order (the one before is 1000)`
- `crossovers[i]: 30000: give a frequency between 20 and 21600 Hz (0.45 x the sample rate)`
- `bands: 3 entries for 2 bands (1 crossovers): give at most one per band`
- `bands[0].ratio: 4: the compressor is off; add threshold_db to this band`
- `bands[1].gain_db: 90: give dB between -60 and 60`

How the crossovers sum flat: the sound is split from the bottom up (the
lowest band comes off first, the rest goes on to the next crossover), and
each band below the top is then passed through the all-pass (low + high) of
every crossover above it. All bands then carry the phase of the whole chain, so
their sum has a flat magnitude with no notch at a crossover. The phase of the
sum is that of the crossovers' all-passes, not zero.

### Recipe

A thicker hit: a struck body through a three-band split, low band squeezed and
lifted, high band driven. Give the human macros on the low band's `threshold_db`
(punch) and the high band's `drive` (grit).

```json
[
  {"type": "impulse", "name": "click"},
  {"type": "resonators", "name": "body", "input": "click", "modes": [[90, 0.3, 1.0], [210, 0.2, 0.7], [1800, 0.08, 0.5]]},
  {"type": "multiband", "name": "hit", "input": "body", "crossovers": [150, 1000],
   "bands": [{"threshold_db": -18, "ratio": 4, "attack": 0.005, "release": 0.08, "gain_db": 3},
             {},
             {"drive": 2, "gain_db": -3}]}
]
```

### Measured

48 kHz, `crates/sfx_analysis/tests/bands.rs`.

| What | Measured | Test |
|---|---|---|
| neutral, crossovers 200 / 1000 / 5000, white noise: power in 8 regions (including 180-220 and 950-1050 Hz) against the input; card limit 0.5 dB | worst 0.0107 dB | `multiband_neutral_sums_flat` |
| same, sines at 60, 200, 350, 1000, 2236, 5000, 9000 Hz | worst level change 0.0000 dB | same |
| a sine at the crossover (1 kHz, one crossover): low band and high band against the input | -6.02 dB and -6.02 dB | `multiband_crossover_is_6_db_down_in_each_band` |
| 100 Hz in the band above a 1 kHz crossover | -89.1 dB | same |
| crossovers 150 / 6000, middle band at -12 dB: level change at 30-100 Hz / 800-1250 Hz / 8-14 kHz | -0.31 / -11.97 / -0.44 dB | `multiband_band_gain_moves_only_its_band` |
| compressor on band 0 (-20 dB, 4:1), 100 Hz sine at amplitude 0.8 (expected about -13.5 dB) | -13.08 dB; a 3 kHz sine in band 1: 0.00 dB | `multiband_compresses_only_its_band` |
| ratio 1 | 0.000 dB | same |
| drive 20 on band 0, 100 Hz sine at 0.5: 300 Hz against the fundamental | -10.1 dB (drive 0: -111.0 dB); peak 1.042 | `multiband_drive_adds_harmonics_in_its_band` |
| stereo noise panned 0.5, compressor on band 0: L - R level after / before | -7.66 / -7.66 dB | `multiband_stereo_runs_both_channels_with_one_gain` |
| 8 bands (7 crossovers), drive and compressor on each, stereo-linked, 10 s at 48 kHz | 0.16 s (optimized test profile) | `render_time_is_stated` |

### Honest limits

Per-band settings are numbers, not signals; you cannot automate a threshold.
The compressor is a plain peak feed-forward with a hard knee and no lookahead,
so a fast transient passes through for about `attack` before it is turned
down; the 4:1 test settled 0.4 dB short of the textbook value (the detector's
release ripple at 100 Hz). Drive is not oversampled: harmonics above Nyquist
fold back (not measured). The 24 dB an octave LR4 skirts leak a little into neighbouring bands, and
the leakage adds in phase: with crossovers 300 / 3000 a -12 dB cut of the
middle band measured -11.49 dB at 900-1100 Hz (-12.00 expected), which is why
the test uses crossovers 150 / 6000 (-11.97 dB) and shows -0.31 and -0.44 dB
in the bands beside it. Crossovers closer than a few Hz apart are legal and
finite (tested at 5 Hz and at the 20 Hz and 21.6 kHz ends) but the band
between them is almost empty. Memory: the bands of the whole document length
are held at once (8 bands x channels x length floats). None of it has been
heard.
