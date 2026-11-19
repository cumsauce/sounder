# Vehicles from Farnell's patches: `helicopter`, `rocket`

Ported from the `.pd` files of *Designing Sound* (read as netlists, not from
the book's photos; see `docs/porting-farnell.md`). Code:
`crates/sfx_graph/src/nodes/farnell_vehicles.rs`, tests:
`crates/sfx_analysis/tests/it/farnell_vehicles.rs`. Pd is not installed here,
so neither node has been compared with a real Pd render: the numbers below
are what the node measures, not a distance to the patch. Neither has been
heard. Both run until the document ends, so give them a `duration`.

## `helicopter`

From HELICOPTER/heli1.pd with `mainrotor.pd`, `mdel.pd` and `vrdel.pd`.

| Field | Default | Meaning |
|---|---|---|
| `rotor_rate` | 14.2 | main-rotor blade passes a second (signal, clamped 2..40); the book's period slider (70 ms) |
| `engine` | 0.7 | engine speed 0..1 (signal); the pulse runs at 1 + 90 x speed Hz through a 0.6 Hz low-pass, the gear whine and tail rotor follow it |
| `tail` | 0.44 | tail-rotor level 0..1 (0 is off) |
| `distance` | 0 | 0..1 (signal): 3 one-poles at 6000 (1 - d) + 50 Hz and a ground echo of 2 + 3 d ms at 0.5 |
| `seed` | 0 | the three noise sources share one stream |

What the patch does, as built:

- **engine**: a phasor `p` into `1 / (1 + (98 p^2 (1 - p))^2)` (a narrow pulse
  at each wrap), times `1 + x` with `x` = noise through `bp~` 500 Q 5, x 12,
  clipped +-0.5, x 0.15; `hip~` 30; then a feedback network of four delays
  (`e1a`, `e2a`, `e1b`, `e2b`): `e1a` takes the pulse plus 0.6 x the last
  delay's output, each stage's output is shaped by `0.17 (x + sin(2 pi x / 8))`
  and the two shaped sums, low-passed at 200 Hz and high-passed at 12 Hz, are
  added back into the second and fourth delays; a `bp~` 100 Q 2 sits between
  the second and third. The output is the last delay through two `hip~` 200.
- **gears**: three sines at 1.45, 2.81 and 9.41 x (engine Hz x 14), levels
  0.2, 0.33, 0.14, then x 0.0159 (the gear-mix slider), so about -36 dB under
  the engine mix.
- **tail rotor**: `osc~` at 1.17 x the engine Hz; its cosine squared sets an
  amplitude `1 / (1 + 2.25 cos^2)` (twice a turn: 150 Hz at the default); the
  sum of 0.1 x that and 0.3 x that x a `vcf~` (Q 6) over noise, centre
  100 + 500 cos Hz.
- **main rotor**: a ramp 0 to 1 over a third of the blade period, then 0
  (the `vline~`), bent into a pulse: `tent = min((1 - s) v, s (1 - v))`
  with skew `s` = 0.438, `sin(pi / 2 x k x tent)`. That blade pulse `c` sets
  a `vcf~` (Q 5.7) centre of 6.35 + 295 c Hz over noise (the patch's level
  corrections for frequency and Q included), and three `mdel`s in series: each
  adds 0.7 (0.5 for the third) of a copy low-passed at 400 Hz and delayed
  25 ms + 43 / 52 / 57 x c ms; the second works on the first's output x c;
  the output is the sum of all three.
- **bus**: 0.286 engine + gears + `tail` x tail + rotor, then `vrdel` (3 kHz
  one-pole, delay 25 ms + 0.2 x c, so each blade pass bends the pitch a few
  percent), then `distance`.

Deviations:

- Every `vd~` inside the engine reads 64 samples back (one Pd block). The
  patch writes `vd~ e1a 8` etc. with a millisecond argument, but `vd~` takes
  only a name and its delay input is unconnected there, so Pd reads at its
  minimum, one block. The `s~` / `r~` hops are taken as having no extra
  delay. If the patch's author meant 8, 9, 3 and 6 ms the engine would sit
  lower and ring longer; that is a one-line change in the node.
- `vd~` is read with linear interpolation (Pd's has four points).
- A 20 Hz one-pole high-pass at the very end: the tail-rotor term
  `0.1 / (1 + 2.25 cos^2)` carries DC that Pd passes to the `dac~`.
- The book's mix and speed sliders are fixed at the values saved in the
  patch (engine-mix 0.286, gear-mix 0.016, gear ratio 14, tail speed 1.17,
  rotor-mix 1); `tail` is the only level exposed.

Measured (default node, 3 s, then `peak_db` -3): -21.8 LUFS, centroid 286 Hz,
61 % of the energy under 200 Hz, 38 % at 200 Hz-2 kHz, pitch line 127 Hz
(twice the 64 Hz engine), blade onsets every 64-75 ms. Raw (no output stage)
RMS -25 dB, peak -6 dB. The engine part alone (render at `engine` 0.9 minus
the render at 0) is 10 dB under the rotor and has lines on the harmonics of
its 82 Hz pulse, 67 dB above the spectrum between them. `distance` 0.9 takes
800 Hz-8 kHz down by 16 dB.

## `rocket`

From ROCKETLAUNCHER/rocket.pd, the `rocket` sub-patch only: the launcher's
`chamber`, `reload`, `handling`, `exit`, `backpressure` and `firestate` are
not ported.

| Field | Default | Meaning |
|---|---|---|
| `thrust` | the patch's curve | 0..1 signal `T`; a rise raises the pipe's resonance |
| `ignition` | 0.02 | seconds; silent before it (the patch's gate opens on `fire`) |
| `burn` | 8 | seconds the built-in thrust takes to fall from 1 to 0 after its 90 ms rise; ignored when `thrust` is given |
| `seed` | 0 | |

Chain: white noise -> `notchbands` (`hip~` x2 at `fc` = 900 T^145 Hz plus
`lop~` x2 at `fc - 230`: with T under 0.97 the high-pass is open and the
low-pass shut, so only the first 0.3 s of the burn is notched) -> three delay
taps at 10 + D, 10 + 1.2 D and 10 + 1.4 D ms with D = 4 (1 - T) -> x 3,
clipped +-1 -> `pipe`: four `bp~` (Q 4) at 1, 3, 5, 7 x L, L = 40 + 800
(T^30 + 0.3 T^4) Hz -> x lowpass-1-Hz(sqrt T) -> x the exhaust jitter, a gain
`(1 - k) + k n` with `k` = (0.8 t / 8 s)^0.25 from ignition and `n` the noise
low-passed twice at 9 Hz and x 30 (slow bipolar flutter, about 0.2 rms) ->
x 0.4. `T` is held for 20 ms at a time, as Pd's `line` outputs it, and the
filters keep their state when retuned.

The pipe's resonance goes from 40 Hz (T = 0) to about 1 kHz (T = 1), so the
default burn is a downward sweep of the roar.

Measured (default, 4 s, `peak_db` -3): centroid 1.78 kHz in the first tenth
falling to 0.11 kHz by 0.8 of the length (about the 55 Hz pipe); -17.4 LUFS;
rises to its peak in 0.5 s (the 1 Hz smoothing of sqrt T). `thrust` 1 against
0.5: 900-1300 Hz is 27 dB higher, and the low-minus-high tilt moves 45 dB.
Raw RMS -21 dB, peak -5 dB.

Deviations: the notch's `hip~` / `lop~` cutoffs and the delay taps are updated
every 20 ms with `T`, so a fast `thrust` signal is sampled that way too. At
T = 1 the pipe's top bands (5.4 and 7.5 kHz) carry most of the energy
(centroid 5.6 kHz), because Pd's `bp~` passes more noise the wider its band:
that is the patch, not a fault of the port. No Pd render to compare against.
