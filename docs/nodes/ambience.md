# Ambience: `leaves`, `underwater`

Two beds ported from the foley project (MIT / Apache-2.0). Code:
`crates/sfx_graph/src/nodes/ambience.rs`; tests:
`crates/sfx_analysis/tests/it/ambience.rs`. Neither has been heard yet.

## `leaves`

Wind through foliage. After `foley-synth/src/plants.rs` `brush`: a body
pushing through plants, a swish of stems (band-passed noise times a 20 Hz
`Wobble` of depth 3, clamped 0.4..1.6) plus ticks (a Poisson stream of
flicks, each adding 0.3-1.0 to a decaying envelope that gates a band of
noise). Foley's constants per plant are kept:

| `foliage` | swish band, Q | tick band | ticks/s at full push | tick decay (per sample at 48 kHz) | swish level |
|---|---|---|---|---|---|
| `grass` (`reeds`, `fern`) | 3.5 kHz, 0.8 | 5.2 kHz | 450 | 0.94 | 0.5 |
| `leaves` (`tree`, `bush`) | 1.5 kHz, 0.7 | 2.3 kHz | 150 | 0.985 | 0.7 |
| `dry` (`stalks`, `crops`) | 4.2 kHz, 1.0 | 6.5 kHz | 650 | 0.9 | 0.2 |

Fields: `wind` (m/s, 0..40, number or signal), `foliage`, `gust` (0..1,
default 0.3), `seed`, `loop`.

Deviations from foley: the brush has a fixed 0.3 s swell shape (up 30 %,
down 70 %); here that shape is the wind, `(wind / 8 m/s)^1.5` times a slow
(0.4 Hz) gust swing, so it never ends and follows a signal. Output is not
normalized to 0.3 peak; a fixed 0.22 scale stands in (calm wind is quiet).
Ticks are also scaled by `min(shape, 1)` so a light breeze is a swish with
few ticks. Zero wind is exact silence. A loop folds a 0.5 s tail onto the
start with an equal-power crossfade.

Use under `wind` (the air bed) and beside `cloth` (cloth-like rustle bursts);
`leaves` is the foliage's own tick-and-swish, not a copy of either.

Measured (48 kHz, 3 s, wind 8, gust 0.3): grass centroid 5.6 kHz, peak
-14.9 dB, ~250 onsets/s; leaves 3.3 kHz, -13.3 dB, ~220/s; dry 6.6 kHz,
-15.3 dB (swish level is only 0.2, rms -36.7 dB). RMS at gust 0: wind 2 =
0.0039, 8 = 0.033, 20 = 0.127. Loops pass `loop_seam` on grass / leaves / dry.

## `underwater`

After `foley-dsp/src/underwater.rs`: bubbles rising past the ear over a 50 Hz
pressure sine. Bubbles are foley's `Bubble` (Minnaert, r^1.5 loudness, radius
log-uniform 0.5-6 mm, loudness `0.02 * (0.5 + u)`), at most 64 ringing, a
Poisson stream at `level * (4 + 16 * level)` a second; the pressure tone is
`0.25 * level * sin`, and the whole is multiplied by `level`.

Fields: `depth` (m, 0..100, number or signal), `bubbles` (rate multiplier,
0..4, default 1), `pressure` (tone level multiplier, 0..2, default 1), `seed`,
`loop`.

Deviations: foley's `level` is a controller's 0..1 submersion; here it is
`1 - exp(-depth / 1.5)` (0 at the surface = silence, 3 m 0.86), glided with
foley's 0.3 s time constant (a start at depth begins at full level; a `ramp`
dives). `bubbles` and `pressure` are new scalers. The first bubble is drawn
like the others (foley's is born at sample 0) so a loop has no onset at its
seam. A loop makes the tone a whole number of cycles (50 Hz nudged to the
nearest) and folds bubble ringing past the end onto the start (no births in
the tail).

Measured (depth 6): rms 0.17, almost all of it the tone (rms 0.170 alone);
the bubbles hold 2.8 % of the energy above 200 Hz (tone alone 0.000 %). Big
bubbles are loud (a 6 mm one peaks near 0.3) but short, so the centroid
reads 70 Hz against the tone's 50. Loop seams: at the default `bubbles` 1
only 2 of 8 seeds pass `loop_seam` (about 6 bubbles fall in its 0.34 s
spectral window: 5-13 dB apart); at `bubbles` 4 all 8 pass. The step across
the wrap is under 0.5x the 99th-percentile step for every seed.
