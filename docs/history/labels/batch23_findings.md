# Batch 23: `impact`'s one-way inputs (2026-11-19)

Five boxes, two designs each, dry and dressed, blind. Key:
`tools/labels/batch23.json`, generator `tools/labels/gen23.py`.

| Box | Picked | Note |
|---|---|---|
| a loose metal lid shaken on a pan | `drive` a `wander` (rate 25), dry | over a swelling 14 Hz sine |
| a marble rolling on a wooden board | none | "none sound like rolling, all just sound like random cracks" |
| a metal sign in the wind, hit once | `excite` breeze (pink noise x slow wander), dressed | over the gale |
| a steel sheet struck, bending as it rings | none picked | "all fine, all sound the same" (a +-6 % wobble and a 20 % sag) |
| a pipe dropped: crack, with and without chips | crack + chips, dressed | "all roughly the same quality" |

- `drive` works for a rattle; an irregular shake beat a periodic one.
- A roll is not a pulsed drive: lifting the marble off makes separate hits.
  Rolling is continuous contact over a rough surface (a profile under the
  contact point), not a force on the ball: the next model to try.
- `pitch` on the sheet was not heard as a bend: 98 % of the sheet's energy is
  under 200 Hz (a 0.8 m, 1 mm plate rings near 89 Hz), where a few percent
  of pitch is hard to hear. Untested on a bright object.
- Chips 11 dB under the pipe body add little: leave them for scenes that ask.
- Saved as parts: `impact_lid_rattle`, `impact_sign_wind`.
