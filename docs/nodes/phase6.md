# Phase 6: modal strike_glide and rattle; subdrop, riser

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

## Honest limits

None of this has been heard. `subdrop` and `riser` are synthetic layers with
fixed shapes (a sine; a two-pole band-pass and a sine); they are not
recordings of anything.
