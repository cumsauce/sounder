## water in a loop (R3-E)

`water` is periodic over the document length by itself (`kind: loop` makes
`loop` true): bubble tails wrap and the hiss filters and DC high-pass are
primed from the end. Measured, default checks including `loop_seam`, seeds
1..40 x flow 0.2 / 0.5 / 0.575 / 0.9 x density 60 / 150 / 400: 0 of 480 fail
at 1 s and at 3 s (2 of 480 at 8 s, both a loud sparse bubble ringing over the
wrap, which `loop_seam`'s whole-loop 99th-percentile step reads as 2.2x; the
sample steps there climb smoothly 0.057 .. 0.123, 0.166 (the wrap), 0.039).

A node after `water` is not periodic at the wrap: a filter starts from zero
state. `water` -> `filter` hp 250 Hz -> `filter` lp 6 kHz failed `loop_seam`
in 169 of 480 cases at 3 s (35 %). End the chain with `crossfade_loop`
(`fade` 0.25): 1 of 480 fail. `splash` is sparse and random, so in a short loop
one end window may hold a splash and the other not: the spectral half of
`loop_seam` fails in about 17 % of cases at `splash` 0.5 (0 at flow 0.9 with
the crossfade); raise `flow`, or crossfade.
