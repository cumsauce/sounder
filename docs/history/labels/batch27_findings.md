# Batch 27: `rig` (2026-11-19)

Three boxes, two designs each, dry and dressed, blind. Key:
`tools/labels/batch27.json`, generator `tools/labels/gen27.py`.

| Box | Picked | Note |
|---|---|---|
| a ball dropped on a drum | the soft ball (hardness 0.2), dressed | over the hard ball |
| a pan struck with a spoon, its lid resting on it | the heavy lid (0.8 kg), dressed | over the light lid |
| a hammer on a tin sheet, a screw on it | none | the loose-screw takes: "the extra detail on C and D are good but it sounds a bit strange" |

- The rig passes its first listen on two of three boxes.
- The loose screw's rattle was heard as good detail, but strange. My guess
  (unheard): the screw is a clean bar mode at 2.5 kHz, ringing as a pitched
  tone; a `noise` on the screw (0.6-0.8) and a lower `level_db` would make it
  read as a loose part rather than a chime.
- Saved as parts: `rig_ball_on_drum`, `rig_pan_and_lid`.
