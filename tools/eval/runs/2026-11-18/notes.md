# Baseline run, 2026-11-18

Ten fresh agents, one at a time, each built one brief from AGENTS.md alone
against a headless server on the build at `543b9bf`. All ten finished with
`verify` passing. Rated blind against the brief: **7 good, 2 ok, 1 bad
(16 of 20, good 2 / ok 1 / bad 0).**

| Brief | Rating | What the listener heard | What the agent built |
|---|---|---|---|
| e01 door slam | good | | the `door_slam` template, unchanged |
| e02 footsteps | ok | "the heel strike is too far from the toe strike in timing" | four wood `footstep` nodes |
| e03 pipe drop | ok | "steel SHEET not pipe" | `modal` metal tube with the thin-metal noise 0.85 / width 0.5 |
| e04 brick drop | good | | the two brick parts plus a noise body |
| e05 UI confirm | good | | the AGENTS.md recipe, shortened |
| e06 campfire loop | good | | `fire` + `crossfade_loop` |
| e07 splash | good | | one `splash` node, 0.1 m |
| e08 door knock | good | | the AGENTS.md knock recipe |
| e09 coin | good | | metal + wood `modal` on a `bounce` |
| e10 sword whoosh | bad | "artificial sweep, sounds like filtered noise / digital" | two narrow band-passed noise layers (q 2.5 and 6) |

## Fixed after this run

- `footstep`: the heel-to-toe gap is 40 % shorter (walk 105 -> 62 ms).
- AGENTS.md and the MCP instructions: metal `noise` 0.4 / width 0.1 / low
  loss for solid metal (0.85 / 0.5 reads as a thin sheet, which the pipe
  agent picked); a whoosh row (the `whoosh_sweep` part, or wide bands,
  q about 1); an upsert replaces the whole node (a field left out returns
  to its default); `pattern` takes `start`; a `fire` loop may still need
  `crossfade_loop` on some seeds.

## Agent friction not fixed (no defect found)

- "Raising the wood modal's gain_db barely moved the mid share" (e09):
  `gain_db` measures exact (+6 / +16 dB, in a `voices` patch too); the
  long-ringing metal disc dominated the share.
- "118 node types" and "stone is not in the library": both from the older
  MCP server instructions in the agents' own context, not from the repo.
- Built-in parts read as "approved": they are, by design (every one was
  rated good by ear).
