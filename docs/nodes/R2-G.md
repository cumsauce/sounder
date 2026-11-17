# R2-G: `footstep` `body` and `swish`

Round 2 (footstep used in 4 of 4 sounds): no way to move the body pitch apart
from `weight`, and no balance between the continuous swish and the
stalk / stone-tick layer.

## New fields (optional; omitted = bit-identical to before)

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `body` | number | `1` | 0.5 .. 2 | Multiplies the resonant body frequency, independent of `weight`. Wood: all three body modes (`f0`, 1.74 f0, 2.9 f0). Concrete: the low thud mode (120 Hz / sqrt(tilt)); the mid modes and the scuffs do not move. Gravel: the thump (95 Hz). Grass: the soft low noise band (140 Hz). 0.5 is an octave down, 2 an octave up. |
| `swish` | number | `0.5` | 0 .. 1 | Grass: swish bands x `1 + 2 (swish - 0.5)`, stalk ticks x `1 - 2 (swish - 0.5)` (0 = ticks only at 2x, 1 = swish only). Gravel: stone ticks x `1 - 1.2 (swish - 0.5)` (1.6x at 0, 0.4x at 1) and, above 0.5, a swish bed (the grass band, 0.35 x level x `2 (swish - 0.5)`) fades in. Concrete and wood ignore it. |

"The current value" of `swish` is 0.5: the neutral setting at which both
layers have exactly the old gain (the gains are written `1 +- (swish - 0.5) k`
so 0.5 multiplies by exactly 1.0). The gravel bed is drawn after every other
random number, and only when `swish` > 0.5, so no other part of the step
reshuffles when it moves.

Errors: `body: 3: give a number from 0.5 (an octave down) to 2 (an octave up), default 1`,
`swish: 1.5: give a number from 0 (ticks only) to 1 (swish only), default 0.5`.

## Measured

| What | Measured | Test |
|---|---|---|
| defaults: 9 renders (4 surfaces default, 4 heavy run boot seed 3, wood hard light sneak seed 5) | FNV-1a of the f32 bits unchanged from before the fields existed | `body_and_swish_defaults_are_bit_identical_to_before` |
| wood strongest low mode, body 0.5 -> 2 (ratio) | weight 0.2: 91.6 -> 376.9 Hz (4.11); weight 0.5: 78.3 -> 305.5 Hz (3.90); weight 0.8: 61.9 -> 245.0 Hz (3.96); asserted 4 +- 10 % | `footstep_body_moves_the_wood_mode_by_the_factor` |
| centroid below 400 Hz, body 0.5 / .71 / 1 / 1.41 / 2 | concrete 90 / 96 / 114 / 146 / 194; gravel 64 / 75 / 95 / 127 / 172; grass 148 / 172 / 198 / 225 / 250 Hz, strictly rising | `footstep_body_is_independent_of_weight_and_moves_concrete_gravel_grass` |
| grass energy share above 2 kHz, swish 0 / .25 / .5 / .75 / 1 | 0.104 / 0.302 / 0.561 / 0.701 / 0.771, strictly rising | `footstep_swish_trades_the_swish_for_the_ticks_on_grass` |
| gravel share above 2 kHz, same | 0.902 / 0.901 / 0.898 / 0.891 / 0.882, strictly falling; rms (raw) -35.4 / -37.1 / -39.0 / -37.9 / -34.1 dB (not monotonic: fewer stones, then the bed) | `footstep_swish_on_gravel_fades_stones_and_adds_a_bed` |

Grass: the share above 2 kHz rises with swish because the ticks are quiet
against the 140 Hz body band; at swish 0 the low body dominates, at 1 the
3.1 kHz swish does. Whether it sounds right is the human's call; give them
macros on `body` and `swish`.
