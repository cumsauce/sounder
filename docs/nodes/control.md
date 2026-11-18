# `bounce` jitter, `wander` min / max, `ramp` duration

Files: `crates/sfx_graph/src/nodes/events.rs` (`bounce`),
`crates/sfx_graph/src/nodes/control.rs` (`wander`, `ramp`),
`crates/sfx_analysis/tests/it/modulators.rs`.

| Node | New field | Type | Default | What it does |
|---|---|---|---|---|
| `bounce` | `jitter` | number, 0 .. 0.5 | `0` | Each hit after the first moves by a seeded random amount of up to `jitter` times the gap before it (the first hit stays at `start`). Different `seed`s (and so exported variations) differ in timing. At 0 no random draw is made: the stream is bit-identical to before. |
| `wander` | `min`, `max` | numbers | none | Together, the same as `range: [min, max]`. |
| `ramp` | `duration` | seconds, >= 0 | none | The same as `time`. |

Errors name the fields:

- `jitter: 0.8: give a fraction of the gap from 0 to 0.5`
- `wander: give `range: [min, max]` or `min` and `max`, not both: drop `min` and `max``
- `wander: `min` and `max` go together: give both, or use `range: [min, max]``
- `ramp: give `time` or `duration` (the same field), not both: drop one`
- `ramp: missing `time` (seconds the sweep takes; `duration` also works)`
