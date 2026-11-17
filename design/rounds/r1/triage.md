# Round 1 triage (binary from commit b56563f)

Scores: mean judge 9.12 (unchanged, ceiling), mean tester difficulty 1.67 (was 2.04), mean calls 25.5 (31.3), mean failed calls 3.2 (4.5). Friction fell about 25 to 30 %. New nodes were used where the brief named them (cloth, wind, water, shatter, bubble) but not where it did not (footstep node never used: 0 of 4 footsteps; fire node not tried; splash node used and gave a rising centroid).

| # | Issue | Sounds | Fix |
|---|---|---|---|
| 1 | `new_sound` rejects `duration`/`seed` (loops need two calls) | 3 (rain, stream, fire) | R1-A |
| 2 | `mix` per-input gain not found (object form `{input, gain}` unknown) | 5 | docs |
| 3 | `duration: null` does not clear a fixed duration (0 does) | 2 (+2 in r0) | R1-A |
| 4 | Hard hit at t=0 vs default fade_in vs silent_edges vs requested peak (peak applied before fades; warning suggests fade_in 0 which fails silent_edges) | 6 | R1-B |
| 5 | limiter-squash warning shown when output uses `peak_db` | 3 | R1-B |
| 6 | macro warning when node and macro created in one batch | 3 | R1-B |
| 7 | macro claim measures for multi-hit/short/dense sounds (density, attack, per-hit decay) | 8 | R1-C |
| 8 | `onsets` check merges events < 40 ms; no way to count dense ticks | 4 | R1-C |
| 9 | macro cannot reach a node inside a `voices` patch; error does not point to `{param}` entries | 3 | R1-B (error), docs |
| 10 | `modal`: no loss override with a named material; glass plate puts 98 % in one mode; size floors too high for small shards/keys | 6 | R1-D |
| 11 | `splash` node: centroid rises instead of falling | 1 (difficulty 3) | R1-E |
| 12 | no latch / mechanism click node | 2 (+1 r0) | R1-F |
| 13 | `catalog` rejects `doc`; `listen zoom` error does not show the shape | 3 | R1-A |
| 14 | `thunder`: no brightness / rumble length fields | 2 | R1-G |
| 15 | `gain` node field is `db` (others `gain_db`) | 3 | R1-G |
| 16 | node discoverability: which ready-made node fits which sound; footstep unused; seed variation guidance; export variations identical without jitter | 8 | docs (last) |

Backlog (single report): wind `tone` field, voices-expression `=` error text, bubble seed, `listen` pitch tracker on rough stick-slip.
