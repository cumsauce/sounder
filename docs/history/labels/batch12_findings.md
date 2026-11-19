# Labels batch 12: the `step` node (foley's footstep contact)

| Box | Pick | Note |
|---|---|---|
| concrete | speed 1, in a room (s14) | "the second impact sounds too resonant / wood-y" |
| gravel | speed 1.6, in a room (s02) | "really good footsteps in general. without the gravely noise they'd be perfect all-purpose footsteps" |
| grass | all bad | "doesn't really sound like grass + dirt; look at foley again or use the gravel footsteps as reference" |

What changed after this (`step` now runs its own copy of foley's contact code):
- `grit` 0..2 scales the grains and the scuff. At 0 the gravel step is the all-purpose footstep.
- The ball of the foot is damped: the foot stays on the surface, so its modes die 3 x faster than the heel's.
- Grass uses the gravel step's body (granular modes) with few, dull grains. Over it go the blades brushing the shoe, not the 2 mm plant plate, which rang.
