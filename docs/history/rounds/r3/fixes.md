# Round 3 fix cards (rules: swarm plan section 2 and .swarm/WORKER_BRIEF.md; evidence: design/rounds/r3/triage.md and r3/*/feedback.json)

## R3-A: expressions in voices-patch fields
Owns the string-to-signal resolution (find it: params.rs / eval.rs / calc.rs where "=expr" and "$param" are resolved and where a string becomes a node reference) and its error text; tests crates/sfx_analysis/tests/r3a.rs.
A string in a signal field that contains `$` or arithmetic on numbers (e.g. "1.5 * $value", "0.1 + $value * 0.9", "2*$rand") currently is parsed as a node name and fails with "reads `1.5 * $value`, which is not a node". Fix: such a string is treated as an expression exactly as if it started with `=` (keep `=` working; keep every existing valid node name, `name.port` and `$param` reference working; a string that is a valid node/port name is never an expression). Where it is still not resolvable, the error says: "not a node; write expressions with a leading `=`, e.g. \"=1.5 * $value\"". Tests: the three round-3 examples render; names with dots and underscores still resolve; an unknown name error contains the `=` hint; `refs()` does not report expression text as a node reference.

## R3-B: density measure and event summary
Owns crates/sfx_analysis/src/measure.rs (density_per_s, fine_events) and descriptors.rs (the density line, plus new lines); tests/r3b.rs. Existing tests pass untouched.
1. `density` (macro measure and the descriptors' density line) must be monotone in the true event rate for band-passed tick trains from 10 to 500 events/s (Poisson and regular): today it saturates near 100/s and then falls. Replace the counter with one that stays proportional while ticks overlap (e.g. count envelope peaks with an adaptive refractory time of min(4 ms, half the median gap) and, above ~150/s, estimate the rate from the envelope's autocorrelation / mean peak spacing); document the method in the doc comment. Test: poisson ticks (3 ms decays) at 20, 40, 80, 160, 320, 500/s read within 25 % of the true rate and strictly increasing; regular trains likewise; unchanged within 5 % for the existing density tests (20-200/s).
2. New descriptors line: "events: N strong (>= 0.5 of the loudest onset), M weak; last event at T s (the sound's tail after it is silence/continuous)"; strong/weak from the fine onsets by peak-strength; handles loops (omit).
3. Keep every existing line's text.

## R3-C: modal band and lowest-mode field
Owns crates/sfx_graph/src/nodes/modal.rs; tests/m7.rs (extend; defaults bit-identical, keep the pinned hashes).
1. `f_min` and `f_max` (Hz, defaults none): keep only modes inside [f_min, f_max] (applied after the mode table, before the `modes` cap, so `modes` counts kept modes); an empty band is an error naming the field and the table range.
2. `f0` (Hz, 20..18000, default none): sets `size` so the lowest mode of the shape sits at f0 (solve from the shape's table and the effective material/thickness); giving both `f0` and `size` is an error. Needed so a brief like "lowest mode 2 kHz" is one field.
Tests: f0 2000 gives a lowest strong peak within 3 % for plate, bar, tube and membrane; f_min/f_max remove modes outside (peaks outside the band under -40 dB relative); defaults hash-identical.

## R3-D: verify gap assertion and seeds
Owns crates/sfx_analysis/src/checks.rs (the onsets check), src/session.rs (verify path only), tests/r3d.rs in sfx_analysis and tests/args.rs additions (you may run `cargo test -p sounder`).
1. `expect onsets` gains `gap: [lo, hi]` (seconds): every gap between consecutive onsets (using `spacing_ms` resolution) must lie in the range; failure text lists the offending gap and times. Round 3: "assert exactly 3 onsets 0.15-0.3 s apart" -> `{min:3, max:3, gap:[0.15,0.3]}`.
2. `verify {seeds: N}` (2..8): re-renders the sound with seeds 1..N (doc seed overridden for the render only, nothing saved) and runs the same checks on each; the report groups FAILs by seed ("seed 3: loop_seam FAIL ..."). Document in the tool description. Test via Session.
Existing behaviour unchanged when the new fields are absent.

## R3-E: water loop
Owns crates/sfx_graph/src/nodes/water.rs, tests/water.rs (extend).
Round 3 stream_loop: a `water` in a `kind: loop` document passed loop_seam by itself but failed (3.45x wrap) at seed 1 after macros / filters were added; stream tester added crossfade_loop. Find why (render water in a loop doc at 40 seeds x flow 0.2..0.9 x density 60..400; with a high-pass 250 Hz and low-pass 6 kHz after it; with a macro on flow) and make the periodicity hold for every case that is within the node's own output, and for linear filters after it (a filter after a periodic signal is not periodic at the seam: so also make `water` output start-aligned with a primed state, or document in the blurb that filters after it need `crossfade_loop`, whichever you can prove). Test: 40 seeds pass loop_seam at default checks for the plain node and for node+hp+lp (or the documented requirement is enforced by a clear warning in the render summary "node X after a looped source breaks the seam: add crossfade_loop").

## R3-F: thunder rumble
Owns the Thunder struct in crates/sfx_graph/src/nodes/foley.rs, tests/r3f.rs. Hash the defaults first: bit-identical.
`rumble` (0..1, default 0 = unchanged): adds a continuous low rumble bed (shaped noise 25-90 Hz with a slow decay following the clap, level relative to the clap by `rumble`) so the tail is not isolated spikes. Tests: with rumble 0.6 the 400 ms rms between 40 % and 80 % of the audible span varies < 6 dB (continuous) while default spikes vary more; centroid stays < 100 Hz; default hash identical; finite at extremes.
