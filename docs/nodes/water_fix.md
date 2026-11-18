# water_fix: water hiss and splash after the batch-2 blind labels

Labels (design/labels/batch2_findings.md): `water` heard as "a stream with wind
noise" (bubbles fine); `splash` 3/3 bad: "no idea", "drop kick", "weird
crunchies". Not heard after the fix; these are the measured causes and what
moved. Tests: `crates/sfx_analysis/tests/water_fix.rs`.

## splash

Cause of the kick: the main cavity bubble (radius `0.12 * size`, amplitude 1,
the peak) was a loud low sine born late: at size 0.15, 95 % of the energy sat
under 250 Hz, peak at 0.28 s, voiced 0.65; at 0.3 / 0.6, 99.9 / 100 %.

Now: a 1-6 kHz slap (white noise, 4th-order hp 1 kHz and lp `6 kHz * (0.05 /
size)^0.1` clamped 3-8 kHz, tau 2.5 ms + 1.5 ms sqrt(size/0.05), at most
10 ms, amplitude `1.5 * burst * max(1, size/0.05)^0.25`), then a dense swarm
(`40 + 80 sqrt(size/0.05)` bubbles, 20..400, density `exp(-4 t / W)`, radii up
to `min(0.12 size, 6 mm)`, i.e. nothing under 550 Hz, amplitude `0.25 sqrt(r /
r_top) exp(-t / W)`, rising chirp as `water`'s), a main bubble at 0.25, and
spray (noise 3-8 kHz, 0.12 x burst, tau W / 6).

| size | <250 Hz share before / after | first 20 ms in 1-6 kHz | voiced | sample peak |
|---|---|---|---|---|
| 0.01 | 0.000 / 0.000 | 0.67 / 0.94 | 0.62 / 0.17 | 0.120 / 0.004 s |
| 0.05 | 0.002 / 0.002 | 0.51 / 0.96 | 0.65 / 0.36 | 0.190 / 0.003 s |
| 0.15 | 0.949 / 0.002 | 0.53 / 0.95 | 0.65 / 0.23 | 0.280 / 0.003 s |
| 0.3 | 0.999 / 0.002 | 0.29 / 0.93 | 0.63 / 0.18 | 0.360 / 0.004 s |
| 0.6 | 1.000 / 0.002 | 0.32 / 0.97 | 0.67 / 0.12 | 0.480 / 0.003 s |

(before: "peak" is listen's loudest-window time; the cavity bubble.) Loudness:
-24 LUFS is met at 0.02 / 0.1 / 0.5 m; -20 at 0.1 and 0.5, the 2 cm pebble
lands at -22.3 (crest 33 dB: use `peak_db`). Centroid 0.02 / 0.1 / 0.5 m:
7.9 / 2.7 / 0.9 kHz, still falling through each sound (by thirds).

## water

Cause of the wind: a steady broadband noise bed (white, hp 400 Hz, rms 0.03 x
flow) whose 400 Hz-1.5 kHz part and constant level read as wind. Now the hiss
gain follows the bubbles' envelope (2 ms up / 25 ms down, divided by its
0.4 s local mean, wrapped in a loop; 10 % steady floor), a dense stream's hiss
is high-passed again at 1.5 kHz (4th order) and at 0.6 of the level. Under 20
bubbles a second the hiss is the old steady one (it fades between 20 and 80
bubbles a second): a trickle with a gated hiss is mostly silence, and a
loop's two ends then differ (the old hiss is what kept `loop_seam` passing).

Envelope movement above 3 kHz (cv of the 10 ms rms, before / after): flow 0.5
density 300 0.254 / 0.408; flow 1 0.060 / 0.197; flow 0.8 density 600 0.112 /
0.185. `loop_seam` still 0 of 240 plain loops; with hp + lp + crossfade 0 of
160; hp + lp without crossfade fails 94 of 240 (was 35 %: put
`crossfade_loop` last).

## Tests updated

- `water.rs`: centroid by tenth now checked by thirds (the denser random swarm
  wiggles, up to 4 reversals); "bright" burst is the share above 2 kHz (> 15 %)
  instead of above 8 kHz; size centroid ratio 0.02 / 0.1 is 2.9x (pin 2.5x
  from 3x).
- `r4d.rs`: the slap is the loudest instant from burst 1, so burst 1 and 2
  both peak at 0.5 and a louder burst shows as a quieter rest; tail 2 puts
  4 fine onsets after 0.2 s (default 1; the old cloud 10+).
