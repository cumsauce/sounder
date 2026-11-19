# Run 2, 2026-11-18 (after the baseline's fixes)

Ten fresh agents built the same briefs on the build at `76635aa` (footstep
heel-toe gap 40 % shorter; metal, whoosh, upsert and loop notes in
AGENTS.md). Both runs were rated blind on one page, shuffled
(`cues_ab.json`, `scores_ab.json`).

| | Baseline (`2026-11-18`) | Run 2 |
|---|---|---|
| good / ok / bad | 8 / 1 / 1 on this page (7 / 2 / 1 on its own page) | 9 / 0 / 1 |
| points (good 2, ok 1) | 17 (16) | 18 |

- **Pipe drop: ok -> good.** The baseline was "a sheet of metal falling
  and bouncing" both times it was rated; run 2's pipe (`modal` metal tube,
  noise 0.3, loss 0.5, a low thud) was good. It did not use the 0.4 /
  width 0.1 recipe either: lowering the noise from the sheet value is what
  mattered.
- **Footsteps:** the baseline was ok on its own page, good here; run 2
  (the shorter heel-toe gap) good. One rating per cue moves by a step.
- **Whoosh: bad both times** ("still synthetic"). The run 2 agent ignored
  the new whoosh row and used a narrow band again. The listener set the
  whoosh aside: it is out of the brief set from here on.

Listener's summary: "basically everything but the sword swoosh is great".
