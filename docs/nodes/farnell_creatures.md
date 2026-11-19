# `roar`, `fly`, `cicada`: creatures ported from Farnell's Pd patches

Three nodes built from the companion Pure Data patches of Andy Farnell's
*Designing Sound*, read as netlists (every box with its inputs, inlets
ordered by x position as Pd orders them), not from the book's photos. Code:
`crates/sfx_graph/src/nodes/farnell_creatures.rs`. Tests:
`crates/sfx_analysis/tests/it/farnell_creatures.rs`. Pd's `bp~`, `lop~` and
the pre-0.44 `hip~` are copied exactly (as in `farnell.rs`; see
`docs/porting-farnell.md`). Nothing here has been heard by a listener, and
Pd was not available to render the patches, so there is no STFT distance to
the originals: the checks are the numbers the patches imply.

All three read the seed (`cx.rng`); their slow noise wander was tuned at
44.1 kHz and is scaled by `sqrt(sr / 44100)` to keep its size at other rates.

## `roar` (MAMMALS/roar.pd, articulate.pd, vowels.pd)

Fields: `size` (0.2 to 5, default 1), `aggression` (0 to 1, default 0.5),
`vowel` (`"roar"` default, or `i I E ae ^ u U & A`), `length` (seconds,
0.1 to 10, default 1), `at`, `seed`.

What the patch does:

- **articulate.pd** is a `line` that rises 0 to 1 over `duration x risefall`
  ms and falls back to 0 over the rest, shaped `a = 0.75 - 0.5 cos(pi/2 line)`
  (0.25 to 0.75). Everything below follows `a`.
- **Cord model** (`pd cord model`): a phasor at `f0 = base + a x range`
  (60.7 + 65.2 a Hz at the sliders' saved values). The pulse is
  `g = (1 - cos 2 pi p) / 2`, the signal `x = width x g x cos(2 pi (p - 0.5)
  x ripple)` with `ripple = lop10(a x cord-ripple)`, squashed to
  `y = 1 / (1 + x^2)`, mixed `y (1 - noise) + y noise white`, high-passed at
  1 Hz. Width 42.3, ripple 1.86 a, noise 0.268.
- **Throat** (`pd tract comb filter`): eight `bp~` at `base`, `base + s`,
  then each step 1.414 times the last: `base + s x (1, 2.414, 4.414, 7.243,
  11.243, 16.9, 24.9)`, all at Q `res`, summed. The patch drives base, `s` and
  `res` with `a` (base 338 + 180 a Hz, `s` 123.7 + 41.2 a, `res` 44.4 + 25.9 a).
- **Loudness**: `sqrt(2 (max(a, 0.5) - 0.5))`, so the roar is silent until
  the line is two thirds up, swells to a peak where the rise turns to the
  fall, and is gone again when the fall passes two thirds. Then x 0.1 into
  the throat and x 8 out.

The fields:

- `size` divides `f0` and every throat frequency (2 is an octave down, the
  highest comb bands above 0.45 sr are dropped).
- `aggression` moves the sliders, 0.5 being the patch's values: noise
  0.05 / 0.268 / 0.7, cord width 20 / 42 / 90, ripple 0.8 / 1.86 / 4, pitch
  range 20 / 65 / 200 Hz, throat Q 50 / 44 / 36, rise share 0.8 / 0.6 / 0.3
  (a faster attack).
- `vowel` replaces the eight-band comb by vowels.pd's three `bp~` (Q 34) at
  the vowel's formants (the nine message boxes: `i` 280 / 2250 / 2900 ...
  `A` 710 / 1100 / 2640), divided by `size`. vowels.pd's own source (pink
  noise and a phasor-shaped pulse) is not used: the roar's cord model drives
  them.
- `length` is the audible time. The patch's `duration` is three times it,
  and the silent first two thirds of the rise are cut so `at` is where the
  sound starts.

Deviations:

- **Typos in the patch.** Three receivers do not match their sliders:
  `r $0-tracb` (slider `$0-tractb`, tract-base), `r $0-tractlm` (slider
  `$0-traclm`, tract-lengthmod) and `r $0-tractrm` (slider `$0-tracrm`,
  tract-resmod). As shipped, the tract base, length modulation and
  resonance modulation never arrive, so the base is `a x 180` Hz, the
  separation a constant 123.7 and Q a constant 44.4. This port follows the
  sliders' evident intent (base 338 + 180 a, and so on), which puts the first
  formant at 430 to 470 Hz instead of 90 to 135 Hz.
- The first bang of the patch's `del` runs with an unset time (it fires at
  once and cuts the first rise); the port plays the intended rise and fall.
- The level is x 32 rather than x 8 (peak about -6 dBFS at the defaults; the
  output node normalizes anyway).
- `line`'s 20 ms grain is a continuous curve; the throat is retuned every
  10 ms.

Measured (48 kHz, defaults, `peak_db` -3 output): pitch 102 Hz (voiced
94 %), centroid 628 Hz, energy under 200 Hz / 200-2k / 2-8k 18 / 75 / 7 %,
peak at 0.63 s of a 1 s length, leading silence 0.01 s. `size` 0.5 / 2:
centroid 1.38 kHz / 302 Hz, lowest line 103 Hz at size 1 and 53 Hz at 2.
`aggression` 0.1 to 0.9 raises the whole-sound flatness from 0.000 to 0.005
and the centroid from 145 to 1340 Hz. `size` 2.5, `aggression` 0.8: 47 %
under 200 Hz, centroid 321 Hz.

## `fly` (INSECTS/buzzing-housefly.pd, flywing.pd, fly1.pd, flywing-waveform.pd)

Fields: `freq` (wingbeat Hz, 20 to 1000, default 220), `res` (0 to 20,
default 5), `flutter` (0 to 5, default 1), `wave` (`"model"` or `"table"`),
`seed`.

- **Wing** (flywing.pd; fly1.pd is the same patch with number boxes): a
  phasor `p`; `s = 12 min(0.2 p, 1 - p) - 1` is a sawtooth that climbs for
  5/6 of the cycle and drops in 1/6. The negative part `d = min(s, 0)` is the
  downstroke and rings: `d (1 + 0.5 cos(2 pi wrap(res x d)))`; the positive
  part `u = max(s, 0)` is the upstroke, `2 u^4`. The sum goes through
  `hip~ 700`. With `res` 5 the ripple sits near `res x 2.4 x freq` = 2.6 kHz
  and the lines are the harmonics of 220 Hz.
- **Housefly**: two wings summed x 0.1. Wing 1's frequency is 220 plus 700 x
  noise lowpassed twice at 4 Hz (a wander of a few Hz); wing 2 adds 3 x a
  second, 5 Hz-lowpassed noise, and the resonance is 5 + 40 x that same
  noise. The wing inlets: `res` is the left inlet and `freq` the right (Pd
  orders inlets by x position), which is the reading that gives
  220 +- 5 Hz and 5 +- 0.3.
- `wave: "table"` plays flywing-waveform.pd's measured wing (Bennet and Ewing
  1968; 256 points, cubic interpolation as `tabosc4~`) in place of the model:
  no resonance, no high-pass, x 0.5 per wing.
- `flutter` scales all three noise-driven wanders (0 is a steady machine).

Deviation: fly1.pd's saved number boxes (resonance 96 / 100, wing frequency
756) are not used; the housefly patch's values are the defaults.

Measured (48 kHz): with `flutter` 0 the strongest lines are 440, 220, 660,
2860, 2200 and 880 Hz (every one on a harmonic of 220); 2.3 to 3 kHz sits
12 dB above 3.6 to 4.3 kHz; below 120 Hz it is more than 20 dB under the
body. With the default flutter, pitch by tenth moves 217 to 223 Hz (steady:
220.03). Default: pitch 221 Hz (voiced 100 %), centroid 1.37 kHz, shares
1 / 77 / 20 / 2 %, never decays (set a `duration`).

## `cicada` (INSECTS/cicada1.pd, cicada2.pd, cicada3.pd)

`cricket` (cricket2.pd) rings very narrow tones near 4.5 and 9 kHz; the
cicada patches are broadband noise in 5 to 8 kHz shaped by pulse trains, so
they are a node of their own.

Fields: `kind` (`"buzz"` default, `"rattle"`, `"call"`), `pitch` (band
scale, 0.5 to 2, default 1), `rate` (pulse-rate scale, 0.25 to 4, default 1),
`call` (for `"call"`: `cycle` default, `trill`, `chirp`, `tick`), `seed`.

- **buzz** (cicada1): white noise through `hip~ 5000` x2 and `lop~ 8000` x2;
  times `lop~ 70` of `p1^4 + p2^2`, where `p1` is a phasor at -90 (1 + 30 m)
  Hz and `p2` one at -20 (1 + 10 m) Hz (`m` is slow noise, `lop~ 2` x2, so
  the rates wander by about 13 Hz and 1 Hz); `bp~ 6000` Q 7, `hip~ 4000`,
  x 0.2.
- **rattle** (cicada2): a phasor at -120 (1 + 190 y) Hz, where `y` is noise
  through `bp~ 120` Q 40, `hip~ 12000` twice (the sawtooth's reset spikes),
  four `bp~` at 5000 / 6000 / 8000 / 7000 Q 30 summed x 2, times the positive
  half of a 12 Hz cosine, through `bp~ 6000` Q 5. (The patch's `*~ 0` in the
  frequency-wobble path is kept: the 12 Hz gate does not wobble.)
- **call** (cicada3): the band noise through `bp~ 7500` and `bp~ 5500` (Q 40)
  times a 500 Hz cosine, gated by `1 / (1 + (depth cos(2 pi f t))^2)` and
  `hip~ 4000`, x 0.7. The patch's `metro 5000` and `random 3` pick one of
  three (f, depth) pairs every 5 s: `trill` (30 Hz, 3), `chirp` (8 Hz, 6),
  `tick` (0.5 Hz, 128: one pulse a second); `call` fixes one.
- `pitch` multiplies every band centre and high-pass; `rate` every phasor
  and gate rate.

Measured (48 kHz, `peak_db` -3 outputs): `buzz` centroid 8.4 kHz (4-9 kHz
band 41 dB over everything under 2 kHz), 67 / 33 % of the energy at 2-8k / over 8k, 12 %
of the envelope energy at 20-150 Hz; `rattle` centroid 6.5 kHz, 85 % at 2-8k,
21 % roughness; `call` (cycling, seed 1) centroid 7.9 kHz. A `tick` call's RMS sits
35 dB under its peak, a `trill`'s 22 dB. None
of them decays: set a `duration`.
