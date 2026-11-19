# lava and cave

`{"type": "lava", "size", "activity", "seed", "loop"}` and
`{"type": "cave", "depth", "mass", "drips", "openness", "seed", "loop"}`.
Code: `crates/sfx_graph/src/nodes/cave.rs`; tests:
`crates/sfx_analysis/tests/it/cave.rs`.

Ports of the `Lava` and `Cave` beds of the foley project (`foley-dsp/src/lava.rs`,
`cave.rs`, MIT / Apache-2.0); the constants are foley's. Both are mono. In a
`kind: loop` document (or `loop: true`) the last 0.5 s of ringing is folded
onto the start; a cave's 55 Hz rumble leaves a little DC and a filter's
start-up breaks the seam, so end a cave loop with `onepole` hp 20 Hz and
`crossfade_loop`.

## lava

A two-pole 60 Hz low-passed noise rumble (level 2.5 x sqrt(`size`), `size`
capped at 100 for the rumble) and bubbles bursting as a Poisson process at
0.35 a second a m2 times `activity` (at most 30, 32 voices). Each burst is
a hemispherical Helmholtz cavity (radius 1-6 cm, opening 0.3-0.7 of it;
`f = c/2pi sqrt(A/(V L))`, Q from radiation, damped further by the melt:
decay = pi f / clamp(Q/2, 1.5, 10)), a second mode at 0.3 f decaying twice
as fast and 1.5 x louder (the thump), and 40 ms of noise spatter.

| field | meaning |
|---|---|
| `size` | open surface, m2, 0.1..1000 (default 20; foley's `area`) |
| `activity` | burst-rate scale 0..4 (default 1; 0 = rumble alone); not in foley |

Measured, 6 s at defaults, seed 1: rms 0.29, centroid 66 Hz, 98 % of the
energy under 200 Hz, 36 onsets; the rumble dominates the spectrum and the
bursts are the clicks on top (`activity` 0 has a lower centroid). Level is
not normalized (foley's gain 1): use `peak_db` or `lufs`.

## cave

Five ceiling voices, each ringing a sine at 800-1400 Hz (the radial mode of
a falling drop's cavity) for about 30 ms, on Poisson clocks totalling
`5 x drips x 5` a second, each panned to an amplitude of 0.5-1.0; a 55 Hz
rumble of level 0.15 x `depth` x `mass` with a 0.3 Hz wobble; settling
stones (a half-rectified 120-200 Hz sine, 80 ms, 0.04 a second) when
`drips > 0` and `drips + depth > 0.5`. The sum is multiplied by
`1 - openness` and goes through tanh.

| field | meaning |
|---|---|
| `depth` | 0..1 (default 1) |
| `mass` | stone overhead, 0..1 (default 1) |
| `drips` | wetness, 0..1 (default 0.5) |
| `openness` | 0..1 (default 0): 1 is silent |

Measured, 4 s: `drips` 1, `depth` 0: rms 0.35, centroid 1160 Hz, 77 onsets;
`drips` 0 (rumble alone): rms 0.005, centroid 262 Hz; `openness` 1 is exactly
silent.

Deviations: foley's `gain` is fixed at 1; its slow glide of `openness` (0.1 s)
is dropped because the scene is constant for a render; the pre-roll that
settles the filters (4096 samples at 48 kHz) is kept. foley's `drips` are
fixed sines on the ceiling voices only: there is no `onto` surface choice (water, stone,
lava) in the source, so no separate `drip` node was made; for a single drip
use `bubble`.
