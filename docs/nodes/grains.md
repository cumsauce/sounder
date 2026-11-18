# `grains` per-grain size and pitch, and `cloud`

Two granular changes in `crates/sfx_graph/src/nodes/events.rs`. `grains`
(existing) now takes `size` and `pitch` as a number, a signal or a random
`{min, max}` range. `cloud` (new) is a grain cloud that schedules itself from
a density signal, with no event stream.

## `grains` (extended)

Granular playback: one windowed grain of a source per event. Everything that
worked before still works and renders the same bits (see Measured).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `source` | node name or `.wav` | required | audio | The sound grains are cut from. |
| `events` | event stream | required | | One grain per event, scaled by the event value. |
| `size` | number, signal or range | required | seconds, above 0 | Grain length. A **number** is fixed. A **`{min, max, [curve]}` range** draws a length for each grain (`curve` `linear` default, `exp`, `smooth`). A **signal** is read at the grain's start sample (`"$param"`, `"=expr"`, a node). A length under 2 samples counts as 2. Was number-only. |
| `position` | signal | `0` | 0 .. 1 of the source | Where each grain starts reading. |
| `pitch` | number, signal or range | `1` | ratio >= 0 | Playback ratio (2 = octave up). Same three forms as `size`. A range needs `min` >= 0, and `curve: "exp"` needs both ends above 0 (even spread in octaves). A signal below 0 counts as 0. |
| `window` | word | `hann` | `hann` `tri` `rect` | Grain envelope. |
| `pan_spread` | number | none | 0 .. 1 | Random pan per grain; makes the output stereo. |
| `jitter` | number | `0` | >= 0 | Random shift of each grain's position, +-jitter of the source. |
| `seed` | integer | `0` | | Mixed into the document seed. |

Random draws per event, in this order: pan (only if `pan_spread` > 0),
position jitter (only if `jitter` > 0), size (only for a range), pitch (only
for a range). A document that uses neither range draws exactly what it drew
before X3. Work cap: events x grain length (at most the document length) above
2e9 samples is an error; for a range the longest grain (`max`) is used, for a
signal the work is added up as grains render.

Errors, field first:

- `size: min 0.05 is above max 0.01: swap them`
- `size: 0: give a number > 0`
- `size: min 0: give a range that stays above 0`
- `pitch: min -1: give a range that stays at least 0`
- `pitch: curve "exp" needs min and max above 0 (got 0..2): use "linear"`
- `field \`size\`: unknown field \`mx\`, expected one of \`min\`, \`max\`, \`curve\``

## `cloud`

A grain source with its own scheduler (granular family, source). Each sample
it adds `density / sample rate` to a running total; when the total reaches the
next threshold a grain starts. Grain starts with `mode: "random"` are a
Poisson process (the threshold is an exponential draw with mean 1, so a
changing density stays exact); with `"regular"` they are even (threshold 1).
The grains are not normalised: level rises with density x size (overlap).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `source` | node name or `.wav` | required | audio | The sound grains are cut from. |
| `density` | signal | required | grains/s, 0 .. sample rate | Grain rate, read every sample. A number outside 0..sr is an error; a signal is clamped per sample (a non-finite sample counts as 0). 0 makes no grains. |
| `size` | number, signal or range | required | seconds, above 0 | Grain length; same three forms as on `grains`. |
| `position` | signal | `0` | 0 .. 1 of the source | Where each grain starts reading, read at the grain's start. |
| `jitter` | number | `0` | >= 0 | Random shift of each grain's position, +-jitter of the source. |
| `pitch` | signal | `1` | ratio >= 0 | Base playback ratio, read at the grain's start. |
| `pitch_spread` | number | `0` | semitones, 0 .. 120 | Random detune per grain, uniform in +-`pitch_spread` semitones around `pitch`. |
| `mode` | word | `random` | `random` `regular` | Poisson or even grain spacing. |
| `window` | word | `hann` | `hann` `tri` `rect` | Grain envelope. |
| `pan_spread` | number | none | 0 .. 1 | Random pan per grain; makes the output stereo. |
| `seed` | integer | `0` | | Mixed into the document seed. |

Caps: at most 200000 grains in a sound (`density: more than 200000 grains by
5.00 s: lower density or shorten the sound`), and the same 2e9-sample work cap
as `grains`. Random draws per grain, in this order: threshold (next grain),
pan, position jitter, size (range only), detune (only if `pitch_spread` > 0).
`pan_spread` makes the output stereo. The node reads its source at any
position, so it is not causal (the live-runtime flag is false); it is random.

Other errors:

- `density: 1000000000: give grains per second from 0 to 48000 (the sample rate)`
- `pitch_spread: 500: give semitones from 0 to 120`
- `pan_spread: 2: give 0..1`
- `source: \`ev\` is an event stream, not audio: render events through \`voices\``

### Recipe

A swarm that thickens: a 330 Hz tone cut into 20 to 80 ms grains, the grain
rate sweeping 4 -> 160 per second, each grain detuned up to 2 semitones, with
random pan. Give the human macros on the density ramp's `to` (how thick it
gets) and on `pitch_spread` (how smeared the pitch is).

```json
[
  {"type": "osc", "name": "tone", "freq": 330, "amp": 0.5},
  {"type": "ramp", "name": "swell", "from": 4, "to": 160, "time": 2.0, "curve": "exp"},
  {"type": "cloud", "name": "swarm", "source": "tone", "density": "swell",
   "size": {"min": 0.02, "max": 0.08}, "position": 0.1, "pitch_spread": 2, "pan_spread": 0.6}
]
```

For `grains`, the same range forms: `"size": {"min": 0.01, "max": 0.05}` and
`"pitch": {"min": 0.8, "max": 1.25, "curve": "exp"}` over a `poisson` stream
give every grain its own length and pitch.

### Measured

Tests in `crates/sfx_analysis/tests/x3.rs`, 48 kHz. Grain lengths are read
from runs of a rect-windowed grain over a constant source; frequencies from
sign changes of a 1 kHz sine source (about 1 % resolution).

| What | Measured | Test |
|---|---|---|
| existing `grains` (4 documents: poisson noise, pattern with position and pitch ramps and tri window, burst with stereo/jitter/rect/seed, fixed position and pitch with pan) | FNV-1a of the output bits equal to the values recorded from the code before the change: `0x7c42577fa478ec9f`, `0xe15fdb0f14b53635`, `0xc546197563e0b6c8`, `0x60e87bf68e4355b0` | `existing_grains_are_bit_identical_to_before_x3` |
| `size` `{0.01, 0.03}` linear, 24 grains | lengths 515..1381 samples (limits 480..1440), mean 973 (uniform: 960) | `grains_size_range_draws_a_length_per_grain` |
| same, `curve: exp` | median 865 samples (geometric centre 831) | same |
| `size` `{0.02, 0.02}` vs `0.02` | all 960 samples, equal | same |
| same seed / other seed | same lengths / different lengths | same |
| `size` from a 0.005 -> 0.03 s linear ramp | 24 grains, first 252 samples, last 1356, worst error against the ramp 0 samples | `grains_size_can_be_a_signal` |
| `pitch` `{0.5, 2}` exp, five grains of 1 kHz | 875, 1667, 1817, 1900, 1325 Hz (limits 500..2000) | `grains_pitch_range_draws_a_ratio_per_grain` |
| `pitch` `{1.5, 1.5}` | 1492 Hz on all five | same |
| `cloud` density 200/s for 4 s, `regular` / `random` | 799 / 827 grains (want 800; Poisson sd 28) | `cloud_density_sets_the_grain_rate` |
| `cloud` density 0 | no grains | same |
| density ramp 0 -> 1000 over 2 s, `regular` | 999 grains, 24.9 % in the first half (want 1000, 25 %) | same |
| same, `random` | 1013 grains, 24.8 % in the first half (Poisson sd 32) | same |
| `position` ramp 0 -> 1 over a 0 -> 1 ramp source, 50 grains/s | worst difference between a grain's first sample and the position 0.0000 | `cloud_position_and_pitch_spread` |
| `pitch_spread` 0 / 12 semitones, 4 grains of 1 kHz | 1000 Hz on all four / 1358, 1350, 1650, 1900 Hz (limits 500..2000) | same |
| `pitch` 2 | 2000 Hz | same |
| `pan_spread` 0.9 | stereo, L -13.60 dB, R -14.49 dB | `cloud_pan_spread_makes_stereo` |
| cloud (random and regular) and ranged `grains`, stereo, one thread vs the pool | same bits | `cloud_and_ranged_grains_are_bit_identical_and_seeded` |
| cloud seeds 1 / 2 / 1 again | `0x947ae61efb3bbd35` / `0x47c5cbb9be945d62` / `0x947ae61efb3bbd35` | same |
| sizes 1e-9..1e6, pitch 0 and 1e9, spread 120, jitter 5, density 48000, signals +-1e9 on every field, ranged grains | all finite | `cloud_and_ranged_grains_are_finite_at_the_extremes` |
| recipe above, 2 s | 96000 samples; rms first quarter -21.11 dB, last quarter -9.48 dB | `cloud_recipe_renders` |

### Honest limits

- A grain's size and pitch signals are read once, at its start sample; they
  do not change a grain while it plays.
- `cloud` is unnormalised: doubling density about doubles overlap and raises
  the level. Set `output.lufs` or add a `gain`.
- `random` mode can start two grains a few samples apart; with a short
  `size` they merge, so counting grains by their outlines under-counts. The
  test counts them by summed energy instead.
- `pitch_spread` is uniform in semitones, so it is symmetric on a log
  frequency scale; a `grains` `pitch` range with `curve: "linear"` is
  uniform in ratio and is not.
- A pitch ratio above 1 reads the source faster than it plays: a grain that
  reaches the end of the source stops there, so late positions at high pitch
  give short grains. No grain wraps around the source.
- None of this has been heard.
