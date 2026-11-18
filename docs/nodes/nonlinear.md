# Shape (oversampling, curves, mix), compress.key

<!-- Measured by crates/sfx_analysis/tests/it/nonlinear.rs. -->

## `shape`

A memoryless waveshaper (nonlinear family): `input * drive` goes through a
curve, optionally run at 2, 4 or 8 times the sample rate, then blended with
the dry input.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | Stereo is shaped per channel. |
| `mode` | word | `tanh` | `tanh` `clip` `fold` `bitcrush` `rectify` `asym` `diode` `tube` `table` | The curve (below). |
| `drive` | signal | 1 | linear gain before the curve | May be a node. Applied at the input rate, so it stays in time with the signal when oversampling. |
| `bits` | number | 8 | 1..24 | `bitcrush` only. |
| `bias` | number | 0.5 | any finite number | `asym` only (other modes: error). |
| `points` | `[[x, y], ..]` | none | x strictly increasing, at most 4096 points | `table` only (required there; other modes: error). |
| `oversample` | integer | 1 | 1, 2, 4, 8 | Other values error with `oversample: 3: give 1, 2, 4 or 8`. Inside a feedback loop anything but 1 is an error naming the field. |
| `mix` | signal | 1 | 0..1 (clamped) | 1 is the shaped signal, 0 is the input before `drive`; both ends are exact (bit for bit). |

Curves, all with slope 1 at 0 except where noted:

- `asym`: `tanh(u + bias) - tanh(bias)`. Slope at 0 is `sech^2(bias)` (0.79 at the default).
- `diode`: `1 - exp(-u)` for `u >= 0`, `-0.5 (1 - exp(2u))` below. Rails +1 and -0.5.
- `tube`: `u / (1 + u^4)^(1/4)` for `u >= 0` (rail +1, sharp knee), `0.7 tanh(u / 0.7)` below (rail -0.7).
- `table`: linear between points, held at the first and last y beyond them. When every x is >= 0 the curve is odd-extended (`y(-x) = -y(x)`); a point at x = 0 must then have y = 0.

The asymmetric curves leave a DC offset on a symmetric input. Follow them
with an `onepole` high-pass (`verify`'s `dc` check says when it matters).
Input beyond +-1e9 is clamped, and a non-finite result is replaced by 0.

### Recipe

```json
[
  {"type": "osc", "name": "tone", "shape": "saw", "freq": 110},
  {"type": "shape", "name": "grit", "input": "tone", "mode": "asym", "drive": 4, "oversample": 4, "mix": 0.6},
  {"type": "onepole", "name": "clean", "input": "grit", "mode": "hp", "freq": 30}
]
```

Macros worth giving the human: `drive` (as a `gain` before the shape, or the
field itself), `mix` as "amount". Aliasing is lower with `oversample` 4 at a
cost of four times the work for this node.

### Measured

| What | Measured | Test |
|---|---|---|
| 5 kHz sine, `clip`, drive 8: off-grid energy below 20 kHz, 1x vs 2x / 4x / 8x | 15.0 / 24.8 / 45.0 dB less (1x: -14.7 dB below the harmonics) | `oversampling_cuts_the_aliasing_of_a_clipped_sine` |
| 1 kHz, amp 0.5, drive 2: 2nd harmonic, `tanh` vs `asym` | -115.2 dBc vs -15.5 dBc | `asym_makes_a_second_harmonic_where_tanh_does_not` |
| 1 kHz, amp 0.5, drive 3: 2nd harmonic, `diode` / `tube` | -20.0 / -23.0 dBc | same |
| `table` `[[-1,-1],[1,1]]` and `[[0,0],[1,1]]` | the input bit for bit | `a_table_with_the_identity_returns_the_input_bit_for_bit` |
| three-point odd-extended table vs the same curve in f64 | worst error 6e-8 | `a_table_interpolates_holds_its_ends_and_extends_oddly` |
| `mix` 0 / 0.5 | input bit for bit / (dry + wet) / 2 within 1e-6 | `mix_blends_dry_and_wet` |
| identity curve, oversample 2: error against the input at 200 / 2000 / 8000 Hz | -106.5 dB at all three (whole-sample delay, removed exactly) | `oversampling_keeps_the_signal_in_time` |
| identity curve, oversample 4, mix 1: same | -37.6 / -17.7 / -5.7 dB (half-sample delay left in) | same |
| identity curve, oversample 4, mix 0.5: same | -106.5 / -84.7 / -37.8 dB | same |
| identity curve, oversample 8, mix 1 / mix 0.5 | -43.7 / -23.7 / -11.7 and -106.5 / -87.3 / -40.5 dB | same |
| extremes: 9 modes x drive 0 .. 1e30 and -50 x 1/2/4/8x, mix 0.7 | finite everywhere | `every_mode_stays_finite_at_the_extremes` |
| feedback loop (delay 2.27 ms, gain 0.95) with asym / diode / tube / table | finite, rms -2.1 / -5.8 / -2.6 / -1.6 dB | `inside_a_feedback_loop_oversampling_is_refused_and_the_new_modes_work` |

### Honest limits

- The oversampler is a linear-phase half-band chain. Its delay is taken out so
  `shape` stays in time with its input: whole samples at 2x, but 4x and 8x
  have a fractional delay (a multiple of 1/4 sample). At `mix` 1 it is rounded,
  leaving up to half a sample of offset (the numbers above are that offset
  measured against the input, not distortion). When `mix` is below 1 or a
  signal, the wet path is shifted by third-order Lagrange interpolation so it
  lines up with the dry one: that is flat to 80 dB at 2 kHz but only 38 dB at
  8 kHz.
- 4x takes the aliasing of a hard clip at drive 8 down by 24.8 dB below 20 kHz
  (the clip's own 1/k^2 spectrum folds back at any filter quality); 8x by 45 dB.
  The input is band-limited to 0.45 of the sample rate on the way up, so
  content above 21.6 kHz at 48 kHz is removed by oversampling.
- `bitcrush` and `fold` are meant to alias; oversampling them removes the
  harshness along with the character.
- The curves are static functions chosen for their harmonic content, not
  models of a diode or tube circuit. None of this has been heard.
- Not in a feedback loop: `oversample` (error); `mix` and the other modes work.

## `compress.key`

`compress` gains `key`: any audio node (mono or stereo, the loudest channel
counts) that drives the level detector. The gain it asks for is applied to
`input`. Without `key` the input drives itself as before.

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `key` | signal | the input | audio | Detector input (ducking, pumping). |

### Recipe

```json
[
  {"type": "compress", "name": "duck", "input": "pad", "key": "kick",
   "threshold_db": -24, "ratio": 6, "attack": 0.002, "release": 0.15}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| 1 kHz tone at -26 dBFS, keyed by a 4 Hz full-scale pulse, threshold -20, 4:1, no knee: gain in the pulses / between them / same compressor without the key | -15.00 dB (20 windows) / -0.09 dB / 0.00 dB (expected -15 dB for 20 dB over at 4:1) | `the_key_makes_the_compressor_listen_to_another_signal` |

### Honest limits

The detector is the existing peak follower; a key that is silent for longer
than the release leaves the input untouched. `key` is not allowed to be an
event stream (error names the field).
