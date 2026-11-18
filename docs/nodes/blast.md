# Explosion, gunshot

Two finished-recipe models (like `thunder`) in `crates/sfx_graph/src/nodes/blast.rs`.
Both are offline (not causal), seeded (random), mono, and exactly the
document's length. Tests: `crates/sfx_analysis/tests/it/blast.rs`. Examples:
`examples/explosion.rs`, `examples/gunshot.rs` (they build with
`cargo check`; they were not run by the worker, the recipe test renders the
same nodes through an `output` with `lufs: -14`).

Shared structure (all numbers are constants in the file):

- a Friedlander pressure pulse `p(t) = (1 - t/t+) exp(-t/t+)` (zero net area);
- a sine thump whose pitch falls exponentially;
- a noise body: white noise through a three-pole low-pass whose cutoff falls
  with the envelope that fades the noise;
- a tail: dense, low-passed noise that decays more slowly;
- propagation: the blast starts `2.5 ms + distance / 343 s` in (the 2.5 ms
  lead keeps the output stage's default 2 ms fade-in off the attack), air
  absorption is two biquad low-pass poles at `16 kHz / (1 + d / 40 m)` (never
  under 120 Hz), the level falls as `1 / (1 + d / 100 m)`; at `distance` 0
  the filter is skipped;
- a final `tanh(0.7 x)`, so the output is always inside +-1.

## `explosion`

An explosion (source model): pressure pulse, falling sub thump, noise body,
long tail, delayed and darkened by distance, with optional debris ticks.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `size` | number | required | fireball radius, metres, 0.1 .. 100 | Pulse length `3 ms * size`; thump starts at `160 Hz * size^-0.35` (kept within 22 .. 160 Hz) and falls to 0.3 of that; body time constant `0.12 s * size^0.4`, body cutoff `5 kHz * size^-0.4`; tail time constant `0.3 s * size^0.5`, tail cutoff `500 Hz * size^-0.2`. |
| `distance` | number | required | metres, 0 .. 30000 | Delay `distance / 343` s, air low-pass and level loss as above. A document shorter than the delay renders silence. |
| `debris` | events | none | an event node (`pattern`, `poisson`, `burst`, ...) | A tick at each event: a 15 ms burst of noise (2.5 ms decay, first-difference high-passed) scaled by `0.3 * value`. The event time is seconds after the blast, and the ticks are delayed and darkened with it. |
| `seed` | integer | 0 | | Mixed with the document seed and the node name. Body, tail and debris use separate streams. |

Caps and errors (rule 7): `debris` over 20000 events errors; so do values out
of range:

- `size: 500: give metres of fireball radius from 0.1 to 100`
- `distance: -3: give metres from 0 to 30000`
- `debris: 50477 events: at most 20000; thin the stream (poisson rate, burst count)`
- `debris: \`ev\` is audio, not events: use on_threshold to make events from a signal`

`debris` is a real events port: it is an `In` read with `Cx::events`, so the
built-in-ticks fallback was not needed.

### Recipe

A boulder-field blast 150 m away: 60 pieces of debris, dense early. Give the
human macros on `size` and `distance`.

```json
[
  {"type": "burst", "name": "rocks", "count": 60, "spread": 2.0, "curve": "exp",
   "values": {"min": 0.2, "max": 1.0}, "seed": 2},
  {"type": "explosion", "name": "boom", "size": 8, "distance": 150,
   "debris": "rocks", "seed": 1},
  {"type": "output", "name": "out", "input": "boom", "lufs": -14}
]
```

### Measured

48 kHz, node output before the output stage, `size` 5 and `distance` 0 unless
stated. Centroid and decay are `sfx_analysis::measure` (decay is the time from
the last frame within 6 dB of the peak to the last within 40 dB, 10 ms frames).
Onset is the first sample at 1 % (-40 dB) of the peak; attack is 5 % to 90 % of
the peak.

| What | Measured | Test |
|---|---|---|
| `size` 1 / 5 / 25: centroid | 2124 / 1346 / 812 Hz | `explosion_size_moves_centroid_decay_and_sub_level` |
| same: decay | 1.16 / 2.68 / 5.92 s | same |
| same: level under 80 Hz | -80.3 / -61.5 / -45.1 dB | same |
| same: peak | 0.800 / 0.816 / 0.799 | same |
| `distance` 0 / 100 / 400: onset | 2.50 / 294.06 / 1168.79 ms (expected 2.50 / 294.05 / 1168.68) | `explosion_distance_delays_by_d_over_343_and_darkens` |
| same: centroid | 1346 / 604 / 378 Hz | same |
| same: peak | 0.816 / 0.455 / 0.187 | same |
| attack, `size` 0.5 / 5 / 50 | 0.125 / 0.146 / 1.292 ms (limit 2 ms) | `explosion_attack_is_under_2_ms` |
| debris ticks (events at 3.0, 3.6, 4.2 s, `size` 1) over the bare tail, 20 ms after each | +75 / +91 / +91 dB | `explosion_debris_adds_ticks_after_the_blast` |
| debris event at 0.5 s with `distance` 343 | first changed sample at 1.503 s (0.5 + 1 + 0.0025) | same |
| two seeds | 120 of 48000 samples equal | `explosion_is_bit_identical_and_seeded` |
| `size` 0.1 and 100, `distance` 0 and 30000 | finite, peaks 0.648 / 0.871 / 0 / 0 (30000 m arrives after 87 s) | `explosion_is_finite_at_the_extremes` |
| one thread vs the pool, with `poisson` debris | same bits | `explosion_is_bit_identical_and_seeded` |

### Honest limits

- The Friedlander pulse and the thump are physically shaped, but the
  constants (time constants, cutoffs) are tuned to make the stated fields move
  the stated measures, not fitted to recordings. Whether it is a good
  explosion is the human's call; none of this has been heard.
- The level does not depend on `size`: all sizes peak near 0.8 (the sub band
  and the length carry the size). Set the loudness with `output.lufs`.
- The attack grows with `size` (0.13 to 1.29 ms from 1 to 50 m) because the
  slower thump adds to the first milliseconds; at `size` 100 it was not
  measured and may pass 2 ms.
- Air absorption is a fixed two-pole low-pass, not a humidity or temperature
  model (use `air` for that). Distance does not model ground reflection.
- Debris ticks are all one kind of tick; only their time and level come from
  the stream.

## `gunshot`

A gunshot (source model): muzzle blast, plus an N-wave crack when the bullet
is supersonic.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `caliber` | number | required | bore, mm, 2 .. 60 | Larger is longer and lower: pulse `0.15 ms * caliber`, thump start `350 Hz * (caliber/9)^-0.6` (60 .. 700 Hz), tail `0.1 s * sqrt(caliber/9)`, body cutoff `9 kHz * (caliber/9)^-0.35`. |
| `barrel` | number | 0.4 | metres, 0.05 .. 1.5 | Muzzle velocity `300 + 600 * (1 - exp(-barrel/0.35))` m/s (380 at 5 cm, 449 at 10 cm, 709 at 40 cm, 866 at 1 m); a short barrel also makes a brighter, stronger blast body (`(0.4 / barrel)^0.3` on cutoff, `^0.45` on level). |
| `suppressed` | bool | false | | Subsonic ammunition behind a suppressor: velocity capped at 310 m/s (no crack), blast 28 dB down, pulse three times longer and weaker, body longer and darker, tail relatively heavier. |
| `distance` | number | 0 | metres, 0 .. 30000 | The blast arrives `distance / 343` s late. The crack arrives at `distance / (0.9 * muzzle velocity)`, so it leads the blast by the difference (135 ms at 100 m, caliber any, barrel 0.4). Both go through the same air low-pass and level loss. |
| `seed` | integer | 0 | | Mixed with the document seed and the node name. |

The crack exists when `0.9 * muzzle velocity > 343` m/s and not `suppressed`
(so not for `barrel` 0.05, 342 m/s). It is `+1` falling linearly to `-1` over
`0.12 ms * sqrt(caliber/9)` (at least 3 samples), amplitude `0.9 * clamp((Mach - 1) / 0.5, 0.4, 1)`,
through a 14 kHz pole.

Errors:

- `caliber: 1: give the bore in millimetres from 2 to 60 (9 pistol, 5.56 rifle)`
- `barrel: 3: give the barrel length in metres from 0.05 to 1.5`
- `distance: -1: give metres from 0 to 30000`

### Recipe

A 7.62 mm rifle heard from 120 m (`examples/gunshot.rs`): the crack lands about
0.1 s before the blast. Give the human macros on `distance` and `caliber`.

```json
[
  {"type": "gunshot", "name": "shot", "caliber": 7.62, "barrel": 0.5,
   "distance": 120, "seed": 1},
  {"type": "output", "name": "out", "input": "shot", "lufs": -14}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| `caliber` 5.56 / 12.7 / 40: centroid | 3618 / 3041 / 2221 Hz | `gunshot_caliber_moves_centroid_and_decay` |
| same: decay | 0.32 / 0.45 / 0.79 s | same |
| same: level under 80 Hz | -96 / -87 / -73 dB | same |
| same: attack | 0.000 / 0.021 / 0.021 ms | same |
| `suppressed`, `caliber` 5.56 / 9 / 12.7: peak | -1.9 to -34.2 / -1.9 to -33.9 / -1.8 to -33.5 dB (down 32.3 / 32.0 / 31.7 dB; limit 15) | `gunshot_suppressed_is_quieter_and_less_peaky` |
| same: crest | 33.2 to 26.6 / 32.0 to 25.9 / 31.2 to 25.5 dB | same |
| suppressed blast onset, `distance` 0 / 100 / 400 | 2.50 / 294.08 / 1168.79 ms (steps of d / 343 within 2 ms) | `gunshot_distance_delays_the_blast_and_leads_with_the_crack` |
| crack onset, `distance` 0 / 100 / 400 (barrel 0.4, 638 m/s mean) | 2.50 / 159.31 / 629.75 ms; leads the blast by 134.8 / 539.0 ms | same |
| centroid, `distance` 0 / 100 / 400 | 3286 / 1420 / 701 Hz | same |
| crack onset at 300 m, `barrel` 0.1 / 0.4 / 1.0 | 744.79 / 472.94 / 387.69 ms (formula: 744.70 / 472.87 / 387.62) | `gunshot_barrel_sets_the_crack_timing_and_blast_brightness` |
| blast body, share above 2 kHz in the first 20 ms, 100 m, `barrel` 0.1 / 0.4 / 1.0 | -7.6 / -9.8 / -10.9 dB | same |
| 2 / 60 mm x barrel 0.05 / 1.5 x suppressed x distance 0 / 30000 | all finite, peak at most 1 | `gunshot_is_bit_identical_and_finite_at_the_extremes` |
| one thread vs the pool | same bits | same |

### Honest limits

- The open shot is saturated by the final `tanh`: all open calibers peak near
  0.80 (-1.9 dB), so "peak down" for `suppressed` is measured against a
  clipped peak. With the crack present the crest is high anyway (31 to 33 dB
  over 2 s of mostly decay).
- The barrel only changes the blast body slightly (cutoff, level) and the
  crack timing; the first is a 3.3 dB effect over that range (table above). The centroid of
  a 270 ms window around the blast moved by under 1 % over the same barrel
  range, so there is no centroid claim for `barrel`.
- Level does not depend on caliber (see above); sub bass (under 80 Hz) is
  very low for every caliber, as the thump starts at 60 Hz or higher.
- The crack is one clean N-wave at the geometry delay. Real shots have a
  Mach-angle dependence on where the listener stands; the delay here assumes
  the listener is on the line of fire, and there is no off-axis variation or
  echo.
- None of this has been heard.

## Suppressed gunshot and distance darkening

Batch 2 labels: suppressed gunshots heard as "hitting a metal sheet" and
"lo-fi slam, no high end or detail". Causes found in `blast.rs`: the
suppressed path kept the falling sine thump (350 Hz at 9 mm) while its noise
was cut by 28 dB and darkened (body 0.3x, tail 1 kHz), so a pitched tone
dominated; and the air low-pass `16 kHz / (1 + d / 40 m)` was 7.1 kHz at
50 m and 4.0 kHz at 120 m (12 dB/oct).

Changes:
- `suppressed: true` is a new model: no sine thump; a gas puff of white
  noise band-passed 200 Hz - 4 kHz (`4 kHz * (caliber/9)^-0.2`), 0.4 ms
  attack, 18 ms * sqrt(caliber/9) decay; the action: two first-differenced
  noise clicks (0.5 ms decay, an echo 1-2 ms later at 0.4), the clack at
  5-8 ms and the return at 35-55 ms (seeded). Level unchanged (gain 0.04).
- `gunshot` air low-pass scale 40 m -> 400 m (14.2 kHz at 50 m, 12.3 kHz at
  120 m, 4.4 kHz at 1 km). `explosion` keeps 40 m.

Measured (`tests/it/blast.rs`, 9 mm, seed 1, shares against the whole):

| | before | after |
|---|---|---|
| suppressed centroid | 1759 Hz | 6578 Hz |
| suppressed flatness | 0.127 | 0.543 |
| under 150 Hz | -7.0 dB | -21.3 dB |
| 4-12 kHz | -13.3 dB | -4.8 dB |
| 8-12 kHz | -19.0 dB | -9.5 dB |
| suppressed 4-12 kHz at 50 / 120 m | -15.8 / (lower) dB | -4.4 / -4.5 dB |
| open 4-12 kHz at 50 / 120 m | -11.9 / -20.2 dB | -8.7 / -8.9 dB |

Suppressed peak still 32 dB under the open shot, crest 30.2 dB (open 32.0).
`tests/it/blast.rs`: the open-shot centroid step 100 -> 400 m is now 0.81x, so
that assertion went from 0.8 to 0.85. Not heard: the centroid at 0 m is high
(the clicks carry much of the energy); if it reads as clicks over a puff,
lower the click levels (0.15 / 0.1) in `suppressed_dry`.
