# Batch 29: `rig` waveguides and the air joint (2026-11-20)

Three boxes, blind, each take dry and dressed. Key:
`tools/labels/batch29.json`, generator `tools/labels/gen29.py`.

| Box | Picked | Beat |
|---|---|---|
| a ball on a drum with a shell tube | the approved `rig_ball_on_drum`, dry | the drum with the air joint at strength 2 and 6 |
| a dropped pipe with its air column | the approved `impact_pipe_drop`, dressed | the pipe with the air joint at strength 3 and 8 |
| a string struck on a wooden body | `rig` waveguide string tied to a wood plate, dry | a plain `string` node and a quieter body |

- The waveguide string on a body works: saved as `rig_string_on_box`
  (knob: coupling, the spring tying the string to the body; at 0.5 it is
  the heard 2e4 N/m. A body-level knob moved the output by only 0.76 dB:
  the rig normalizes its output).
- The air joint lost both of its boxes to the builds without it: it is kept
  but unheard-good; leave it out of drums and drops.
