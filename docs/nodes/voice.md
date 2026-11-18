# `glottis` and `tract` extensions

Both nodes live in `crates/sfx_graph/src/nodes/foley.rs`. Every new field is
optional and off by default: with none of them given (or all at 0) the output
is bit-identical to what the nodes produced before (nine default renders are
hash-pinned in `crates/sfx_analysis/tests/m8.rs`, recorded before the change).

## `glottis` (source): `roughness`, `subharmonic`, `fry`

A voice source (foley's LF pulse). The three new fields work per glottal
cycle: when a cycle starts the fields are read, two seeded draws (jitter,
shimmer) are taken, and the cycle's pitch and level are set for its whole length.
The draws come from their own generator (seed + a constant), so the breath
noise stream is the same with or without them. `glottis` still aliases above
about 1.5 kHz; this was not touched.

### Fields (new ones)

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `roughness` | signal | off (0) | 0 .. 1 | Per cycle: period moves by up to +-4 % x roughness (jitter), level by up to +-50 % x roughness (shimmer). |
| `subharmonic` | signal | off (0) | 0 .. 1 | Every second cycle is scaled by `1 - subharmonic`: a line at f0 / 2. 1 leaves one pulse in two. |
| `fry` | signal | off (0) | 0 .. 1 | Period jitter up to +-35 %, level jitter up to +-50 %, alternating long / short periods (+-25 %) with the long ones 50 % weaker, `tension` pushed halfway to pressed. |

A number outside 0..1 is an error (`roughness: 2: give a number from 0 to 1`);
a signal is clamped per sample (non-finite counts as 0). The period factor is
clamped to 0.3..1.7 and the pitch to 0.45 x sample rate, so extremes cannot
double-wrap the phase. Seeded (`seed` as before); causal in kind (no
lookahead), `random()` is already true.

## `tract` (filter): `nasal`, `frication`, `growl`

The Klatt formant cascade. New stages, in signal order: growl (on the input),
nasal pair, the formants, then frication added to the output.

### Fields (new ones)

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `nasal` | signal | off (0) | 0 .. 1 | A pole (peak at 250 Hz, +9 dB, Q 2.5) and a zero (notch at 1 kHz, Q 2) ahead of the formants: `x + nasal * (pair(x) - x)`. DC and highs pass unchanged. |
| `frication` | `{freq, level}` | off | see below | White noise through a band-pass (Q 2, band `freq / 2` wide, unity gain at `freq`), times `level`, added after the formants. Not shaped by `input`: gate it with `level`. |
| `frication.freq` | signal | required | Hz, 20 .. 0.45 x sr (21600 at 48 kHz) | Band centre; a number outside is an error, a signal is clamped. |
| `frication.level` | signal | required | >= 0 | Linear level; a negative number is an error, a negative signal counts as 0. |
| `growl` | signal | off (0) | 0 .. 1 | Amplitude modulation of the input by a raised cosine at about 45 Hz (rate wanders +-30 %, seeded), `1 - growl` at the troughs. |
| `seed` | integer | 0 | | Seeds `frication` and `growl` (separate streams). The node is `random()` only when one of them is given. |

Errors: `frication.freq: 30000: give a number from 20 to 21600 Hz (0.45 x the
sample rate)`, `frication.level: -1: give a number >= 0`, an unknown key in
`frication` lists `freq` and `level`. Nothing scales with a field, so there is
no cap to state.

### Recipe

A creaky, nasalised vowel with a breath of air: pitch falls, fry and growl
rise with it, a fricative band sweeps up.

```json
[
  {"type": "ramp", "name": "pitch", "from": 150, "to": 80, "time": 0.8},
  {"type": "ramp", "name": "creak", "from": 0, "to": 1, "time": 0.8},
  {"type": "ramp", "name": "air_hz", "from": 2500, "to": 7000, "time": 0.8},
  {"type": "wander", "name": "air_lv", "range": [0.0, 0.1], "rate": 3},
  {"type": "glottis", "name": "voice", "f0": "pitch", "fry": "creak", "subharmonic": 0.2, "seed": 4},
  {"type": "tract", "name": "mouth", "input": "voice", "vowel": "o", "nasal": 0.5, "growl": 0.3,
   "frication": {"freq": "air_hz", "level": "air_lv"}, "seed": 4}
]
```

Give the human macros on `fry`, `nasal` and `growl`. Macro claims: `fry` and
`growl` raise `swing`/roughness, `nasal` moves the centroid down.

### Measured

Tests in `crates/sfx_analysis/tests/m8.rs`, 48 kHz. "S8a roughness" is
`sfx_analysis::measure::roughness_pct` (envelope energy 20..150 Hz) of a 1 s render.

| What | Measured | Test |
|---|---|---|
| 9 default renders (glottis x4, tract x5) | hashes unchanged from before the change | `defaults_are_bit_identical_to_before` |
| every new field at 0 | same hash as the default | `fields_at_zero_equal_the_defaults` |
| `roughness` 0 / 0.5 / 1 at f0 250: S8a roughness | 0.00 / 0.87 / 3.29 % | `roughness_raises_the_envelope_roughness` |
| `roughness` 0.5 at f0 200: median YIN pitch | 200.0 Hz | `roughness_keeps_the_pitch` |
| `subharmonic` 0 / 0.25 / 0.5 / 1 at f0 200: band 96..104 Hz | -120 / -38.0 / -32.0 / -25.9 dB (f0 band -16.6 / -17.7 / -19.1 / -22.6) | `subharmonic_puts_a_peak_at_half_f0` |
| `subharmonic` 1: lowest strong peak | within 2 Hz of 100 Hz | same |
| `fry` 0 / 0.5 / 1 at f0 250: S8a roughness | 0.00 / 1.39 / 6.94 % | `fry_raises_the_envelope_roughness_and_adds_half_f0` |
| `fry` 1 at f0 200: band 90..110 Hz | -42.4 dB (off: -120) | same |
| `nasal` 0.5 / 1 on white noise: 200..300 Hz | +4.1 / +7.1 dB | `nasal_adds_a_murmur_and_an_antiformant` |
| same: 900..1100 Hz | -5.5 / -13.4 dB | same |
| same: 4..8 kHz | -0.0 dB | same |
| `frication` freq 1500 / 4000 / 9000, level 1: centroid | 2258 / 4955 / 9467 Hz | `frication_band_follows_freq_and_level` |
| same, rms | -18.2 / -14.3 / -12.0 dB | same |
| `frication` freq 5000, level 0.01 / 0.1 / 1: rms | -53.5 / -33.5 / -13.5 dB | same |
| freq ramp 1500 -> 10000, level ramp 0 -> 1: first vs last quarter | centroid 2812 vs 8599 Hz; rms -28.6 vs -12.6 dB | `frication_takes_signals_for_freq_and_level` |
| `growl` 0 / 0.5 / 1 on a glottis f0 250: S8a roughness | 0.00 / 1.22 / 9.84 % | `growl_raises_the_envelope_roughness` |
| two seeds of `roughness` | differ; same seed identical | `roughness_is_seeded` |
| all fields at once, one thread vs pool | same bits | `new_fields_are_bit_identical_on_any_thread_count` |
| recipe-style graph, 0.6 s | 28800 samples, rms -13.58 dB, peak 0.86 | `recipe_renders` |
| f0 0 / 1 / 30000 x fields 0 / 1 x tension 5, breath 3, scale 10, frication level 1e6; signals -5..5 and +-1e9 Hz | all finite | `extremes_stay_finite` |

### Honest limits

- The measurements are of envelope energy and spectral lines, not of how
  anything sounds; none of it has been heard.
- The frication centroid is above `freq` (2258 Hz for 1500) because the Q 2
  band-pass has wide skirts and the noise is white; `freq` sets the band, not
  the exact centroid. The test only requires it within 0.6 to 1.6 x `freq`.
- `nasal` is a fixed peak and notch (250 Hz / 1 kHz), not a retuned
  waveguide or a damping of F1; the zero at 1 kHz also lowers the vowel's own
  partials there (-13 dB at full). Frequencies are not fields.
- `growl` is an AM at about 45 Hz, applied before the formants (they smooth
  it a little); the rate is not a field. `fry` is a pulse-timing model with a
  pressed shift, not a creak with double pulses.
- `subharmonic` scales every second cycle in amplitude only, not in pitch.
- Roughness values are small in percent terms (a few %), because the S8a
  measure is a share of the whole envelope spectrum including DC.
- `glottis` still aliases above about 1.5 kHz (unchanged).

## Natural glottis, tract and bubble

Batch 2 blind labels: glottis -> tract creature calls were heard as "SAM"
(1980s speech synthesis), "muffled groan, artificial"; `bubble` drips as an
"iPhone keyboard click" and a "cartoon thought-bubble pop". Measured causes:
a perfectly periodic, aliasing LF pulse with flat breath noise; fixed,
narrow formants; a lone clean Minnaert sine. Nothing here was heard.

Each node takes `natural` (default **true**); `natural: false` is the old
output bit for bit (the M8 hash pins now set it).

### glottis

- per-cycle jitter 0.6 % and shimmer 6 % (peak), added to `roughness`;
- a slow seeded pitch drift, +-12 cents peak, ~1 Hz;
- the pulse rendered 4x oversampled through the halfband chain (`Oversampler`);
- aspiration high-passed (first difference) and gated by the glottal flow
  (`0.1 + 0.9 flow`), default `breath` 0.08 (was 0.05).

### tract

- every bandwidth x1.4 (presets and `formants` lists);
- each formant drifts on its own by up to +-3 %, ~0.7 Hz, seeded (the node
  is now random by default).

### bubble

- a high-passed noise tick at birth (time constant 0.8 ms, 0.4x the ring);
- extra seeded damping, 1.15..1.5x foley's decay rate;
- +-15 % noisy amplitude wobble (~250 Hz smoothing).

### Measured (`crates/sfx_analysis/tests/voice_fix.rs`)

| what | old | natural |
|---|---|---|
| glottis f0 2900: off/on-grid energy above 4 kHz minus below | 21.1 dB | 6.1 dB |
| glottis f0 140: YIN pitch spread over 2 s | 3.1 cents | 25.0 cents |
| glottis: energy share above 6 kHz | -29.6 dB | -27.3 dB |
| tract /a/: centroid by tenth, max / min | 1.012 | 1.037 |
| bubble 2.5 mm: first 4 ms above 6 kHz | -78.9 dB | -34.5 dB |
| bubble: decay to -20 dB | 0.02 s | 0.01 s |
| bubble: whole-sound flatness | 0.0005 | 0.0113 |
| bubble: pitch | 1657 Hz | 1637 Hz |

Limits: the tract is still a 4-formant cascade with no sub-glottal coupling
or consonants; voiced and harmonicity still read 1.00 on a vowel.
