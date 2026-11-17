# Phase 6: modal strike_glide and rattle; subdrop, riser

**Listening result (the human, 2026-11-17).**
- `strike_glide`: "adds more detail and does sound like a harder hit"; the
  knob settled at 0.22 of 0..160 cents, so about **35 cents**. But neither lid
  sounds like a metal lid: a real one is far more inharmonic and atonal than
  a `modal` membrane struck by an impulse.
- `rattle`: the plain crate is better ("woodier"); with `rattle` 0.6 it has
  more texture but reads as **plastic**. Both crates read as a marimba rather
  than wood, because they are tonal; the plain one passes for a small plank
  of wood falling on hard ground.
- `riser` / `subdrop`: not what was meant by "cinematic" (that meant
  production-quality foley for films and games, not trailer exaggeration),
  and the hit "sounds bad", though a real one would need effects like reverb.
- The recurring lesson (the door slam too): `modal` struck cleanly is too
  tonal for real objects. Wood is heavily damped and noisy; thin metal has
  dense, clashing partials.

Exaggeration and liveliness for designed hits (`docs/plans/sfx-quality.md`
Phase 6). None of it has been heard; every number is from
`crates/sfx_analysis/tests/phase6.rs` (`--nocapture`).

## `modal` `strike_glide` and `rattle`

Two fields on `modal`, off by default: left out or 0, the samples are
unchanged bit for bit (`modal_without_the_new_fields_is_unchanged`).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `strike_glide` | number | off | cents, 0..1200 | every mode starts this far sharp at the ring's loudest and settles with the level (a second pass of per-sample retuned resonators following the first pass's envelope: instant attack, 30 ms release) |
| `rattle` | number | off | 0..1 | the part of the ring above a gap is added back high-passed above 1.5 kHz (12 dB/oct, gain 8); the gap is the ring's level (its loudest 5 ms RMS as a sine peak) at 0, 30 dB under it at 1, so the buzz lasts longer as `rattle` rises and stops when the ring falls under the gap |

### Recipe

A dustbin lid struck hard, with a loose handle:

```json
[
  {"type": "impulse", "name": "hit", "at": 0.01},
  {"type": "modal", "name": "lid", "excite": "hit", "material": "metal", "shape": "membrane", "f0": 300, "loss": 0.5, "strike_glide": 60, "rattle": 0.5}
]
```

Macros: `rattle` (no claim), `strike_glide` (no claim; a pitch knob).

### Measured

| What | Measured | Test |
|---|---|---|
| metal bar f0 400, `strike_glide` 100: pitch against plain `modal` | +45.5 cents at 20-110 ms, +2.1 cents at 1.0-1.5 s | `strike_glide_starts_sharp_and_settles` |
| wood f0 200 loss 0.6, `rattle` 0.5: 2-12 kHz in the first 100 ms | +14.7 dB, the buzz 14.8 dB under the body | `rattle_buzzes_while_loud_and_stops` |
| same, 0.2-0.5 s (the ring under the gap) | differs from plain by -65.8 dB re its level | same |

### Honest limits

- The glide follows the level against the very first peak; a struck object's
  first milliseconds are far louder than its ring, so the full glide lasts
  only an instant (100 cents averaged 45.5 over 20-110 ms).
- The glide is the same for every mode (a real plate's modes glide by
  different amounts); the retuned resonators are not renormalized.
- On a bright body (a metal bar) the buzz barely shows above what is already
  there (+3.6 dB at an earlier, quieter gain): a rattle reads on dull bodies.
  At gain 2 the buzz sat about 30 dB under a wood body; the gain is 8.
- `rattle` is a hard-knee distortion of the ring, not a modelled second body.

## `subdrop`

A sine sweeping down under a hit, with its own envelope: a cinematic layer,
tonal on purpose.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `from` | number | 90 | Hz, 20..400 | starting pitch |
| `to` | number | 32 | Hz, 15..400 | final pitch (held after `time`) |
| `time` | number | 0.6 | s, 0.01..10 | the exponential sweep's length |
| `attack` | number | 0.004 | s, 0..1 | raised-cosine rise |
| `decay` | number | 1.2 | s, 0.01..30 | 60 dB fall after the attack |
| `at` | number | 0 | s | start |
| `drive` | number | 0 | 0..1 | tanh saturation (2nd / 3rd harmonics, so a speaker with no sub hears it), peak kept |
| `amp` | number | 0.8 | linear, 0..4 | peak level |

### Measured

| What | Measured | Test |
|---|---|---|
| 90 -> 32 Hz over 0.6 s: pitch | 86.3 Hz in the first 50 ms, 32.0 Hz at 1.0-1.5 s | `subdrop_sweeps_from_to_and_drive_adds_harmonics` |
| 3rd harmonic (96 Hz) at drive 0 / 0.8 | -120 / -40.1 dB | same |

## `riser`

A swell of band-passed noise and a rising tone that peaks at `at` (the hit)
and stops: an anticipation layer. Seeded, not causal (it is placed against a
future moment).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `at` | number | required | s, at least `length` | the hit: the swell peaks here |
| `length` | number | 1.5 | s, 0.05..20 | how long before `at` it starts |
| `from` / `to` | number | 200 / 2000 | Hz, 20..8000 / 20..12000 | the tone's and the noise band's pitch at the start and at `at` (exponential) |
| `noise` | number | 0.6 | 0..1 | noise against tone |
| `shape` | number | 2 | 0.5..8 | level goes as (time / length)^shape: higher holds back longer |
| `release` | number | 0.01 | s, 0..2 | fade after `at` |
| `amp` | number | 0.8 | linear | peak level |
| `seed` | integer | 0 | | the noise |

### Measured

| What | Measured | Test |
|---|---|---|
| `at` 1.5, `length` 1.4: rms at 0.2-0.5 / 0.8-1.1 / 1.35-1.5 s | -40.3 / -19.9 / -11.2 dB | `riser_swells_to_the_hit_and_stops` |
| after the hit (1.55-2 s) | -120 dB | same |

## A cinematic hit, and the `layers` check

A riser into a noise crack and a wood body with `rattle` 0.4, with a
`subdrop` under it, read voiced 0.58 (pitch 56 Hz), energy 49 / 6 / 32 / 14 %
(weight / body / crack / air) (`a_cinematic_hit_is_voiced_where_a_realistic_one_is_not`).
The `layers` `impact` preset is for realistic hits (voiced at most 0.3, body
30-80 %) and fails this one on purpose: for a hit the brief wants larger
than life, give `layers` its own `voiced_max` and `body`, or check it
against a reference set of cinematic hits instead.

## `modal` `noise`: less tonal wood and metal

After the listening note above, a third field, off by default (left out or
0: unchanged bit for bit, `defaults_and_zero_are_unchanged`):

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `noise` | number | off | 0..1 | each mode becomes (1 - noise) x its clean ring + noise x a band of white noise at its frequency, following the ring's envelope and decay (round 1-2 used a modulation instead; see below) |
| `noise_width` | number | 0.25 | 0.05..2 | the noise band's width as a fraction of the mode's frequency |

Measured on a noise-burst strike (`crates/sfx_analysis/tests/tonality.rs`):

| Sound | voiced | occupied flatness | strongest partial over the median |
|---|---|---|---|
| wood plank f0 180, loss 1.2 | 0.89 | 0.21 | 13.8 dB |
| + noise 0.6 | 0.24 | 0.45 | 14.8 dB |
| + noise 0.6, tilt 4, loss 3 (`p7_plank_new`) | 0.17 | 0.26 | 10.1 dB |
| metal lid (membrane f0 300, 128 modes) | 0.92 | 0.08 | 12.3 dB |
| + noise 0.6 | 0.01 | 0.42 | 12.7 dB |
| + noise 0.5, tilt 4 (`p7_lid_new`) | 0.00 | 0.51 | 6.3 dB |

`noise` takes the pitch out; `tilt` with it takes down the one dominant
partial (the "tuned gong"). A tried `irregular` field (seeded scatter of
mode frequencies and levels) moved none of these measures, so it was
dropped: the tuned quality is clean partials, not their spacing. Listening
pairs: `p7_plank_old` / `p7_plank_new`, `p7_lid_old` / `p7_lid_new`
(-36 LUFS; the noisy versions are spikier).

Listened (the human, 2026-11-17): both "in the right direction"; the plank
"a bit too short / not resonant enough", the lid "still slightly too
tonal". Round 2, measured over MCP on the listening sounds:

| Sound | Was | Now | Measured now |
|---|---|---|---|
| `p7_plank_new` | loss 3 (-60 dB at 140 ms) | loss 1.8, noise 0.6, tilt 4 | -60 dB at 210 ms, voiced 0.00, occupied flatness 0.40 |
| `p7_lid_new` (impulse strike, 128 modes, glide 35) | noise 0.5, tilt 4: voiced 0.29 | noise 0.75, tilt 6 | voiced 0.00, 73 % of the energy at 2-8 kHz, occupied flatness 0.73 |

The lid's lowest mode (300 Hz) still rings longest (3.7 s, at -45 dB).

Round 2 listened: the lid "still too tonal". Two causes, both fixed:

- **A bug: `strike_glide` dropped `noise`.** The glide rebuilt the output
  from the clean modes after the noise was mixed in, so every lid heard so
  far (all with `strike_glide` 35) had no noise at all; only `tilt` changed
  between rounds. Glide and noise are now computed per mode together
  (`glide_keeps_the_noise`; the glide alone is unchanged, +45.5 cents).
- **`noise` modulated each ring**, `(1 - noise) + noise x low-passed noise`:
  at 0.75 a quarter of every partial stayed a clean line, and the smeared
  part sat beside the line. Now each mode is `(1 - noise) x its ring +
  noise x a band of white noise at its frequency` (`noise_width` x f wide,
  following the ring's envelope and decay): at 1 no line is left. New field
  `noise_width` (default 0.25, 0.05..2).

Lid with `strike_glide` 35, `tilt` 6: flatness 0.40 and voiced 0.48 without
noise; 0.74 and 0.00 with `noise` 1, `noise_width` 1. Round 3 in the window:
`p7_lid_new` at `noise` 0.85, `noise_width` 0.5 (knobs `grain`, `width`);
`p7_plank_new` unchanged settings, now with real noise bands.

**Round 3 listened: "much better"** (no knobs moved). Approved starting
points: wood `noise` 0.6, `tilt` 4, `loss` 1.8; thin metal `noise` 0.85,
`noise_width` 0.5, `tilt` 6 (the lid measured voiced 0.00, occupied
flatness 0.79, 68 % of its energy at 2-8 kHz).

## `click` `noise` and `footstep` `noise`

An audit of the ready-made struck nodes at their defaults (voiced share of
the pitch tracker): `click` 0.33-0.78 for every kind (the latch was heard as
"a chime"), `footstep` wood 0.84 at 158 Hz; concrete, gravel, grass,
`explosion` and `gunshot` 0.00; `splash` 0.50 and `modal` glass 1.00 are
pitched by nature and were left. `click` and `footstep` (wood only) now take
the same `noise` (0..1, off by default, bit-identical at 0), through the
helper `modal` uses (`add_noise_band`, width 0.25).

| Sound | voiced plain / `noise` 0.6 | occupied flatness |
|---|---|---|
| `click` latch, switch, button, lock, clasp | 0.60, 0.33, 0.60, 0.78, 0.60 / 0.00 each | 0.21-0.41 / 0.51-0.65 |
| `footstep` wood | 0.84 / 0.00 | 0.15 / 0.36 (strongest partial 19.4 to 8.6 dB over the median) |

(`click_and_wood_footstep_take_noise` in `tests/tonality.rs`.) Listening
pairs `p8_latch_old` / `p8_latch_new`, `p8_step_wood_old` /
`p8_step_wood_new` (peak -6 dBFS; the noisy ones are about 1.5 LU quieter
at the same peak).

Listened (the human): the latch at `noise` 0.6 "sounds much better but not
really like a latch... like the clack from a small plastic object": short
noise bands at 1.5-8 kHz are a small plastic part, and a latch is metal, so
round 2 is `noise` 0.3 (some of the metal ring kept). The wood step at 0.6
"a bit better but feels a bit forceful, like something was slammed or
dropped. A footstep has parts to it; boards could creak". `footstep` gained
two wood-only fields, off by default (bit-identical at 0):

| Field | What it does | Measured (`wood_footstep_scuff_and_creak`) |
|---|---|---|
| `scuff` 0..1 | the sole scuffing between heel and toe: noise around 2.5 kHz swelling and fading across the roll | 0.8: +7.9 dB between heel and toe |
| `creak` 0..1 | a board under the weight: a jittered stick-slip train (70-130 a second) from 30-60 ms after the heel for 0.15-0.3 s through three board resonances | 0.8: +6.7 dB at 0.15-0.4 s |

Round 2 `p8_step_wood_new`: `weight` 0.4, `noise` 0.35, `scuff` 0.6,
`creak` 0.5 (knobs `grain`, `scuff`, `creak`).

Round 2 listened: the step note was misread: "forceful, like slammed" was a
complaint about the **original**, and round 2 was "even more energetic".
The latch: the listener set `noise` to **0.75** ("a plausible real life
object of small plastic or small metal"). Measured on the steps (peak -6
dBFS):

| Step | crest | low (<200 Hz) share | -60 dB | voiced |
|---|---|---|---|---|
| original wood step | 24.0 dB | 84 % | 290 ms | 0.84 |
| round 2 (scuff, creak) | 24.2 dB | 42 % | 170 ms | 0.00 |
| round 3 `p8_step_wood_soft` | 28.2 dB | 43 % | 200 ms | 0.29 |

The original's "slam" is the board: 84 % of the energy under 200 Hz for
290 ms. `footstep` gained `ring` (wood, 0.25..2, default 1, bit-identical
when left out): a multiplier on the board's body decays. Round 3: `weight`
0.25, `ring` 0.6, `noise` 0.35, `scuff` 0.2, no creak (knobs `ring`,
`weight`, `scuff`). `ring` 0.4 rang 80 ms but let the board's small tick
dominate (attack centroid 1.7 kHz); the soft shoe is already the default.

## Honest limits

None of this has been heard. `subdrop` and `riser` are synthetic layers with
fixed shapes (a sine; a two-pole band-pass and a sine); they are not
recordings of anything.
