# `rain` (Farnell's RAIN patches)

`{"type": "rain", "surface": "leaf" | "water" | "window" | "ground",
"intensity": drops/s, "seed", "loop"}`. Code: `crates/sfx_graph/src/nodes/farnell_rain.rs`;
tests: `crates/sfx_analysis/tests/it/farnell_rain.rs` (its `println!`s give the
numbers below). Not heard by a listener yet, and not rendered through real Pd
(Pd is not installed): the ports follow the netlists of
`BOOK-PUREDATA/RAIN/*.pd` and reuse the exact Pd filters of `farnell.rs`.

## What each surface is

| `surface` | Patch | What it does |
|---|---|---|
| `leaf` | rain-on-leaf.pd | gaussian noise (`gaussianoise 0.4`: Box-Muller, radius drawn from 0.6..1) through `bp~ 50 0.4` and `lop~ 500` gives a slow random L. A sine at 40 + 80 L Hz times 10 L^2, `max~ 0.35`, minus 0.35, x 0.5, then `hip~ 500` twice. Drops are the bursts where 10 L^2 sine passes the threshold: ticks, nothing under 200 Hz. |
| `water` | rainonwater.pd | the same drop generator (`raindrops 20 0.1`) feeds `threshold~ 0.01`; each upward crossing draws r = 0..999 and starts one bubble: length r / 7 ms, base 400 + r / 5 Hz, a pitch rise of (2 + r / 100)^ramp (so up to x12), level r / 1000, envelope (1 - ramp)^4 through `lop~ 12`, `lop~ 1000`, x 0.1. One voice: a new drop cuts the last. |
| `window` | rainwindow.pd + glasswindow.pd | `raindrops 70 0.2` through `hip~ 9000` and x 10 into two delay lines (3.7 / 4.2 ms) that feed each other through four `bp~` (Q 2.3, 2007 / 1994 / 1986 / 1969 Hz) ringmodulated by `osc~` 254 / 669 / 443 / 551 Hz, each read at 0.61; the output is the two delayed reads. |
| `ground` | rain1.pd + cpraingen.pd + cpulse.pd | three streams; each waits 40 + exp((r + 7) / 15 + 2.718) ms (r random below 60 / 50 / 70, so 64 ms to 2.6 s) then fires a parabolic hump 0.15..0.51 ms long of height sqrt(length + 0.1); sum x 0.2, `hip~ 900`. |

`windywindow.pd`, `spikerain.pd`, `drops.pd`, `dropsig.pd`, `rainforwater.pd`,
`comborain.pd` and `glasswindow.pd` on its own were read but not made
surfaces (windywindow and spikerain are wind and a spiky variant; the rest
are parts of the four above).

## Fields

- `surface` (default `leaf`).
- `intensity`, drops a second, 0.1 to 500. Unset: the patch exactly as drawn
  (measured below). Set: for `leaf`, `water` and `window` the `max~`
  threshold is moved (bisection over the rendered buffer) until that many runs
  a second pass it, so the patch's character is kept and only the rate
  changes; `ground` stretches all three streams' waits by one factor.
- `seed`, `loop` as `fire`: in a `kind: loop` document (or `loop: true`) the
  filters run twice and ground clicks wrap, so `loop_seam` passes.

## Measured (10 s, seed 1, 48 kHz; levels are the patch's, not normalised)

| surface | unset rate | peak / rms dB | centroid |
|---|---|---|---|
| leaf | 2.3-2.7 drops/s | -30.7 / -64.6 | 595 Hz |
| water | 3 drops/s | -27.6 / -48.4 | 824 Hz |
| window | rings (counter reads 249/s) | -19.4 / -44.4 | 795 Hz |
| ground | 10.2 drops/s (analytic mean of the three: 8.95) | -20.8 / -50.3 | 1968 Hz |

- `intensity` 4 / 12 / 30 gives 4.2 / 11.7 / 30.9 drops/s on the leaf and
  4.6 / 34.9 on ground at 4 / 30 (the envelope counter, which overcounts
  a few percent).
- Limits: `leaf` and `window` follow intensity until the threshold reaches 0
  (the sine's positive half cycles at 40-120 Hz, about 100 a second); `water`
  stops near 8 counted drops a second because a drop needs 10 L^2 above 0.02.
  The output is quiet (the patch is): put `peak_db` or `lufs` on the output.
- Water pitch: a drop that starts at 803 Hz is at 1445 Hz 30-80 ms later.
- Window: 1-3 kHz is 18 dB over >8 kHz; it stays below peak 0.5 at 10 drops/s.
- Loop: wrap discontinuity 0.00-0.09x, spectral change 2.3-4.8 dB (max 6) in a
  4 s loop at intensity 12.

## Deviations

- `pow~` in rainonwater.pd is read right ^ left (Pd-extended), as in
  `docs/porting-farnell.md`: the pitch is rise^ramp. Read as vanilla (ramp^rise)
  it would sweep 0 up to 400-600 Hz.
- Pd's blocks, `threshold~` hysteresis and `delread~` rounding are
  approximated: a drop is a rising edge of the thresholded signal with 1 ms
  debounce; delays are rounded to whole samples.
- The `$9..$12` arguments of glasswindow are written `3.74.2 0.61 2.3` in
  rainwindow.pd, which is `3.7 4.2 0.61 2.3` with the space lost; windywindow
  shows the same fault.
- `intensity` and `loop` are additions; the patches are free-running.
- Phase of the 40-120 Hz sine is not wrapped for loops, so a drop straddling
  the seam can have a small step (not seen by `loop_seam`).
