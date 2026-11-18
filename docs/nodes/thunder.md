# `thunder`: `rumble`, `tail`

Files: `crates/sfx_graph/src/nodes/foley.rs` (`Thunder`),
`crates/sfx_analysis/tests/it/foley.rs`.

| Node | New field | Type | Default | What it does |
|---|---|---|---|---|
| `thunder` | `rumble` | number, 0 .. 1 | `0` (off) | Adds a continuous bed of seeded noise band-limited to 25-90 Hz, from the first sound to the end of the audible tail (last sample over -60 dB of the peak; 0.25 s fade out, 50 ms attack). Foley's tail is separate spikes; the bed fills between them. Its rms is `sqrt(rumble)` times the rms of the flash's loudest 0.4 s window (1 = equal, 0.6 = 2.2 dB under, 0.25 = 6 dB under) and falls 3 dB from start to end of the tail. Added before `exposure`, `brightness` and `amp`; the level is not re-normalised, so the flash gets louder with `rumble`. |

At 0 no random draw is made and the samples are bit-identical to before
(pinned by hashes taken before the field existed). The bed has its own random
stream, so the foley flash itself is the same at every `rumble`.

Error: `rumble: 1.5: give 0..1 (0 = foley's tail as it is, 1 = a bed as loud as the flash's loudest 0.4 s)`.

Measured (`tests/it/foley.rs`, distance 1500 m, 16 s document): the 400 ms rms
between 40 % and 80 % of the audible span spans 18-50 dB with `rumble` 0
(seeds 1, 3, 5, 8) and 2.2-5.9 dB with `rumble` 0.6. Spectral centroid of the
whole render 80-82 Hz at `rumble` 0.3-1 (under 100 Hz); 25-90 Hz band
40+ dB above the band over 2 kHz. Not measured: how it sounds.

`length` still changes the rumble's duration; the bed follows the stretched
tail. The `length` macro claim depends on the document `duration` (the tail is
cut at it): that is not changed here.

## `tail`

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
