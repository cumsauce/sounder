# Macros in full: map paths and claims

AGENTS.md "Macros: knobs for the human" holds the rules; this file holds the
deeper map forms and every claim measure with its measured limits.

## Map entries that reach deeper

- **Map entries reach deeper** (R4-C). A node inside a `voices` patch is
  `"node": "voices_name/patch_node"` (deeper for a `voices` in a patch): the
  write goes to the patch template, so every voice sees it, with no param to
  declare (a typo errors with the patch's node names). A breakpoint list
  (`env` `points`, `curve` `points`) takes `"field": "points", "index": 1,
  "key": "x"` (also `time`, `t`; the first number of the pair) or `"key": "y"`
  (also `value`, `v`). An `expr`'s initial state is `"field": "state", "key":
  "y1"` (the entry must exist). Known gap: the "driven by a macro" warning and
  the window's effective-value display do not look inside patch paths, so an
  edit to a patch field a macro writes is silently ignored.
- **A map entry may write a sound param** instead of a node field:
  `{"param": "size", "from": 0.2, "to": 0.8}` (no `node`, `field`, `col`,
  `index`). A `voices` patch (and its `map`) reads the sound's params, so this
  is one way a knob reaches the inside of a patch (the other is the path form). A map entry naming a patch node without the path is an error that
  says so and shows both: `"voices_name/node"`, or declare a param with
  `sound_ops` `params`, use it in the patch as `"=$name"` (or `$name` in the
  `voices` `map`), map `{"param": ...}`.

## Claims: the measures and how verify holds them

- **The name is a claim.** `claim` is `[measure, direction]`: `centroid`
  (Hz), `decay` (ring-out), `crest` (dB), `lufs`, `flatness`, `swing`
  (loudness spread), `inharmonicity` (cents off a harmonic series; clean to
  bell-like), `roughness` (percent of the loudness envelope's energy at
  20-150 Hz; smooth to beating), `density` (events per second at the fine 1 ms
  hop of `listen`'s `density` line: rain, crackle, ticks), `attack` (ms from
  the first sample over -60 dBFS to the first 0.25 ms frame at 90 % of the
  peak: a swell or one hit), `spacing` (s, the median gap between the
  strongest onsets: a bounce, a roll, a footfall pair; 0 under two onsets),
  `ring` (ms, the median over the strong onsets, at least 0.25 of the loudest,
  of each hit's own fall to -20 dB under its peak, stopping at the next onset;
  a hit cut off by the next onset is skipped, none usable falls back to
  `decay`: it falls as a modal `loss` rises and rises with a `decay` node's
  `time`);
  `1` rises over 0..1, `-1` falls. `verify`
  (macros, in the defaults when a sound has any) sweeps each knob over 0, .25, .5, .75, 1
  with the others where they are and requires the measure to move that way at
  every step and no position to peak over -0.3 dBFS, **and that the knob moves
  the measure by at least 2 % of its largest value across 0..1** (one step
  may go against the claim by up to 15 % of the end-to-end move, so a knob
  over a noisy layer, `modal` `noise` or a noise crack, still holds its
  claim: keep the noise, do not lower it to pass) ("the knob
  barely moves X: widen its map, or rename the macro"). A FAIL says to rename
  the macro to what it really does, or fix what it moves. Pick names that
  match: brightness -> centroid up, weight -> centroid down, length -> decay
  up, punch / snap -> crest up, grit / howl -> flatness, gustiness -> swing,
  clang / stiffness -> inharmonicity up, rattle / buzz -> roughness up,
  rate / busyness -> density up, softness / swell -> attack up, spread /
  bounciness -> spacing up, ring / sustain on a multi-hit sound -> ring up.
  Pick the claim that fits the knob: a layer's level (a `gain` before the
  output) -> `lufs` only with a `peak_db` output (else `flatness`, or leave
  the claim off); a filter or brightness knob -> `centroid`; an envelope time
  -> `attack` (a rise) or `decay` (a single hit's fall); event spacing ->
  `spacing` (a few hits) or `density` (any event-rate knob: rain, crackle,
  crumple, bubbles); the ring-out of each hit in a knock, typewriter or
  footfall train -> `ring`. The `density` claim holds for tick trains
  from 10 to about 500 events/s (calibrated on 3 ms ticks; the counter's
  ceiling is near 590 events/s, so a knob that sweeps past it flattens; it
  is an estimate when the ticks overlap, which `listen` says: "density
  estimate"). A `pattern` with `jitter` gives a steadier count than a
  `poisson` of the same rate (its chance clumps move the counter between
  seeds), so use it under a density-claim knob when the human did not ask
  for a random stream.
  **An `output.lufs` target renormalizes the level, so a "loudness" /
  "volume" macro with claim `lufs` cannot move**: use a `peak_db` target (then
  map a `gain` before the output) or claim another measure, or leave the
  claim off. When the whole output fails a claim (a loud layer masks the
  knob, or a multi-hit sound's `decay` jumps between hits), `verify` also
  measures it on the knob's own layer (the node its `map` writes, followed
  down to the `mix`) and passes it if it holds there, saying "measured on
  `node`"; `decay`, `swing` and `crest` stay unreliable on a very short
  sound, so give those a `density`, `attack`, `spacing` or `ring` claim. `flatness` is the
  full-band measure; to check a "high flatness" (grit, hiss) knob by hand read
  `listen`'s whole-sound flatness lines (full-band, and the occupied band, the
  middle 80 % of the energy: a band-limited texture reads low full-band and
  high occupied-band) at the knob's ends with `sweep`; `verify` has no
  occupied-band check yet.
