# `droid`, `transporter`, `torpedo`, `phaser_beam`: generic sci-fi sounds from Farnell's Pd patches

Four nodes ported from the Pure Data patches of Andy Farnell's *Designing
Sound*, read as netlists (see `docs/porting-farnell.md`). Code:
`crates/sfx_graph/src/nodes/farnell_scifi.rs`. Tests:
`crates/sfx_analysis/tests/it/farnell_scifi.rs`. None has been heard, and
none was compared with a Pd render (Pd is not installed here): the checks
are measured properties of what the patch implies. The patches run until
stopped, so each node takes a `length`. Nodes are mono.

| Node | Patch | Fields |
|---|---|---|
| `droid` | R2D2/droids3.pd (FM core: droids2.pd) | `chatter` 0..1 (0.9), `pitch` 0.25..4 (1), `mood` 0..1 (0.5), `length` s (2), `seed` |
| `transporter` | SCIFI/transporter-TNG.pd | `length` 1..30 s (12), `pitch` 0.5..2 (1), `chime` 0..2 (1), `reverb` 0..2 (1), `seed` |
| `torpedo` | SCIFI/photon-torpedo.pd | `pitch` Hz (2700), `sweep` s (5), `ring` 0.5..0.995 (0.98), `reverb` 0..2 (1), `length` s (8) |
| `phaser_beam` | SCIFI/tos-phaser.pd | `length` s (1), `mode` `kill` / `stun` (kill), `pitch` 0.25..4 (1), `seed` |

(`phaser_beam`, not `phaser`, which is the modulation effect.)

## `droid`

A `metro` ticks every `period` ms (the `talk-speed` slider: 100 + 600 x,
the patch's saved value is 138 ms). Each tick re-rolls, each with its own
chance and with a glide of either 0 or one period (`random 2`):

- the phasor rate F: `random 1000 / 1000`, scaled by 0.6, so 0 to 0.6 Hz;
  every tick;
- the carrier factor K: 100 + `random 9000` (1 in 3): the cos~ argument is
  K x phasor, so the carrier is F x K, up to about 5.5 kHz;
- the modulator: (20 + `random 800`) x F Hz (1 in 4), and the index
  0.2 + `random 100 / 200` cycles (1 in 4);
- the amplitude, a 0 or 1 line with a 3 ms or one-period slide (1 in 2);
- the tick interval: halved with probability 1 in 3 (`random 3`, `random 2`).

The voice is `cos~(K phasor + index osc~(mod))` times the amplitude, through
`lop~ 10000` and `hip~ 100` twice each, times 0.06. A new line starts from
where the old one is (Pd's `line~`). `chatter` sets the period (default 0.9
= 160 ms), `pitch` multiplies F (so every frequency), `mood` scales the odds
of the 1-in-3 and 1-in-4 rolls (1 in 3 and 1 in 4 at 0.5) and the index span
(x 2 mood). At `mood` 0 the carrier, modulator and index never move after the
first tick: a calm tone. The same seed repeats the babble; `pitch` leaves
the decisions untouched.

Measured (`droid` defaults, 3 s): leading silence 0.3 s (the amplitude line
starts at 0, as in the patch), whole-sound centroid 0.58 kHz with chirps to
2 kHz, voiced 62 %, peak under 0.12 before the output stage. `chatter` 1
gives 26 onsets in 6 s, `chatter` 0 gives 4; `pitch` 2 against 0.5 moves the
centroid from 3.1 to 8.1 kHz; `mood` 0 against 1 widens the occupied band
from 47 Hz to 3 kHz.

## `transporter`

- Two squares multiply: 467.1 Hz (the patch builds it from `phasor~` and two
  `wrap~`) and a second whose `phasor~` frequency is 277 + 6500 x the first,
  so it hops between 3527 and -2973 Hz in step.
- That feeds a delay line (120 ms) with feedback `0.7 x 0.3 x` the sum of
  three `vd~` taps, 12 ms apart. The first tap's time is 1 + 12 (1 - v) ms,
  v a `vline~` from 0 to 1 over `length` (12 s): the comb slides.
- The taps pass a triangle swell 4 min(v, 1 - v) and `hip~ 100`, then twelve
  `bp~` at 466.164 x 1.12247^k Hz, k = 0..11, Q 1200, 1100, 930, 910, 880,
  840, 800, 760, 720, 680, 640, 620 (centre and Q as `bp~ 1 Q` with the
  note in the middle inlet), times 12, then `rev3~ 100 95 5000 1`. **The
  shimmer reaches the output only through the reverb**, so `reverb` 0 mutes
  it.
- The `activate` abstraction runs at once: a saw (unipolar, as `phasor~`) at
  775 + 120 (1 - a) Hz, a the 10 ms-attack, 5 s-decay envelope, with 30 Hz
  vibrato of 40 Hz and 0.1 of noise, times a 30 Hz low-passed copy of a,
  plus a noise burst that falls to 0 over 120 ms; x 3, clipped at 1, through
  three `bp~` (775, 1550, 2325 Hz, Q 13) x 0.5. It goes out at 0.5 and into
  the reverb at 0.1.

Deviations: `rev3~` is Pd's own C reverb; the node has a small Schroeder
reverb (four damped combs of 30-44 ms, feedback 0.93, two allpasses) instead,
so the tail differs. Pd's `bp~` passes DC and the reverb piles it up (a mean
of 0.08), so the output has a 20 Hz high-pass. `pitch` scales the notes only.

Measured (`chime` 0, defaults otherwise, `length` 3): peaks of the shimmer sit
on the scale (660, 933, 1178 Hz), level swells 28 dB from the first 0.3 s to
the middle and falls 60 dB after the end. The whistle peaks at 753 Hz
(bp~ 775). Default render, 16 s: -24 LUFS target met, centroid 0.33 kHz
over the whole sound (the whistle's start, the shimmer at 1 kHz after 3 s),
decay -60 dB at 11.5 s.

## `torpedo`

An envelope E jumps to 1, falls to 0 over 90 ms, rises to 0.89 over 60 ms
from 91 ms and falls to 0 at 351 ms. The zap is a cosine at 2700 E^7 Hz
(`pow~` with `sig~ 7` on the left and E on the right: Pd-extended's
right ^ left, so E^7; vanilla 0.42+ would give 7^E and an 18 kHz top) times
E. It feeds a delay line (1000 ms) with feedback `0.98 x lop~ 10000
(hip~ 23 (tap))`, the tap at 1.2 + 240 v ms, v a line from 0 to 1 over 5 s.
Because the delay grows, each pass reads slower: the stored zap slides down
until the sound is a 40 Hz rumble. The tap goes out at 0.3 and through
`lop~ 1200` into `cmreverb`, an abstraction: four delays at 79, 79 x 1.618,
79 x 2.618 and 79 / 1.618 ms mixed by a 4 x 4 Hadamard (sums and
differences) at feedback 83 / 200. The patch's output is the sum of the
mono tap and that network (left and right from two of the four lines; the
node sums them to mono).

The patch rings for about a minute (0.98 per pass); `length` cuts it with a
fade over the last quarter (2 s at most). `ring` is the 0.98.

Measured (defaults, 9 s, reverb on): centroid 258 Hz, 60 % of the energy
under 200 Hz, 0-12 ms centroid 1.9 kHz, 50-85 ms 0.46 kHz, then 168 Hz at
0.3 s, 89 Hz at 1.5 s, 47 Hz at 5 s. The sound is mostly low: expect a
`weight`-heavy, falling thump, not a bright zap.

## `phaser_beam`

Four cosine oscillators, each wobbled by a slower one: 1360 +- 80 Hz at
32 Hz and 1390 +- 200 at 28 Hz (x 0.3 together), 2446 +- 443 at 47 Hz and
2586 +- 346 at 36 Hz (x 0.7). White noise through `hip~ 2000` is added. The
sum goes to two `bp~` (2600 and 1300 Hz, Q 7: Pd's low centre gain makes
this quiet), clipped at 1, `hip~ 1200`, then the patch's `kill` (0.4) or
`stun` (0.2) toggle gain; `mode` picks one. The node adds a 3 ms fade in and
a 10 ms fade out so the beam does not click. `pitch` scales every frequency.

Measured (defaults, 1 s): centroid 2.52 kHz, flat over time (the wobble
moves the pitch, not the centroid), 87 % of the energy at 2-8 kHz, nothing
under 600 Hz, `stun` exactly 6.02 dB under `kill`.
