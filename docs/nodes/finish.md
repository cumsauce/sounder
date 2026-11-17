# Phase 2: finish

## `finish`

The bus a designed sound ends with, as one node (dynamics / tone): punch, tail,
glue, warmth, weight and air shelves, and width above 150 Hz. It runs existing
nodes in order (`transient`, `transient`, `compress`, `shape` + `onepole`,
`eq`, `multiband` + `width`), each only when its field is set. It has no
reverb: SFX are dry by default (`docs/plans/phase0-results.md`).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | the mixed layers |
| `punch` | number | off | 0..1 | `transient` `attack_db` 8 x punch, `speed` 0.005 (fast enough for the first milliseconds of a hit); raises the crest |
| `tail_db` | number | off | dB, -12..12 | `transient` `sustain_db`, default speed: the falling part of every hit; negative tightens, positive lets it ring |
| `glue` | number | off | 0..1 | `compress`: threshold 4 + 16 x glue dB under the input's peak, ratio 1.5 + 2.5 x glue, instant attack, 120 ms release, 6 dB knee, no makeup; lowers the crest |
| `warmth` | number | off | 0..1 | `shape` `tube`, `oversample` 4, drive 1 + 2 x warmth, mix 0.4 x warmth, then a 20 Hz `onepole` hp for the tube's DC |
| `weight_db` | number | off | dB, -12..12 | low shelf at 120 Hz |
| `air_db` | number | off | dB, -12..12 | high shelf at 6 kHz |
| `width` | number | off | 0..2, stereo input only | `multiband` split at 150 Hz: the band below is summed to mono, the band above goes through `width` with this amount |

The fields are plain numbers (a macro maps them; they are not signals). A value
outside its range is an error naming the field and the range. `width` on a mono
input is an error: `finish` keeps a mono sound mono; make the mix stereo first
(a `mix` input's `pan`, `haas`, `decorrelate`) to widen it. A field at 0 (or 0
dB) is off: with every field off or left out, the output is the input bit for
bit.

### Recipe

The door slam's layers (AGENTS.md "Layering a designed sound") through
`finish` instead of the hand-built bus:

```json
[
  {"type": "finish", "name": "fin", "input": "dry", "punch": 0.4, "tail_db": -6, "glue": 0.3, "warmth": 0.6, "weight_db": 4},
  {"type": "output", "name": "out", "input": "fin", "lufs": -28}
]
```

Measured over MCP (Phase 2 check): STFT distance to the two scoring door
slams 11.28 and 7.36 dB, a mean of 9.32, against 9.23 for the hand-built bus
(`slam_b2`). Macros to give the human: `glue` (claim `crest`, -1: held on 4 of
4 seeds) and `punch` (claim `crest`, 1: held on 3 of 4 seeds of the noisy door
slam; on seed 2 it dipped 0.01 dB once, because the loudest sample of noise
layers is chance: leave the claim off on noisy layers), `tail_db`, `weight_db`.

### Measured

From `crates/sfx_analysis/tests/finish.rs` (`--nocapture`). The test source is
two pink-noise hits under a 1 ms / 250 ms `trigger_env`.

| What | Measured | Test |
|---|---|---|
| no field, or every field 0 | the input, bit for bit | `no_field_is_the_input_bit_for_bit` |
| crest at punch 0 / .5 / 1 | 22.08 / 23.71 / 24.71 dB | `punch_raises_the_crest_and_glue_lowers_it` |
| crest at glue 0 / .5 / 1 | 22.08 / 19.84 / 19.33 dB | same |
| `tail_db` -6: first 2 ms / 60-250 ms | 0.00 dB / -3.90 dB | `tail_db_turns_the_tail_down_and_leaves_the_attack` |
| 200 Hz sine 0.5, 2nd harmonic at warmth 0 / .5 / 1 | -120 / -41.9 / -34.5 dB; DC under -155 dB | `warmth_adds_harmonics_and_no_dc` |
| `weight_db` +6 at 30-60 Hz; `air_db` +6 at 12-16 kHz; both at 1 kHz | +5.89 / +5.92 / +0.01 dB | `weight_and_air_are_shelves` |
| decorrelated pink noise: side under 60 Hz vs mid | -40.1 dB (width 1), -34.1 dB (width 2) | `width_keeps_the_low_band_mono` |
| side 1-4 kHz at width 0 / 1 / 2 | -120 / -36.1 / -30.1 dB | same |
| same bits on one thread and many | yes | `renders_the_same_bits_on_any_thread_count` |

### Honest limits

- `tail_db` reaches about two thirds of its value on a noisy decay (-6 gave
  -3.9 dB): `transient`'s fast detector reads noise jitter as rising level.
- `punch` is causal with no lookahead: it boosts from the first millisecond
  the detector sees, so a hit whose peak is its very first sample gains less.
- `glue`'s threshold follows the input's peak, so it compresses the same
  amount whatever the input level; with an instant attack it flattens the
  first peak of each hit (which is how it lowers the crest).
- `width` leaves the low band at exactly mono only to the precision of the
  Linkwitz-Riley split (the side under 60 Hz sat 34-40 dB under the mid).
- Fixed corners: 120 Hz, 6 kHz, 150 Hz. Use `eq` / `multiband` for others.
- None of it has been heard.
