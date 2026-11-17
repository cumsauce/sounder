# Round 4 fix cards (rules: swarm plan section 2 and .swarm/WORKER_BRIEF.md; evidence: design/rounds/r4/triage.md and r4/*/feedback.json)

## R4-A: per-hit decay claim
Owns crates/sfx_analysis/src/measure.rs (new measure fns only) and crates/sfx_graph/src/macros.rs `Measure` enum + claim names (add arms; do not reorder); tests/r4a.rs.
Add macro claim measure `ring`: the median over the fine onsets (strength >= 0.25 of the loudest) of each onset's own -20 dB fall time in ms, measured from its peak to the first frame under peak - 20 dB, stopping at the next onset (if the next onset arrives first, that hit is skipped; none usable -> the whole-sound decay). `ring` +1 rises with a longer ring-out. It must be monotone in a modal `loss` knob for: a single hit; a 3-hit knock (pattern 0.2 s); a strike plus a later ring (typewriter-like). Also `tail` claim? NO: only `ring`. Add `ring` to the claim list text in macros.rs error messages and to `listen`'s extra lines is NOT your job (R4-B owns descriptors). Tests: modal `loss` 0.3 -> 3 in each of the three layouts gives strictly falling `ring`; a decay knob on `Decay` time gives rising. Existing tests untouched.

## R4-B: per-onset detail in listen
Owns crates/sfx_analysis/src/descriptors.rs and the onset-table code; tests/r4b.rs. Existing line texts keep working.
1. Onset table gains a column `-60 dB ms` (time from the onset's peak to the signal falling 60 dB under it, `>N` if it hits the next onset or the end).
2. Pitch columns print `n/a` (not a guess) when the onset's modes are inharmonic (inharmonicity over 100 cents) or voiced share is under 30 %; for an onset under 40 ms long with energy, add `rise` (the spectral peak slope up or down in Hz over the burst, from a short-time spectral peak track at 2 ms hop), e.g. `glide up 1.6k>3.4k`.
3. The modes line drops partials whose fitted decay exceeds the sound's length x 5 and says `(N ignored: slower than the sound)`.
4. `inharmonicity` is computed from the strongest partials' ratios to their lowest member (not a YIN pitch), and the `inharmonic: partials at ...` line is printed whenever it exceeds 100 cents.
5. A line `closely spaced events: N pairs within 20 ms (at t1, t2 ms)` from the fine onsets (omit when none).
Tests: a 3 kHz inharmonic metal hit shows `n/a` pitch and cents over 100 (test two clink-like partial sets at 1.5k/1.93k/3.9k/5.3k); a 40 ms bubble sweeping up reads `glide up`; per-onset -60 dB for a 3-knock sound reads about the true decays within 20 %; the 'sustained 600 s' case is gone for a 0.3 s noise-tick footstep; two clicks 12 ms apart report one closely-spaced pair.

## R4-C: macros reach into patches, envelopes and levels
Owns crates/sfx_graph/src/macros.rs (the map application only) and node field setters it needs; tests/r4c.rs.
1. A map entry may name a node inside a `voices` patch as `{"node": "<voices name>/<patch node>", "field": ..., ...}` (any field a macro may write on a top-level node); an unknown path errors listing the patch nodes. Mapping applies to the patch template, so every voice sees it. `$param` mapping stays.
2. A map entry may address a point of an `env` / `curve` `points` list: `{"node": n, "field": "points", "index": i, "key": "x"|"y"|"time"|"value", ...}` (use the node's own point field names; the error lists them).
3. `expr` constants: `{"node": n, "field": "state", "key": "<state name>"}` writes the initial value of that state name.
4. `contact` and `modal` gain optional `gain_db` (default 0, bit-identical when absent) so a body can be balanced against a click without a `gain` node; owns those two node files for this field only.
Tests: a macro on `voices/piece.size` changes the rendered centroid monotonically; env point y mapped moves peak time; expr state mapped changes the output; `contact` `gain_db` -6 lowers peak by 6 +- 0.1 dB.

## R4-D: splash tail, thunder tail, water tone
Owns crates/sfx_graph/src/nodes/{splash.rs or its file in water.rs, thunder in foley.rs (new fields only)}; tests/r4d.rs.
1. `splash` new `tail` (0..3, default 1 = as now, bit-identical): scales the length and number of the bubble cloud after the burst (`tail` 2 = a cloud twice as long); new `burst` (0..2, default 1 bit-identical): level of the opening burst relative to the cavity bubble, so `burst` 2 makes the burst the loudest moment. A 0.15 m splash with `tail` 2, `burst` 2 peaks in its first 20 ms and has >= 10 fine onsets after 0.2 s.
2. `thunder` new `tail` (0..1, default 0 bit-identical): an exponential fade of the whole output reaching -40 dB at the end of the audible tail (0 off); monotone tail length with `tail` 0 -> 1 (decay -20 dB time shortens).
3. `water` new `tone` (0..1, default 0.5 = bit-identical): a +-12 dB shelf tilt above 2 kHz applied to the hiss and the bubble pop band without breaking the periodic loop (no filter after the loop; build it into the render). `tone` 1 raises the energy share above 2 kHz by >= 6 dB; loop_seam still passes at seeds 1-4 (3 s loop, flow 0.5).
Tests for each as stated.

## R4-E: docs pass (last, after the others merge)
Owns AGENTS.md and the INSTRUCTIONS string in src/bin/sounder/server/tools.rs (keep them in sync). Add: the `ring` claim; `n/a` pitch and `-60 dB ms` column; macro paths `voices/node`, `points` index/key, `expr` state; `contact` / `modal` `gain_db`; `splash` `tail`/`burst`, `thunder` `tail`, `water` `tone`; zoom shape reminder in every place zoom is first mentioned; the output default fade-in cuts a bounce/hit at t = 0 (set `fade_in` 0.0005 or start at 0.005); a `pattern` with `jitter` is steadier than a `poisson` for a density-claim knob; full-band vs occupied-band flatness and how to check "high flatness" (`listen` line, no verify check yet); which claim fits which knob (layer level -> lufs/flatness, filter -> centroid, envelope time -> attack/decay, event spacing -> spacing/density, per-hit ring -> ring); the density counter caps near 590 events/s; `decay` node `time` meaning (read the node: say what it is); a fire/footstep/click layering recipe for layer levels via `gain`. Run workspace tests, fmt and clippy.
