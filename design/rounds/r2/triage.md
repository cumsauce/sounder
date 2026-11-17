# Round 2 triage (binary from commit 2764919)

Core 24 sounds, rounds 0 / 1 / 2: mean judge 9.12 / 9.12 / 9.50; mean difficulty 2.04 / 1.67 / 1.46; mean failed calls 4.5 / 3.2 / 2.25; mean calls 31.3 / 25.5 / 20.3. Reserve sounds (4, new this round): judge 8.75, difficulty 1.5, failed 2.5. Ready-made nodes were used where they fit (footstep 4 of 4, click, shatter, water, wind, fire, cloth, thunder).

| # | Issue | Sounds | Fix |
|---|---|---|---|
| 1 | `verify variations` (loudness spread < 1 LU) fails whenever output uses `peak_db`, the recommended target for short hits; `variations` cannot be named in `checks`; `expect variations` errors with "data did not match any variant of untagged enum Toggle" | 14 | R2-A |
| 2 | fade_in warning "0 ms fade-in cuts the first hit" still fires with `fade_in: 0.0005` even when peak_db/lufs restores the level | 9 | R2-B |
| 3 | sample-peak limiter lets true peak exceed -1 dBTP (+0.1 .. +1.1 dBTP) | 4 | R2-B |
| 4 | onset list merges hits < 40 ms; three conflicting attack figures; no rate-over-time; zoomed window has no text; short-blip pitch start>end | 12 | R2-C |
| 5 | `cloth` cannot make dense crackle; no crackle/crumple generator (also r0, r1) | 3 rounds | R2-D |
| 6 | `modal`: no way to size small thin objects (coin, keys, shards): thickness fixed by material | 5 | R2-E |
| 7 | `bounce` has no seed/jitter: exported variations identical in timing | 3 | R2-F |
| 8 | `wander` min/max and `ramp` duration guessed by testers | 3 | R2-F |
| 9 | footstep: no body-pitch independent of weight; no swish/tick balance | 2 | R2-G |
| 10 | docs: which node; variations; peak_db vs lufs | - | docs pass at the end |

Backlog: thunder denser tail / length macro vs duration, `bubble` glide in Hz, pitch tracker on rough stick-slip, macro on string fields.
