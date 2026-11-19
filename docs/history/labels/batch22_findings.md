# Batch 22: `impact` against the builds (2026-11-19)

Three boxes, six takes each (`impact` alone, `impact` with a noise crack driven
by its `force` port, the current best build; each dry and dressed), blind.
Key: `tools/labels/batch22.json`, generator `tools/labels/gen22.py`.

| Box | Picked | Beat |
|---|---|---|
| metal pipe dropped on stone | `impact` alone, dressed | the approved `large_pipe_drop` |
| coin dropped on stone | `impact` alone, dressed | a `bounce` -> `voices` coin (note: two takes about the same) |
| wooden block dropped on stone | `impact` + force crack, dry | the approved `wooden_crate_drop` |

- `impact` won all three: the coupled solver passes its gate.
- Pipe note: "good but tail is cut off, maybe also trails on too long" (3 s
  render, loss 0.4: the ring was still loud at the cut). The part
  `impact_pipe_drop` uses loss 1.0, 3.5 s and a 0.8 s fade-out: not heard yet.
- Saved as parts: `impact_pipe_drop`, `impact_coin_drop`, `impact_block_drop`.
