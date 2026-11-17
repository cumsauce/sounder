# R3-F: `thunder.rumble`

Files: `crates/sfx_graph/src/nodes/foley.rs` (`Thunder`),
`crates/sfx_analysis/tests/r3f.rs`.

| Node | New field | Type | Default | What it does |
|---|---|---|---|---|
| `thunder` | `rumble` | number, 0 .. 1 | `0` (off) | Adds a continuous bed of seeded noise band-limited to 25-90 Hz, from the first sound to the end of the audible tail (last sample over -60 dB of the peak; 0.25 s fade out, 50 ms attack). Foley's tail is separate spikes; the bed fills between them. Its rms is `sqrt(rumble)` times the rms of the flash's loudest 0.4 s window (1 = equal, 0.6 = 2.2 dB under, 0.25 = 6 dB under) and falls 3 dB from start to end of the tail. Added before `exposure`, `brightness` and `amp`; the level is not re-normalised, so the flash gets louder with `rumble`. |

At 0 no random draw is made and the samples are bit-identical to before
(pinned by hashes taken before the field existed). The bed has its own random
stream, so the foley flash itself is the same at every `rumble`.

Error: `rumble: 1.5: give 0..1 (0 = foley's tail as it is, 1 = a bed as loud as the flash's loudest 0.4 s)`.

Measured (`tests/r3f.rs`, distance 1500 m, 16 s document): the 400 ms rms
between 40 % and 80 % of the audible span spans 18-50 dB with `rumble` 0
(seeds 1, 3, 5, 8) and 2.2-5.9 dB with `rumble` 0.6. Spectral centroid of the
whole render 80-82 Hz at `rumble` 0.3-1 (under 100 Hz); 25-90 Hz band
40+ dB above the band over 2 kHz. Not measured: how it sounds.

`length` still changes the rumble's duration; the bed follows the stretched
tail. The `length` macro claim depends on the document `duration` (the tail is
cut at it): that is not changed here.
