# Labels batch 11: the library's gaps

This was a grouped page (`tools/labels/gen11.py`): two steps per footstep take, each take rendered dry and in a room.

| Box | Pick | Note |
|---|---|---|
| steps on concrete | all bad | "all sound more like a snare / wooden drum" |
| steps on gravel | all bad (the dry boot, s15, closest) | "something hitting back and forth, like a pitter patter, instead of the crunch" |
| steps on grass | all bad | "too heavy, but the right idea. sounds like hitting grass, not stepping" |
| large splash | 0.4 m over a brown-noise thump, in a room (s30) | "sounds like close up / in water"; saved as `splash_close` |
| electric motor | geared fan motor, in a room (s07) | "its small high rpm motor"; saved as `small_motor_spin` |

The `footstep` node fails on concrete, gravel and grass, even at weight 0.3. Foley's `foley-synth/src/modal.rs` has a footstep contact: a Hertz heel strike on a material, sole scuffing as the foot rolls, and grains for gravel and sand. It is the candidate to port.
