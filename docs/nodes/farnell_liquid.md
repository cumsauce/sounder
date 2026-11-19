# `stream` and `pour`: running and poured water from Farnell's Pd patches

Two nodes ported from the companion Pure Data patches of Andy Farnell's
*Designing Sound*. Code: `crates/sfx_graph/src/nodes/farnell_liquid.rs`.
Tests: `crates/sfx_analysis/tests/it/farnell_liquid.rs`. The existing `water`
node (a Minnaert bubble population, `nodes/water.rs`) is a different model and
is unchanged; these two are the book's.

Not compared with a Pd render (Pd was not installed here): the netlists were
read and ported box for box; the numbers below are measured on the nodes.
Neither has been heard.

## The shared core: `waterflow`

`RUNNINGWATER/waterflow.pd` (the same patch as `running-water3.pd`, with
`rate` and `depth` inlets added; `pour1.pd` carries two copies inside it).

- `metro` every `rate` ms (default 6) draws `bilexp`: `exp(9 m) / 23000` with
  a random sign, m uniform in 0..1. Most draws are a few Hz, one in ten is
  over 100, the largest is 0.35.
- The frequency is `depth + 1600 bilexp`, reached over a 2.689 ms `line~` ramp
  from the previous value, and drives `osc~` (a cosine).
- The loudness is `(0.9 lop~10(clip~ 0 1 (slope)))^2` with `slope = f[n] -
  f[n-1]` (`fexpr~`, Hz a sample at 44.1 kHz, rescaled to the sample rate).
  Only upward glides count: **water is a sine whose rising pitch bursts are
  loud.** Output x 0.3.
- Deviation: `line~` starts at `depth`, not 0 (the patch's first jump from 0
  is a start-up blip).

Precursors, not ported as modes: `water1.pd` (a bare sine at `800 + random
1600` every 6 ms), `running-water2.pd` (the same with `400 + random 1600`
every 12 ms, a `slew` ramp and no squaring), `running-water3.pd` (=
waterflow). `water4.pd` is the controllable variant: `rate`, `base`, `range`,
`slew`, `amp`, `hip` sliders, and an amplitude detector that high-passes the
frequency at 1 kHz and subtracts a one-block `vd~` copy instead of `fexpr~`;
its `base` and `range` became `depth` and `range`. `water2.pd` ("stream try 3",
a sample-and-hold and `pow~` noise design) and `water3.pd` (band-passed noise
through `delwrite~` / `delread~`, a cubed envelope and an `rpole~` integrator
driving a `phasor~` sine) are earlier tries at the same sound and are not
ported; the book's final running water is `waterflow`.

## `stream`

`layers` independent waterflows summed (each with its own seeded generator),
divided by `sqrt(layers)` so more layers thicken the brook without making it
louder. Fields:

| Field | Default | Meaning |
|---|---|---|
| `flow` | 1 | 0.1..10; the metro period is `6 / flow` ms |
| `depth` | 800 | base pitch, Hz (100..5000): lower reads as deeper water |
| `range` | 1600 | the patch's x 1600: width of the excursions, Hz (0..5000) |
| `layers` | 2 | waterflows summed, 1..6 (pour1 uses 2) |
| `seed` | 0 | mixed with the document seed and the node name |
| `loop` | document | a periodic buffer (see below) |

**Loop.** With `loop: true` (or in a `kind: loop` document) the pitch-jump
period is rounded to a whole number of ticks in the duration, the frequency
and loudness state run twice over the same ticks and the second pass is kept
(so the state at the end is the state at the start), and the frequency is
nudged by at most `0.5 / duration` Hz so the phase makes a whole number of
cycles over the buffer. `verify` `loop_seam` passes with no `crossfade_loop`.

## `pour`

`POUREDLIQUIDS/pour1.pd`: the fill button starts, over `fill_time` (9 s in the
patch):

- a `cavitylength` line 0 -> 1 -> `100 + 2000 x` Hz, into `openpipe`: four
  `bp~` (Q 30) at 1, 3, 5, 7 x it, summed, fed by two waterflows and the
  bubble factory x 0.05;
- the waterflows' `depth` falls `600 + 300 (1 - x)` (900 -> 600 Hz); their
  `rate` is 4 ms, and after the fill goes to 28 ms over 0.8 s;
- `bubblefactory`: every 20 ms a 29 % gate (`random 100 > 70`) starts one of
  four bubble voices (`poly 4 1`, no note-offs, so they take turns). A bubble
  has a size `a` and velocity `v` (random 0..1): a cosine at `800 b (1 + 2 a)
  + 2000 (1 - u^4)` Hz where `b` is the fill line, `u` falls 1 -> 0 over `80 /
  (a + 0.4)` ms; loudness `lop~120(u^4 (0.6 + v/3)) (1.1 - a)`. (The patch
  sends `swap 1` to the `80` branch down two wires into one inlet, so that
  factor is always 80: `base` does not shorten the bubbles.)
- the whole is x `lop~ 0.5` of a level line: 0 -> 1 over 0.3 s, back to 0 over
  1.2 s once the fill ends.

| Field | Default | Meaning |
|---|---|---|
| `fill_time` | 9 | seconds to fill (0.5..60): the sweep, the bubbles' pitch and the stream's thinning follow it |
| `vessel` | 1 | size, 0.25..4: the cavity (1 = 100 -> 2100 Hz), the bubbles and the stream's depth are divided by it; `range` is not |
| `bubbles` | 1 | bubble rate, 0..3 (the 29 % gate x this; 0 is none) |
| `seed` | 0 | |

Deviations: the cavity line is re-tuned every 20 ms like Pd's `line` (so a
short `fill_time` steps visibly); a resonator above 0.4 x sample rate is muted
(Pd's `bp~` breaks above a quarter of the rate; at the patch's 7 x 2100 Hz the
7th partial is above that); a `vessel` of 1 has the book's sweep, which is
linear in frequency, not the 1/(height) of a real air column; `fill_time`
stretches the fill but not the 0.3 s rise or the 1.2 s fall; the document
needs a `duration` of at least `fill_time + 1.5` (the 0.5 Hz low-pass rings
past the 1.2 s fade).

## Measured

Both nodes get a make-up gain (`stream` x 20, `pour` x 12): the patches sit at
about -48 dB rms (the book raises the volume by hand), so the nodes are
usable without an output stage hunting for the level. Measured through
`sound_ops` / `listen` (48 kHz):

- `stream` default (flow 1, depth 800, range 1600, 2 layers), `lufs` -23: peak
  -7.1 dBFS, crest 15.6 dB, pitch 811 Hz, centroid 884 Hz, 100 % of the energy
  at 200 Hz to 2 kHz, 125 fine onsets in 4 s (35 events/s). Raw node: rms
  -22.9 dB, peak 0.40; 200-1500 Hz -23 dB against -79 dB above 3 kHz.
  `depth` 400 / 1600 with `range` 400: spectral peak 441 / 1641 Hz. `flow`
  0.2 / 4 (one layer): rms -34.8 / -7.0 dB. 1 and 4 layers: -22.4 / -22.6 dB.
- `stream` loop (3 s, `kind: loop`, `lufs` -23), `verify seeds: 4`: the wrap
  is a 0.07-0.30 x p99 step on every seed. `loop_seam` also compares the
  spectra of the first and last 0.34 s and failed one of four seeds (seed 2,
  7.3 dB against the 6 dB limit): two windows of a thin random stream differ
  by chance. Use a longer duration, three or more `layers`, or another seed.
- `pour` (fill_time 3, `peak_db` -3, 4.5 s): -21.6 LUFS, crest 20.1 dB,
  swell 1.57 s (the 0.5 Hz low-pass), centroid by tenth 916, 1030, 889, 962,
  991, 1110, 1570, 1860, 1820, 1770 Hz (rising through the fill, then the
  cavity at 2.1 kHz ringing after it); 92 % of the energy at 200 Hz - 2 kHz,
  8 % above. With fill_time 6, centroid per 1.25 s: 939, 883, 965, 1227,
  1402, 1799 Hz. `vessel` 0.5 / 2: centroid over the fill 1804 / 482 Hz.
  `bubbles` 0 / 2: rms -29.2 / -23.7 dB. Level 0.3 s in -62 dB, mid-fill -24
  dB, 2 s after the end -75 dB.

Tests (`it/farnell_liquid.rs`) cover finite and bit-identical renders, seed
differences, depth / flow / layers / vessel / bubbles / fill_time, field
errors, and the loop.
