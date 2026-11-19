# Batch 5 labels (stone built on the slab), 2026-11-18

14 blind cues from `tools/labels/gen5.py` (key: `tools/labels/batch5.json`),
labelled on the "Stone Round Labels" page. In batch 4 only a heavy slab had
read as stone ("hitting a rock"); small bright hits read as clicks. Every
design here keeps the slab's weight (brown noise low-passed under 250 Hz)
and grit (crackle noise band-passed at 2-2.5 kHz) under the hit.

| Design | Dry | Small room |
|---|---|---|
| brick: weight 200 Hz 0.22 s, body noise 800 Hz, grit, quiet `modal` stone (f0 700, loss 2.5, -6 dB), chips | brick dropped or medium rock hit (good) | good |
| rock: the same smaller (weight 230 Hz 0.15 s, body 700 Hz), `modal` stone f0 900 loss 2 -8 dB, a bounce | brick dropped (good) | good |
| rock, noise layers only | ok | good |
| slab (batch 4 anchor) | hard knock on door (ok) | hitting wall (good) |
| boulder: weight 110 Hz 0.6 s, heavy grit, debris | ok | hitting wall (ok) |
| hammer on stone: `modal` stone f0 1800, loss 1.2, -4 dB | woodblock (ok) | wood clack (bad) |
| two rocks knocked, `modal` stone f0 750 | ok | bad |

- **Stone is weight and grit, with a quiet body.** The brick and the rock,
  which sit between the slab and the clicks in size, are the first sounds
  heard as stone by name. A struck-stone body 6-8 dB under the noise layers,
  with loss 2 or more, helped (the rock with it was named, the one without
  it was not).
- **More ring turns stone into wood.** The hammer's stone body (loss 1.2,
  -4 dB, f0 1800) was heard as a woodblock: a stone that rings reads as wood.
- **Very large reads as a wall.** The boulder and the slab, with long low
  weight, were "hitting wall" or "knock on door": big and dull loses the
  stone's grit.

Kept as parts: `brick_or_rock_drop`, `small_brick_drop` (knobs weight and
pitch / ring; each knob's middle is the rated sound).
