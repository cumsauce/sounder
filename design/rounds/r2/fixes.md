# Round 2 fix cards (rules: swarm plan section 2 and .swarm/WORKER_BRIEF.md; evidence: design/rounds/r2/triage.md and r2/*/feedback.json)

## R2-A: variations check and verify arguments
Owns crates/sfx_analysis/src/checks.rs (the `variations` check and `Toggle` deserialization), src/session.rs (`bare_check`, `check_tool_args` only), tests/r2a.rs under crates/sfx_analysis/tests (+ tests/args.rs additions for the root crate; you may run `cargo test -p sounder`).
1. `variations` compares what the output stage normalizes: when the output uses `peak_db`, the spread limit applies to the sample-peak spread (max - min in dB, default 1 dB) and the loudness spread is reported only; when it uses `lufs` or neither, the existing LU limit applies. The failure text says which was compared and what to change.
2. `variations` is a bare check: `verify {checks: ["variations"]}` and `only` run it with 4 seeds; `{expect: {variations: {seeds: 4}}}` keeps working; wrong shapes (e.g. `{count: 4}`) give an error naming the field and the valid forms ("variations: expected true or {seeds, lufs_spread, ...}"), not "data did not match any variant of untagged enum Toggle". Apply that error improvement to every Toggle-typed expect check.
3. Tests: a footstep-like seeded hit through `peak_db -3` passes variations; through `lufs` the old behaviour is unchanged; the error text for a bad shape names fields.

## R2-B: output warnings and true peak
Owns Output in crates/sfx_graph/src/nodes/offline.rs and the output warnings in doc.rs; tests in crates/sfx_analysis/tests/output.rs (extend).
1. The fade-in warning does not fire when `fade_in` <= 0.001 s, or when a `peak_db`/`lufs` target is set and met (the delivered level is correct); with no target and a cut of 3 dB or more it still warns. Existing warning tests adapt, never weaken.
2. The ceiling limiter holds TRUE peak (4x oversampled, the same estimator as the true_peak_db check) at `ceiling_db`; sounds whose true peak is already under the ceiling stay bit-identical (keep the existing hash tests). A pure 1 kHz sine at the ceiling and a 22 dB-crest noise burst train both end at true peak <= ceiling + 0.05 dB.
3. Document the change in the Output doc comment.

## R2-C: descriptors
Owns crates/sfx_analysis/src/descriptors.rs, tests/descriptors.rs (extend).
1. The `onsets` line gains the fine count: "onsets 3 (40 ms merge); fine 18 (5 ms)"; keep the old fields; the per-onset table keeps its merge, plus a second compact row list "fine onsets: t1 t2 ..." up to 40 times in ms.
2. One headline "swell/attack" line that states one number (time from first sound to 90 % of the smoothed-envelope peak, ms) and says it is the headline; the other attack figures keep their text but are labelled by what they measure so they cannot be mistaken ("10-90 % rise", "to sample peak").
3. "density by tenth" (events per second per tenth) so a rate that rises over the sound is visible (zipper), one short line.
4. Per-onset start>end pitch for blips of 15-60 ms (drips): use a short-window YIN/autocorrelation on the onset's first 40 ms in two halves; report when both halves are voiced and differ by more than 60 cents. Synthetic test: a 30 ms 800->1600 Hz chirp reads start 800+-8 %, end 1600+-8 %.
5. A `descriptors_window(audio, t0, t1)` public function returning the descriptors of a time window, for `listen zoom` text (do not change tools.rs; report the function for the integrator).
Existing lines keep their text and numbers; tests that read them pass untouched.

## R2-D: crumple node
New file crates/sfx_graph/src/nodes/crackle.rs, tests crates/sfx_analysis/tests/crackle.rs, docs/nodes/crumple.md; slot comment `// slot R2D`.
`crumple {density (signal, bursts/s, default 40), size (0..1, default 0.5), hardness (0..1, 0.5), [seed]}`: paper / foil / plastic / dry-leaf crumple as a power-law population of short band-passed bursts (bigger bursts rarer, log-uniform centre frequencies 1.5-9 kHz rising with hardness, 1-12 ms decays falling with hardness), plus sparse louder snaps. Unlike `cloth` the burst count scales with `density` all the way to 400/s (no saturation: level per burst is scaled so overlap does not merge them into noise). Tests: the fine onset count (expect onsets spacing_ms 5) tracks density within 25 % for 20, 40, 80, 160 per s; centroid rises with hardness; `size` lowers the centroid; seeds differ; 1.0 s at density 40 into `output lufs -20` gives >= 20 fine onsets and no missed-target warning; bit-identical; finite at extremes.

## R2-E: modal thickness
Owns crates/sfx_graph/src/nodes/modal.rs, tests/m7.rs (extend; defaults bit-identical, keep the pinned hashes).
`thickness` (metres, default: the material's) overrides the plate/bar/tube thickness, so a 1.5 mm coin, a 3 mm shard or a 4 mm key is reachable without a custom material. Mode frequencies scale linearly with thickness (as the table already does); the size-range error and hint use the effective thickness. Tests: thickness 0.002 vs 0.004 doubles the lowest mode (+-2 %); a metal disc of size 0.025 m, thickness 0.0015 m has 3+ modes between 3 and 9 kHz; glass size 0.03 m, thickness 0.003 m renders; stable sweep over thickness 0.0003..0.05.

## R2-F: small field fixes
Owns the Bounce struct in crates/sfx_graph/src/nodes/events.rs and the Wander and Ramp structs in nodes/control.rs; tests/r2f.rs.
1. `bounce` gains `jitter` (0..0.5, default 0: fraction of each gap by which the seeded random shifts the hit time; the first hit stays at `start`). Hash the default first: unchanged.
2. `wander` accepts `min` and `max` (both required together) as an alias of `range: [min, max]`; both forms given is an error.
3. `ramp` accepts `duration` as an alias of `time`; both given is an error.
Tests for each, error text names the fields.

## R2-G: footstep fields
Owns Footstep in crates/sfx_graph/src/nodes/surface.rs, tests/surface.rs (extend, defaults bit-identical: hash the nine default surface renders first).
`body` (0.5..2, default 1): scales the resonant body frequency (wood/concrete) independent of `weight`; `swish` (0..1, default the current value): balance of the continuous swish vs the stalk/tick layer for grass and gravel. Tests: body 0.5 vs 2 moves the strongest low mode by a factor 4 (+-10 %) for wood; swish 0 vs 1 moves the grass energy share above 2 kHz monotonically; defaults hash-identical.
