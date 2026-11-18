# machines_fix: engine and motor after the batch 2 blind listen

Labels: engine 2 / 4 cylinders "digital riser", "motorcycle revving, really low
resolution"; 6 cylinders "racecar, a bit trombonish"; motor "artificial /
lo-fi", "digital". No field was added; defaults changed.

## engine

- Timing jitter per firing 2 % -> 4 % of the period; level spread
  0.04 + 0.12 (1 - load) -> 0.10 + 0.15 (1 - load).
- Each firing adds a combustion noise burst (white noise, time constant a
  quarter of the pulse width, at least 0.6 ms, level drawn per firing),
  band-passed at 900 + 1800 x load Hz, into the pipe and the output.
- A valve-train clatter: noise high-passed at 3 kHz, gated by the pulses.
- Exhaust pipe softened: feedback 0.6 -> 0.4, loop low-pass 2.5 -> 1.5 kHz.
- Minimum pulse width 4 -> 8 samples; the saturated sum is low-passed at
  0.36 x the sample rate.

Measured (`tests/machines_fix.rs`, 3000 rpm, seed 1, cylinders 2 / 4 / 6):

| | before | after |
|---|---|---|
| off-grid energy (firing grid) | -12.2 / -8.8 / -5.3 dB | -7.5 / -3.4 / 0.6 dB |
| cycle-to-cycle rms cv | 7.8 / 4.5 / 6.0 % | 11.0 / 8.8 / 13.5 % |
| loudest third-octave over median (50 Hz-2 kHz) | 12.3 / 8.5 / 10.2 dB | 9.9 / 4.6 / 5.3 dB |
| 2-8 kHz share | -34.8 / -22.8 / -10.8 dB | -25.1 / -19.7 / -9.1 dB |

Energy above 18 kHz on a 1000 -> 9000 rpm full-load rev: -47.3 dB before,
-43.9 dB after (now mostly the broadband noise, not folded pulse edges).
Pitch still within 0.02 % of the firing rate. The exhaust echo is weaker:
`machines.rs` now smooths 0.5 ms and searches 0.6-1.4 round trips; it read
9.02 / 13.12 / 18.65 ms for 9.33 / 13.99 / 18.66 ms.

## motor

- Speed drift of about +-0.4 % (noise low-passed twice at 2 Hz) plus a
  0.15 % once-a-turn eccentricity; every line follows it.
- Brush noise: hiss 0.05 -> 0.10, modulated by spark clicks at twice the
  electrical rate (random height, 0.3 ms decay).
- Bearing noise: band-pass at 2.5 kHz, level 0.08, rising once a turn.
- The PWM carrier was and is a sine with sine ripple: no aliasing from it.

Measured (3000 rpm, 20 teeth, seed 1): off-grid energy (50 Hz grid) -26.8 ->
-16.5 dB; mesh-line spread over 0.25 s windows 0.00 -> 3.25 Hz; flatness
0.003 -> 0.005. Mesh and hum still within 0.02 % of rpm.

Limits: none of this has been heard; the numbers show the lines are less
clean and the pipe less resonant, not that it reads as an engine.
