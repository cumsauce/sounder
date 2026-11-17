# Round 1 fix cards

Same rules as design/swarm-plan.md section 2 and the worker brief (.swarm/WORKER_BRIEF.md). Evidence: design/rounds/r1/triage.md and r1/*/feedback.json.

## R1-A: server tools (root crate)
Owns src/bin/sounder/server/tools.rs, args.rs, src/session.rs, tests/args.rs (extend).
1. `new_sound` accepts `duration` (seconds), `seed` and `kind`; a loop with no duration is still an error naming the field.
2. `sound_ops` `duration: null` clears a fixed duration (back to automatic length); `duration: 0` keeps working. Serde must tell missing from null.
3. `catalog` accepts `doc` (alias of `name`) like every other tool; both given and different is an error.
4. A bad `listen` `zoom`/`sweep` argument error shows the accepted shape (`zoom {t: [a, b], hz: [lo, hi]}`, `sweep {param, values: [..]}`).
Tests in tests/args.rs for each.

## R1-B: output warnings and macro creation
Owns crates/sfx_graph/src/nodes/offline.rs (Output), doc.rs (warning functions only), macros.rs (validation/warning code only, NOT the Measure enum), crates/sfx_analysis/tests/output.rs (extend).
1. `peak_db` and `lufs` targets are met AFTER the edge fades (today the peak is set first and the fade then cuts a hit at t=0): a hit at 0 with fade_in 0.0005 and peak_db -1.5 delivers -1.5 +- 0.1 dB final. Sounds with default fades and no hit near 0 stay bit-identical (keep the existing hash test).
2. The fade-in warning suggests `fade_in` 0.0005 (0.5 ms: reaches zero at sample 0, so silent_edges passes) not 0, and states the dB lost.
3. The limiter-squash warning does not fire (or says so truthfully) when the output uses `peak_db` and the final peak equals the target.
4. No "driven by macro / your value is ignored" warning for a field whose node and macro are created in the same sound_ops call.
5. A macro map entry naming a node that exists only inside a `voices` patch gets an error that says so and shows the `{"param": ...}` form ("node `m` is inside the voices patch of `v`: declare a param with sound_ops params and map {param: ...}").
Tests for each.

## R1-C: measures and onset check
Owns crates/sfx_analysis/src/measure.rs, checks.rs, tests/r1c.rs; and the Measure enum in crates/sfx_graph/src/macros.rs (variants, ALL, name, unit only).
1. New macro claim measures: `density` (onsets per second at the fine hop used by the descriptors' density line), `attack` (ms, time to 90 % of the peak), `spacing` (median gap between the strongest onsets, s). Document each in the enum doc comments; sweep tests with synthetic signals (a train whose rate rises -> density rises; a swell whose rise lengthens -> attack rises; a bounce train whose first gap grows -> spacing rises).
2. `expect onsets` accepts `spacing_ms` (merge distance, default 40 so existing behaviour is unchanged) and `min`/`max`; with `spacing_ms: 5` a 200/s click train counts ~its events (within 10 %).
3. `verify`'s macro sweep needs a minimum effect: a claim that moves by less than 2 % of its range across 0..1 is a FAIL with "the knob barely moves X: widen its map" (check it does not fail any existing test; if it does, make it a warning line and say so).
Wire the new names through window_state.rs / args.rs only if compile requires it (minimal edits, report).

## R1-D: modal
Owns crates/sfx_graph/src/nodes/modal.rs, tests/m7.rs (extend, do not weaken).
1. `loss` multiplier (default 1) on the material's loss, so `"material": "wood", "loss": 0.5` rings twice as long; numbers/ranges validated.
2. `tilt` (dB per octave, default 0, -12..12) on mode gains to flatten or darken the spectrum (round 1: a glass plate put 98 % of the energy in one mode).
3. Relax the size floor/ceiling for plate/bar/tube so glass shards 3 mm thick and small metal keys are reachable: the size error range must come from the same table and every size inside it must stay stable (Nyquist: modes above 0.45 sr are dropped, not folded); a size that would drop ALL modes is an error naming the fix.
4. Defaults bit-identical to the current plate (record hashes first; the M7 test pins three).
Tests: loss doubles the 20 dB decay time (+-15 %); tilt +6 dB/oct raises the mode-level ratio of mode 4 to mode 1 by about 6*log2(f4/f1) dB (+-2 dB); a glass shard of 0.03 m renders 3+ modes between 1.5 and 8 kHz.

## R1-E: splash
Owns crates/sfx_graph/src/nodes/water.rs (splash part), tests/water.rs (extend).
`splash` today gives a RISING centroid (605 Hz to 3.7 kHz in round 1; brief: broadband burst then a bubbling tail with a FALLING centroid). Redesign the envelope/spectrum so the burst is bright and brief and the bubble cloud follows with larger bubbles later (falling pitch), keeping the node's fields. Tests: centroid by tenth falls monotonically (allow 1 reversal) for sizes 0.02, 0.1, 0.5; early 20 % has energy above 8 kHz and the last 50 % has under 2 % of it; a splash into `output {lufs -24}` has no missed-target warning. Document the change in docs/nodes/M4.md.

## R1-F: click (mechanism) node
New file crates/sfx_graph/src/nodes/mechanism.rs, tests/mechanism.rs, docs/nodes/click.md. Slot comment `// slot R1F`.
`click {kind latch|switch|button|lock|clasp, [size 0.5], [hardness 0.7], [at], [double true], [seed]}`: a mechanical click as 1 or 2 transients (a press and a release/second snick, 20-80 ms apart by kind) through resonators (small body modes 1.5-8 kHz scaled by size). `kind` is a word. Seeded jitter on timing/level/mode frequencies so seeds differ (+-3 %). Tests: latch gives 2 onsets 25-60 ms apart (use the descriptors onset table), switch centroid 2-6 kHz and length under 120 ms, bigger size lowers the centroid, seeds differ, a hit at `at: 0` is not cut (document fade_in), bit-identical per seed, finite at extremes. This is the single-node route for door_slam latch and switch_click.

## R1-G: small field fixes
Owns the Thunder struct in crates/sfx_graph/src/nodes/foley.rs and the Gain struct in nodes/maths.rs, tests/r1g.rs.
1. `thunder` gains `brightness` (0..1, default unchanged behaviour at the current value; a post-shaping tilt that moves energy above 100 Hz: centroid rises monotonically) and `length` (stretch of the rumble, 0.5..3, default 1 bit-identical). Hash the defaults first.
2. `gain` accepts `gain_db` as an alias of `db` (keep `db`); both given is an error.

## R1-H: docs (runs last, after A-G merge)
Owns AGENTS.md, INSTRUCTIONS in tools.rs, args.rs doc comments. See the docs list in design/rounds/r1/triage.md rows 2, 9, 16, plus: the new measures `density`, `attack`, `spacing` (claims) and the `onsets {min, max, spacing_ms}` check; `new_sound` duration/seed; `duration: null`; `catalog doc`; `output` peak_db after fades and the fade_in 0.0005 advice; `modal` loss/tilt; `click` node; `thunder` brightness/length; `gain_db`. Add a compact "Which ready-made node for which sound" table to INSTRUCTIONS (footstep surface presets, fire, wind, water/splash, cloth/rolling, shatter, explosion/gunshot, engine/motor, tube/string, click, thunder, modal shapes), stating each node's one-line fit and that building from primitives is fine for others. Add "Variation" guidance: seeds change only what reads the seed; use pattern jitter/poisson/voices $rand/bubble jitter so the 4 variations differ, and `mix` input objects {input, gain, pan, at}. Keep INSTRUCTIONS compact.
