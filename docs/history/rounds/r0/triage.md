# Round 0 triage (baseline, binary from f0ea79a)

Source: 24 `feedback.json` (testers) and 24 `score.json` (judges), see
`scores.md`. Counts are sounds that reported the issue. Judges were lenient
(mean 9.1/10, 11 sounds at 10): scores are near the ceiling, so friction is
the main signal from here on.

| # | Issue | Sounds | Kind | Fix type | Round-0 action |
|---|---|---|---|---|---|
| 1 | `describe_op` (and `verify`, `listen`) silently ignore unknown args; `type` returns the whole list | 24 | doc/bug | tool | F1 |
| 2 | Macro `claim` mandatory, error names no macro | 10 | confusing_error | field | F2 |
| 3 | Macros cannot reach `voices` patches or sound params | 9 | missing_field | field | F2 |
| 4 | Node delete form undocumented; top-level `delete:` silently ignored | 9 | doc_gap | doc+bug | F1 + docs step |
| 5 | `verify` cannot run named checks (`clicks`, `onsets`, `duration`); `expect` shape undocumented | 6 | doc_gap | tool | F1 |
| 6 | `output.lufs` misses on peaky/short sounds; warning blames limiter wrongly; thunder -70 LUFS not normalized up | 8 | wrong_result | field | F3 |
| 7 | Default 2 ms fade-in cuts a click at t=0 | 5 | wrong_result | field | F3 |
| 8 | listen: no whole-sound centroid / centroid track / per-onset table / onset density / band shares / pitch contour / attack-to-peak / band flatness / loop lines / silence wording | 19 | sense_gap | sense | S8b |
| 9 | `modal`: lowest mode does not fall with size (408 Hz at 0.8 m vs 537 Hz at 1.4 m), output +22 dBFS, wrong size hint for metal | 4 | wrong_result | bug | M7 addendum |
| 10 | `delete_macros` + same-name `macros` in one call drops the new macro | 4 | bug | field | F2 |
| 11 | `export variations` identical when no node reads the seed, no warning | 3 | wrong_result | tool | F1 |
| 12 | Noise loops do not pass `loop_seam` "easily" as AGENTS.md says | 3 | doc_gap | doc | docs step; S12 `crossfade_loop` |
| 13 | `impulse` blurb says "at the given times" but docs/field naming unclear | 5 | doc_gap | doc | docs step (blurb belongs to wave-1 owner of sources: S1) |
| 14 | `wander` field named `range`, testers tried min/max | 3 | doc_gap | doc | docs step |
| 15 | `duration: null` does not clear a fixed duration (0 does) | 2 | doc_gap | doc | docs step |
| 16 | Node named `mono` refused with misleading error | 1 | confusing_error | tool | F1 |
| 17 | Macro claim `decay`/`swing`/`crest` unusable on multi-hit and short sounds | 5 | sense_gap | sense | S8a owns measures; note |
| 18 | Calls files in shared `/tmp` collided | 1 | process | process | TESTER_SETUP.md fixed in-run |

Wishlist items that are nodes (counted, to roadmap): crackle/grain texture
generator (cloth, paper, fire, gravel: M6/X3), footstep/impact-with-surface
model (wood_floor, concrete: M6), mechanism click (slam, switch), bar/tube
modes (pipe: M7), shard generator (glass: M7 `shatter`), splash/water
(splash, stream: M4), seamless loop helper (wind, fire: S12), mode-aware
`bubble` level (splash, stream: backlog).

Backlog (reported once, severity 1, not selected): thunder rumble length
field, `bubble` level independent of radius, drip second-drip macro,
per-mode gain macro map for `resonators`, `sample_hold`-style spacing macro
for `pattern`.
