# `bell`, `ball_bounce`, `cricket`, `twang`: ported from Farnell's Pd patches

Four nodes built straight from the companion Pure Data patches of Andy
Farnell's *Designing Sound*, then compared with those patches rendered by
real Pd. Code: `crates/sfx_graph/src/nodes/farnell.rs`. Tests:
`crates/sfx_analysis/tests/it/farnell.rs`.

## Method

1. Dump the patch as a netlist (every box and its inputs) and port it box
   for box. Pd's filters are copied exactly: `bp~` (a two-pole resonator,
   centre gain not 1, its cosine 0 above a quarter of the sample rate),
   `hip~` before 0.44 (no gain normalisation), `lop~` (y += 2 pi f / sr (x -
   y)), `vcf~` (a complex one-pole).
2. Render the patch with Pd 0.57 in batch mode with `-compatibility 0.41`
   and Pd-extended's `pow~` (right ^ left), at 48 kHz, the top-level `dac~`
   swapped for `writesf~` (see AGENTS.md, "Porting Farnell's Designing Sound
   patches"). The bell is an abstraction: a host patch sets 750 Hz and
   strikes at 0.05 and 1.55 s. The ball needs its start message banged, the
   twang its trigger.
3. `listen reference` the node against the render.

## Measured (STFT distance to the Pd render, 0 = identical)

| Node | Distance | Biggest differences |
|---|---|---|
| `cricket` | 1.90 dB | none over 3 dB |
| `bell` | 3.57 dB | 45-177 Hz 6-7 dB quieter (the 375 Hz sub and the body loop's low end) |
| `twang` | 4.80 dB | none listed |
| `ball_bounce` | 6.54 dB | none listed |

About 10 dB is the spread between real recordings of one event, so all four
sit closer to the book than one real recording sits to another. None of
them has been heard yet.

## Notes per patch

- **Bell**: Pd's `osc~` is a cosine; every strike restarts all five line
  envelopes; the 16x and 24x strike partials feed the body loop with the
  noise. The partials above Nyquist (24 x 750 = 18 kHz is fine at 48 kHz)
  are skipped.
- **Ball**: the patch's `pow~` has e on the left and the bounce line on the
  right, so in Pd-extended it is v^e. The metro interval is read when each
  bounce fires.
- **Cricket**: `ring` (default 0.3, the listener's pick over the book's 1)
  scales both Qs; the 1.9 dB above is at ring 1. The `vline~` message `0, $1 0.1 0, 0 0 0.1` is a 0.1 ms ramp
  to the pulse height then a jump to 0.
- **Twang**: the `pow~` has 15 on the left, so the push is envelope^15. The
  phasor runs backwards at freq / 4 and is reset at the pluck; its line is
  the loudest (107 Hz at 426).

## Round 2: `siren`, `door_creak`, `jet`, `red_alert`

Rendered in Pd with the siren's speed message (3), the door's force slider
at 0.6, the jet's speed slider at 0.8 and the alert's button pressed.

| Node | Distance | Patch |
|---|---|---|
| `red_alert` | 4.24 dB | REDALERT/TOS_redalert.pd |
| `siren` (echo 1) | 6.06 dB | POLICE/police3.pd + logosc~, plastichorn~, environment |
| `jet` | 6.31 dB | JETENGINE/jetengine1.pd |
| `door_creak` | 7.99 dB | CREAKING/doorcreaker.pd + dfbef (random tick timing in both) |

Feedback loops written with `s~` / `r~` (dfbe, dfbef, the alert's 61 ms
loop, the siren's echoes) run one 64-sample block late in Pd; the ports add
that block. `police00.pd`, `police01.pd` and `police1.pd` are waveform
studies with no output; `police3.pd` is the siren.
