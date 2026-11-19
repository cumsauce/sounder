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

## Batch 13 picks

- concrete: speed 1, grit 0.5, in a room (s10). Saved as `concrete_step`.
- all-purpose: gravel, grit 0, speed 1.6, dry (s02). Saved as `all_purpose_step`.
- grass: all bad (take A, speed 1.6 in a room, closest): "the grass crunch would be closer to first impact; the ground sounds a bit too hard (too high freq with first hit?)". The fix was soil (foley's earth) instead of the gravel body, with the blade crunch moved into the first 35 ms of the heel and the toe.

## Batch 14 pick

grass: speed 0.8, dry (s08): "still a bit heavy sounding but otherwise good". Saved as `grass_step` with a 120 Hz high-pass under a `weight` knob (250 Hz light, 40 Hz heavy). The high-pass is unheard.

## Batch 15: the other surfaces, and the split

| Box | Pick | Note |
|---|---|---|
| gravel, whole vs layers | the rebuilt one (s09) | "great": a body with another take's texture over it works |
| carpet | speed 1.4, in a room (s26) | "good but needs some frictive noises to represent the cloth". Saved as `carpet_step`, with fibre rubs added |
| dirt | all bad | "dirt and gravel should be pretty much the same except the gravelly noises are tuned down a bit" |
| ice | all bad | "like stepping on glass, not ice ... I'd expect small cracking / crunch noises" |
| metal | all bad | "like the rungs of a ladder ... I imagine a metal sheet / plate, not a pipe. very tonal" |
| snow | all bad | "good steps, but sounds like gravel. snow has a satisfying crunch" |
| wood | all bad | "a wooden plank being hit, too resonant. a footstep on a wooden floor sounds a lot like a regular footstep" |

Rework: dirt, wood, metal, ice and snow now share the all-purpose body (gravel physics). Over it:
- dirt: gravel's grains at about a third.
- wood: a heavily damped board knock (loss x 8), plus the creak.
- metal: a 1.5 mm sheet's damped dense modes and a short 3.5 kHz shimmer.
- ice: 6-14 sharp crack bursts and now and then a bigger crack.
- snow: 400 sub-ms bond-breaking clicks over 0.2 s, densest mid-press.
- carpet: fibre rubs at the heel and toe.

## Batch 16

- wood: speed 1.4, dry (s14). Saved as `wood_step`.
- snow, ice: all bad again ("give up on this"). Set aside. `step` keeps both surfaces, but they are not finished sounds.
- dirt, metal, carpet: not rated on this page.
