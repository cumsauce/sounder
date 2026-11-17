# R4-D: `splash` tail / burst, `thunder` tail, `water` tone

Three new optional fields. Each default leaves the render bit-identical to
before (`splash_defaults_are_bit_identical`, `thunder_tail_off_is_bit_identical`,
`water_tone_default_is_bit_identical` in `crates/sfx_analysis/tests/r4d.rs`; the
older hash-pinned tests are untouched). Tests: `crates/sfx_analysis/tests/r4d.rs`.

## `splash`: `tail`, `burst`

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `tail` | number | 1 | 0..3 | times the length of the bubble cloud after the burst and times its bubble count (the main cavity bubble ends the cloud, so it comes later); 0 is no cloud; error outside the range |
| `burst` | number | 1 | 0..2 | level of the opening burst against the main bubble; 2 makes the burst the loudest moment, 0 drops it; the splash is then levelled to its usual peak (0.5), so a louder burst quiets what follows; error outside the range |

A splash never rings past 2.5 s (the buffer cap), so a big `size` with `tail` 3 is cut there.

```json
[
  {"type": "splash", "name": "s", "size": 0.15, "tail": 2, "burst": 2, "seed": 3},
  {"type": "output", "name": "out", "input": "s", "peak_db": -3}
]
```

Macros worth giving: `tail` and `burst` on the node (no claim measured: the
sweeps of claims such as `decay` or `crest` were not run on this node).

Measured (`splash_tail_and_burst_reshape_it`, 0.15 m, seed 3, 3 s document):

- default: peak at 284 ms, 2 fine onsets after 0.2 s.
- `tail` 2, `burst` 2: peak at 2.8 ms, 12 fine onsets after 0.2 s.
- `splash_tail_lengthens_the_cloud_monotonically`: last fine onset at 0.14 / 0.27 / 0.54 s for `tail` 0.5 / 1 / 2.
- `splash_burst_moves_the_peak_into_the_opening`: peak in the first 20 ms is 0.135 / 0.461 / 0.500 for `burst` 0 / 1 / 2 (peak after 20 ms: 0.500 / 0.500 / 0.271).

## `thunder`: `tail`

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `tail` | number | 0 | 0..1 | an exponential fade of the whole output: from the first sound the gain falls in dB, linearly, reaching `-40 * tail` dB at the end of the audible tail (the last sample over -60 dB of the peak, measured after `length` and `rumble`), then carries on at the same slope; 0 is off; error outside the range |

It runs after `length` and `rumble` and before `exposure`, `brightness` and
`amp`. The fade lowers the peak too (the loudest part of a 1.5 km flash is in
the rumble, not at the clap), so re-level with `amp` or the output's
`lufs` / `peak_db`.

```json
[
  {"type": "thunder", "name": "t", "distance": 1500, "rumble": 0.4, "tail": 0.8, "seed": 3},
  {"type": "output", "name": "out", "input": "t", "peak_db": -3}
]
```

with `duration` 16. Macro: `tail` on the node, claim `decay`.

Measured (`thunder_tail_shortens_the_decay_monotonically`, 1500 m,
seed 3, 16 s; "audible" is the 10 ms RMS last over -45 dBFS, counted from
the first sound at 2.76 s):

| `tail` | 0 | 0.25 | 0.5 | 0.75 | 1 |
|---|---|---|---|---|---|
| audible, s | 7.19 | 6.54 | 6.53 | 4.80 | 4.80 |
| peak, dBFS | -10.65 | -13.74 | -15.98 | -18.21 | -20.45 |

The audible length is non-increasing, not strictly: foley's tail is a string of
spikes, so the end moves in steps. `listen`'s `decay -20 dB` is NOT monotone from
0 (1.84 s) to 0.25 (4.93 s): it counts from the loudest window, and with no fade
that window is in the rumble, with a fade it moves to the clap. From 0.25 up it
falls (4.93 / 4.82 / 3.20 / 3.16 s). Read the audible length, or the loudness
curve in `listen`'s picture, when tuning a tail.

## `water`: `tone`

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `tone` | number | 0.5 | 0..1 | a high shelf at 2 kHz of `(tone - 0.5) * 24` dB (so +-12 dB) over everything the node makes: hiss, bubble pops, splashes; error outside the range |

The shelf is part of the render. In a loop it is primed with the last 0.25 s of
the (periodic) buffer, so the node's output stays periodic: put the knob here,
not a `filter` after the node (which breaks the seam). The level is not
re-normalised: `tone` 1 is brighter and louder.

```json
[
  {"type": "water", "name": "w", "flow": 0.5, "tone": 0.8, "seed": 3},
  {"type": "output", "name": "out", "input": "w", "lufs": -20}
]
```

as a `kind: loop` with `duration` 3. Macro: `tone` (claim `centroid`, 1).

Measured (flow 0.5, seed 3, 4 s):

- `water_tone_raises_the_band_level_by_six_db`: `tone` 1 against 0.5 moves 2-20 kHz by +9.5 dB and 50-500 Hz by +0.0 dB.
- `water_tone_tilts_the_spectrum_above_2k`: the share of power above 2 kHz is 0.059 / 0.234 / 0.609 at `tone` 0 / 0.5 / 1: +4.2 dB from 0.5 to 1 (the share saturates: it cannot gain 6 dB from 0.234), +10.1 dB from 0 to 1, -6.0 dB from 0.5 to 0.
- `water_tone_keeps_loop_seam_at_seeds_1_to_4`: 3 s `kind: loop`, `lufs` -20, `loop_seam` PASS for `tone` 0 and 1 at seeds 1-4 (wrap discontinuity 0.27-0.66x, spectral change 0.8-2.1 dB).

Limits: the shelf is a biquad, not a model of a brighter bubble population: it
cannot add content the node does not have, and the pops sit mostly at 0.8-4 kHz
(their upper part is what it lifts). Not heard.
