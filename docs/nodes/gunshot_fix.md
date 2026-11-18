# gunshot_fix: suppressed gunshot and distance darkening

Batch 2 labels: suppressed gunshots heard as "hitting a metal sheet" and
"lo-fi slam, no high end or detail". Causes found in `blast.rs`: the
suppressed path kept the falling sine thump (350 Hz at 9 mm) while its noise
was cut by 28 dB and darkened (body 0.3x, tail 1 kHz), so a pitched tone
dominated; and the air low-pass `16 kHz / (1 + d / 40 m)` was 7.1 kHz at
50 m and 4.0 kHz at 120 m (12 dB/oct).

Changes:
- `suppressed: true` is a new model: no sine thump; a gas puff of white
  noise band-passed 200 Hz - 4 kHz (`4 kHz * (caliber/9)^-0.2`), 0.4 ms
  attack, 18 ms * sqrt(caliber/9) decay; the action: two first-differenced
  noise clicks (0.5 ms decay, an echo 1-2 ms later at 0.4), the clack at
  5-8 ms and the return at 35-55 ms (seeded). Level unchanged (gain 0.04).
- `gunshot` air low-pass scale 40 m -> 400 m (14.2 kHz at 50 m, 12.3 kHz at
  120 m, 4.4 kHz at 1 km). `explosion` keeps 40 m.

Measured (`tests/gunshot_fix.rs`, 9 mm, seed 1, shares against the whole):

| | before | after |
|---|---|---|
| suppressed centroid | 1759 Hz | 6578 Hz |
| suppressed flatness | 0.127 | 0.543 |
| under 150 Hz | -7.0 dB | -21.3 dB |
| 4-12 kHz | -13.3 dB | -4.8 dB |
| 8-12 kHz | -19.0 dB | -9.5 dB |
| suppressed 4-12 kHz at 50 / 120 m | -15.8 / (lower) dB | -4.4 / -4.5 dB |
| open 4-12 kHz at 50 / 120 m | -11.9 / -20.2 dB | -8.7 / -8.9 dB |

Suppressed peak still 32 dB under the open shot, crest 30.2 dB (open 32.0).
`tests/blast.rs`: the open-shot centroid step 100 -> 400 m is now 0.81x, so
that assertion went from 0.8 to 0.85. Not heard: the centroid at 0 m is high
(the clicks carry much of the energy); if it reads as clicks over a puff,
lower the click levels (0.15 / 0.1) in `suppressed_dry`.
