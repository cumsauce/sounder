# Round 3 triage (binary from commit f5c7f06)

Core 24 sounds, rounds 0 / 1 / 2 / 3: mean judge 9.12 / 9.12 / 9.50 / 9.25; mean difficulty 2.04 / 1.67 / 1.46 / 1.29; mean failed calls 4.5 / 3.2 / 2.25 / 1.42; mean calls 31.3 / 25.5 / 20.3 / 18.7. Reserve (4 new): judge 9.5, difficulty 1.75, failed 2.5.

Stop test (section 9.6) after round 3:
1. issues that qualify for fixing (>= 3 before the cap): MORE than 3 (see table) -> NOT met.
2. mean judge rose by less than 0.3 (it fell 0.25): met.
3. mean difficulty fell by less than 0.2 (fell 0.17): met.
All three must hold, so round 4 runs after these fixes.

| # | Issue | Sounds | Fix |
|---|---|---|---|
| 1 | a numeric/expression string in a node field inside a `voices` patch (`"gain": "0.1 + $value * 0.9"`, `"speed": "1.5 * $value"`) is read as a node name; error says "not a node" and no hint | 3 (+2 in r2) | R3-A |
| 2 | macro `density` claim fails or saturates on dense / bubbly / crackle sounds (rain, splash, paper, boiling, stream) | 5 | R3-B |
| 3 | `listen` cannot separate strong events from weak ones (clinks vs crackle, hit vs chips), nor say where the last event is; sparse tail decay misleading | 6 | R3-B |
| 4 | `modal`: no band limit on modes and no way to set the lowest mode in Hz (glass_clink, ice, coin, keys: 4 rounds running) | 4 | R3-C |
| 5 | `verify expect onsets` cannot assert the gap between hits; no way to run loop_seam / default checks over several seeds in one call | 4 | R3-D |
| 6 | `water` with `loop` (kind loop) failed loop_seam at seed 1 once the sound had macros | 2 | R3-E |
| 7 | `thunder`: rumble is isolated spikes, not continuous; length macro claim depends on duration | 3 | R3-F |
| 8 | docs: per-input `mix` gain, `verify` forms for variations/onsets, when claims fit (density, lufs with a target), seed variation | 8 | docs pass |

Backlog: a claim for raggedness (rope, creak), `bubble.rise` in Hz, `shatter` crack level, `poisson` start offset, `cloth.weight` monotone, voices `max` vs macro sweep hint.
