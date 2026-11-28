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

A gunshot: Farnell's open shot (fig. 53.9: a cartridge chirp, a barrel wave
and a noise burst through an 8-band weapon body), a low punch, the action, an
N-wave crack when the bullet is supersonic, and the report of the space it is
fired in. One round, or one per event (automatic fire). The shared structure
at the top of this file (a Friedlander pulse, a sine thump, a noise body)
describes `explosion`; the open shot is the Farnell model below.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `weapon` | word | none | `pistol revolver smg carbine rifle sniper shotgun lmg` | Defaults for caliber, barrel, action and punch (table below); explicit fields win. Give this or `caliber`. |
| `caliber` | number | the weapon's | bore, mm, 2 .. 60 | Larger is longer and lower: chirp, barrel wave and noise burst last `(caliber/9)^0.3` times 93, 20 and 115 ms; the body bands are divided by the same factor. |
| `barrel` | number | 0.4 or the weapon's | metres, 0.05 .. 1.5 | Muzzle velocity `300 + 600 * (1 - exp(-barrel/0.35))` m/s (380 at 5 cm, 709 at 40 cm); the barrel wave and noise burst start `12 ms * barrel / 0.4` after the chirp; a short barrel drives the body harder. |
| `suppressed` | bool | false | | Subsonic ammunition behind a suppressor: no crack, no punch, no `action`; a gas puff, a noise thump and its own action clacks ("Suppressed gunshot and distance darkening" below). |
| `punch` | number | 0.5 or the weapon's | 0 .. 1 | The low end: one Friedlander pressure swing (positive phase `2.5 ms * sqrt(caliber/9)`, a single cycle, so atonal) plus noise band-limited to 50-250 Hz (1 ms attack, `30 ms * sqrt(caliber/9)` decay). It does not feed the report. |
| `drive` | number | 0.7 | 0 .. 1 | Soft saturation of the close layer, keeping its peak (`tanh(g x / p) / tanh(g) * p`, g = 5 x drive): recorded shots overload the microphone, so their crest is low. |
| `action` | word | none or the weapon's | `none semi` | `semi`: the slide or bolt carrier, a clack 5-8 ms in and its return 35-55 ms in, about 20 dB under the blast. |
| `space` | word | none | `none open forest urban canyon room hall hangar` | The report tail: the close shot and the crack convolved with a response built for the space (table below). |
| `space_size` | number | 1 | 0.25 .. 4 | Scales the echo delays, and the tail length as `size^0.7`. |
| `tail_db` | number | 0 | dB, -40 .. 20 | The tail level against the space's own. |
| `layer` | word | all | `all close crack mech tail` | One layer alone; the layers add back to `all` (before the final soft clip). A game exports `close` dry and one `tail` per space. |
| `events` | events | none | an event node, at most 2000 | One round per event (a `pattern` `every: 60 / rpm`), scaled by the value. Each round's body is detuned +-1.5 % and its level +-0.5 dB (seeded), and the body sinks up to 4 % as the barrel heats (0.2 % a round). The report rings under every round. |
| `distance` | number | 0 | metres, 0 .. 30000 | The blast arrives `distance / 343` s late; the crack at `distance / (0.9 * muzzle velocity)` (at least 44 ms after the trigger, as in guns1.pd), so it leads the blast far away; air low-pass `16 kHz / (1 + d / 400 m)`, level `1 / (1 + d / 100 m)`; the tail falls only as the square root of that. |
| `seed` | integer | 0 | | Mixed with the document seed and the node name; each seed also moves the eight body bands by up to 3 %. |

Weapons:

| `weapon` | caliber | barrel | action | punch | note |
|---|---|---|---|---|---|
| `pistol` | 9 | 0.1 | semi | 0.45 | |
| `revolver` | 9.1 | 0.15 | none | 0.6 | .357 |
| `smg` | 9 | 0.2 | semi | 0.4 | |
| `carbine` | 5.56 | 0.37 | semi | 0.5 | |
| `rifle` | 7.62 | 0.5 | semi | 0.6 | |
| `sniper` | 8.6 | 0.7 | none | 0.8 | .338 bolt gun |
| `shotgun` | 18.5 | 0.5 | none | 1.0 | 12 gauge; the shot is capped at 420 m/s, so the crack is weak (Mach 1.1) |
| `lmg` | 7.62 | 0.55 | semi | 0.7 | |

Spaces (the response: noise that builds over `build`, falls 60 dB over `t60`,
low-passed from `hf0` to `hf1` over the first half of `t60`, high-passed at
150 Hz, plus discrete slapbacks; scaled to unit energy times the level):

| `space` | t60 s | predelay ms | hf0 -> hf1 Hz | level dB | slapbacks s (gain) |
|---|---|---|---|---|---|
| `open` | 1.8 | 20 | 14000 -> 1000 | -4 | |
| `forest` | 1.4 | 8 | 16000 -> 1500 | +1 | |
| `urban` | 1.6 | 12 | 6000 -> 600 | -2 | 0.035 (0.5), 0.07 (0.35), 0.11 (0.3), 0.16 (0.25) |
| `canyon` | 3.5 | 50 | 4000 -> 250 | -3 | 0.4 (0.5), 0.9 (0.35), 1.6 (0.25) |
| `room` | 0.5 | 2 | 9000 -> 2500 | 0 | |
| `hall` | 1.8 | 10 | 7000 -> 1500 | -1 | |
| `hangar` | 3.0 | 20 | 5000 -> 1000 | 0 | 0.06 (0.3), 0.12 (0.2) |

`camera` with `space: outdoor` was tried on a dry pistol first: its discrete
slaps came out louder than the shot and the audio-text model heard "a digital
glitch". A gun's report is a dense, darkening field, hence its own tail.

Errors: `caliber: give the bore in millimetres ... or a weapon (pistol,
rifle, shotgun, ...)` when both are missing; `punch: 2: give 0 to 1`, `drive`,
`space_size: 9: give 0.25 to 4`, `tail_db: 50: give -40 to 20 dB`,
`events: N rounds: at most 2000`, and the earlier caliber / barrel / distance
errors.

### Recipe

A pistol in a street, for a video (the tail baked in), and a game's dry shot
plus its tail as two sounds (`layer`):

```json
[
  {"type": "gunshot", "name": "shot", "weapon": "pistol", "space": "urban", "seed": 1},
  {"type": "output", "name": "out", "input": "shot", "peak_db": -1, "fade_in": 0.0005}
]
```

Automatic fire at 900 rounds a minute, eight rounds, in the open:

```json
[
  {"type": "pattern", "name": "rof", "every": 0.0667, "count": 8, "jitter": 0.002, "seed": 2},
  {"type": "gunshot", "name": "burst", "weapon": "smg", "events": "rof", "space": "open", "seed": 1},
  {"type": "output", "name": "out", "input": "burst", "peak_db": -1, "fade_in": 0.0005}
]
```

Give the document a `duration` that holds the report (1.5 s open, 2 s
urban, 4 s canyon). Knobs: `punch` (claim crest is unreliable with `drive`;
leave unclaimed), `space_size` (claim `decay`, 1), `tail_db`, `distance`.
A rifle heard from 120 m: `{"weapon": "rifle", "distance": 120, "space":
"open"}`; the crack lands about 0.1 s before the blast.

### Measured

These rows were measured before the weapon kit (no punch, no drive, no band
jitter). The tests still hold their bounds with the new defaults; the kit's
own numbers are in "Against recordings" below.

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

- All open calibers still peak near the same level (`drive` keeps the
  peak, and a final `tanh(0.7 x)` keeps the output inside +-1): set the level
  with `peak_db`. The caliber is heard in the length, the punch and the body
  bands, not the level.
- The barrel only changes the blast body slightly (cutoff, level) and the
  crack timing; the first is a 3.3 dB effect over that range (table above). The centroid of
  a 270 ms window around the blast moved by under 1 % over the same barrel
  range, so there is no centroid claim for `barrel`.
- The punch reaches 60-250 Hz; there is no separate sub drop (a sine under
  60 Hz read as a tuned drum on impacts). Add `subdrop` only when the brief
  asks for a cinematic shot.
- The crack is one clean N-wave at the geometry delay. Real shots have a
  Mach-angle dependence on where the listener stands; the delay here assumes
  the listener is on the line of fire, and there is no off-axis variation.
  The crack's echoes are only in the report.
- The report is a built response, not a measured space; whether each space
  reads as itself has not been heard.
- Only the suppressed shot has been heard (three rounds); the open shot and
  the kit have not.

## Against recordings (the weapon kit)

`listen {reference_set: refs/gun_set}` (four CC0 outdoor shots, cut; their
own spread 13.6 dB), 9 mm unless stated, seed 1, `peak_db` -1:

| Shot | distance to the 4 recordings, dB | out of range | heard_as (top 3) |
|---|---|---|---|
| before: `caliber` 9 `barrel` 0.1, dry | 29.7 / 23.5 / 22.7 / 18.1 | crest 23.8 (15.3-18.1), tail 30 ms (110-1350) | a kick drum, a digital glitch, hitting a stone |
| before, through `camera` outdoor at 20 m | 22.2 / 17.9 / 17.2 / 14.3 | body 79 %, crest 23.4 | a digital glitch, an error sound, a clock ticking |
| `pistol`, dry | 28.1 / 20.6 / 19.9 / 14.6 | crest 21.8, tail 70 ms | a digital glitch, a kick drum, a snare drum |
| `pistol` `open` | 15.2 / 7.2 / 7.4 / 11.4 | none | a snare drum, a gunshot, hitting a metal object |
| `rifle` `open` | 15.6 / 7.4 / 7.3 / 9.8 | none | a snare drum, a gunshot, a kick drum |
| `shotgun` `open` | 18.8 / 9.4 / 8.8 / 9.7 | crack 10.9 % (11.3-29.6, in tolerance) | a gunshot, a snare drum, hitting a piece of wood |
| `pistol` `urban` | 16.1 / 7.4 / 7.3 / 10.4 | none | a snare drum, hitting a metal object, a gunshot |

What moved the distance: the report (dry 14-28 -> 7-15 dB), keeping the punch
out of the report (the recordings' tail is mid-band; a low end rolling through
it was the biggest red area in the difference image), and `drive` (crest
23.7 / 19.6 / 17.4 dB at drive 0 / 0.5 / 1). The Farnell body's Q 4.56 stays:
`voiced` 0 and at most 2 ringing partials, inside the recordings.

Tests (`crates/sfx_analysis/tests/it/blast.rs`, `mod kit`): every weapon finite,
the shotgun's centroid under 0.85 x the pistol's (1176 / 1428 Hz); punch 0 -> 1
raises the share under 200 Hz by 4 dB (-9.1 -> -5.1); drive lowers the crest;
`open` adds a report 0.3-0.6 s in (-120 -> -34 dB) whose 4-16 kHz share falls
from -12.2 dB (50-200 ms) to -19.9 dB (0.6-1 s); the four layers sum to `all`
at 400 m; six events give six rounds, and two rounds are not copies.

Not heard yet: whether the open shot reads as a gun, which spaces fit, and
whether the automatic fire reads as one gun. The audio-text model's "snare
drum" is a hint only.

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
