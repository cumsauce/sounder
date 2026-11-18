# Water, splash

Two sources built from a population of Minnaert bubbles summed in one pass
(`crates/sfx_graph/src/nodes/water.rs`, tests in
`crates/sfx_analysis/tests/it/water.rs`, `examples/water.rs`). The bubble maths
is foley's `Bubble` (the `bubble` node): a sine at `3.29 kHz / radius_mm`,
decaying at `pi f 0.045` a second, rising by `0.1 x` the decay rate a second.
Each bubble is written straight into one buffer: no per-voice graph, so
thousands a second are cheap.

**Level law (why not `bubble`'s).** `bubble` has amplitude `r^1.5`, so energy
(amplitude squared times ring time, which goes as `r`) goes as `r^4`: a few
large bubbles set a population's level and the limiter does the rest. Here a
bubble of radius `r` in a population of median `r_med` has peak amplitude
`A0 * sqrt(r / r_med)`, `A0 = 0.1`. Energy then goes as `r^2` relative to the
median, and a +-1 sigma spread (0.4 in ln r) moves the amplitude about +-22 %.
The stream's level follows the bubble rate and the median size.

## `water`

A running-water source: bubbles born at a rate and with a median size that
follow `flow`, plus band-limited hiss and optional splashes.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `flow` | signal | required | 0 .. 1 | How hard it runs. A number outside 0..1 is an error; a signal is clamped per sample (non-finite counts as 0). 0 is exact silence. Mono. |
| `depth` | number | `0.5` | 0 .. 1 | Median radius x `2^(2 (depth - 0.5))` (0.5x at 0, 2x at 1); hiss low-pass `10 kHz x 0.25^depth` (10 kHz .. 2.5 kHz). |
| `splash` | number | `0` | 0 .. 1 | Splashes mixed in: `splash x flow x 4` a second, sizes 0.02 .. 0.12 m (log-uniform), each at peak 0.3. |
| `density` | number | `300` | 0 .. 3000 bubbles/s | Bubble rate at flow 1. The rate is `density x flow^1.5`. Above 3000 is an error. |
| `loop` | bool | the document's `kind: loop` | | Periodic over the document length (bubble tails and splashes wrap to the start; the hiss filters are primed from the end). `false` forces a one-shot even in a loop document. |
| `seed` | integer | `0` | | Mixed with the document seed and node name. |

Per-bubble model: median radius `0.8 mm x 5^flow x depth_scale` (flow 0: 0.8 mm,
4.1 kHz; flow 1: 4 mm, 820 Hz), each bubble `median x exp(0.4 N(0,1))`
clamped to 0.7 mm (or the radius that rings at 0.4 x sample rate) .. 20 mm
(165 Hz). Hiss: white noise, 400 Hz high-pass, the low-pass above, scaled to
rms `0.03 x flow` (-30.5 dB at flow 1). A 12 Hz high-pass on the bubble sum
removes the DC the sine-phase starts leave at high density.

Caps and errors (a count the caller can set is capped, never clamped):

- `flow: 2: give a number from 0 to 1`
- `density: 5000: give bubbles a second from 0 to 3000 (more is not a stream: lower density or use flow)`
- `density: 3000: this render would create about 1896000 bubbles (limit 800000): lower density, splash or flow, or shorten the sound` (a splash counts as 40 bubbles)

Random; causal unless it loops (the end of the sound rings into the start).

### Recipe

A brook with a wandering flow, a few splashes and one plop at 1.5 s. Give the
human macros on `depth` (size and tone) and `splash`.

```json
[
  {"type": "wander", "name": "level", "rate": 0.4, "range": [0.35, 0.7], "seed": 1},
  {"type": "water", "name": "brook", "flow": "level", "depth": 0.3, "splash": 0.15, "seed": 2},
  {"type": "splash", "name": "plop", "size": 0.12, "at": 1.5, "seed": 5},
  {"type": "gain", "name": "plop_g", "input": "plop", "db": -6},
  {"type": "mix", "name": "scene", "inputs": ["brook", "plop_g"]}
]
```

For a loop: `kind: loop`, a `duration`, a constant `flow` (or one that returns
to its start value), and the node loops by itself.

## `splash`

A single splash (source, one-shot): a bright, brief impact burst, then a
cloud of bubbles that grow with time and end in the main cavity bubble. The
centroid falls through the sound (round 1 redesign; before, the loud main
bubble came first and the smaller, higher cloud after it, so it rose).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `size` | number | required | 0.005 .. 1 m | Splash radius. Main bubble radius `0.12 x size` (1 .. 60 mm). Timeline, with window `W = 0.06 + 0.12 sqrt(size/0.05)` s: the burst starts after 2.5 ms of silence (so the output's 2 ms fade-in does not cut it): white noise low-passed at `16 kHz (0.05/size)^0.1` (13 kHz .. 0.4 sr), time constant `2 ms + 1.5 ms sqrt(size/0.05)`, amplitude 1. Then `10 + 18 sqrt(size/0.05)` bubbles (6 .. 80), one per equal slice of a density falling as `exp(-2.5 t/W)`, radius `0.1 .. 0.8` of the main one growing as `(t/W)^0.6` (ln-sigma 0.08; floored at 0.7 mm), amplitude `0.5 sqrt(r/r_main) exp(-t/W)`. The main bubble is born last, at `W`, amplitude 1. Each bubble fades in over 1 ms (raised cosine) and glides down by `0.03 x its decay rate` a second (water's bubbles glide up by 0.1). |
| `at` | number | `0` | seconds >= 0 | Start time. Cut at the document's end with a 20 ms fade. |
| `seed` | integer | `0` | | Another seed is another splash of the same size. |

The output is scaled to a **peak of 0.5 (-6 dBFS)** whatever the size (as
`thunder` is): set loudness with `gain` or the output's `lufs`. Not causal (a
finished one-shot). Errors:

- `size: 5: give the splash radius in metres from 0.005 to 1 (0.05 is a stone in a pond)`
- `size: 0: give a number > 0`, `at: -1: give a number >= 0`

## Measured

48 kHz; tests in `crates/sfx_analysis/tests/it/water.rs`. Centroid is the power
centroid over 100 Hz .. 16 kHz. "Rises" counts envelope onsets (3x the level 5 ms
before) per second.

| What | Measured | Test |
|---|---|---|
| `flow` 0.1 / 0.35 / 0.8 at density 60: onset rises/s | 1.9, 9.4, 17.4 | `flow_moves_onset_density_lufs_and_centroid` |
| same: LUFS | -43.8, -34.2, -27.3 | same |
| same: centroid | 3102, 2607, 1971 Hz | same |
| `flow` 0.1 / 0.35 / 0.8 at density 300: LUFS, centroid | -38.7, -29.5, -22.4; 2866, 2141, 1167 Hz | same |
| `flow` 0 | exactly 0 | `flow_zero_is_silence_and_a_ramp_is_followed` |
| flow ramp 0 -> 1 over 8 s: rms per quarter | -35.4, -25.8, -22.2, -20.0 dB | same |
| `depth` 0 / 0.5 / 1: centroid | 3640, 1759, 835 Hz | `depth_moves_the_centroid_down` |
| `splash` 0 / 0.5 / 1 at flow 0.7: peak, LUFS | 0.365, 0.434, 0.515; -23.8, -22.9, -22.4 | `splash_amount_adds_energy_and_peaks` |
| `splash` node size 0.02 / 0.1 / 0.5: centroid (whole sound, 100 Hz .. 16 kHz) | 3805, 371, 243 Hz (round 0: 8050, 6722, 688) | `splash_size_moves_centroid_decay_and_low_band` |
| same: time to -50 dB | 0.18, 0.39, 1.21 s | same |
| same: power share under 200 Hz | 0.000, 0.015, 1.000 | same |
| `splash` centroid by tenth of the audible part (to -40 dB; 20 Hz .. 20 kHz), size 0.02, seed 1 | 8046, 4191, 3485, 2799, 2360, 2229, 2129, 1969, 1367, 1260 Hz | `splash_centroid_falls_through_the_sound` |
| same, size 0.1, seed 1 | 1663, 751, 570, 489, 422, 395, 297, 266, 255, 243 Hz | same |
| same, size 0.5, seed 1 | 231, 125, 101, 75, 54, 53, 53, 52, 50, 50 Hz | same |
| same, 3 sizes x 5 seeds: reversals (tenth louder in centroid than the one before) | 0 or 1 in all 15 (asserted at most 1; first tenth over twice the last) | same |
| `splash` energy above 8 kHz in the first 20 % of the audible part, as a share of that part's energy | 0.46 (size 0.02), 0.07 .. 0.13 (0.1), 0.009 (0.5); asserted over 0.005 | `splash_burst_is_bright_and_brief` |
| same: what the last 50 % holds of the early 20 %'s energy above 8 kHz | under 3e-6 (asserted under 0.02) | same |
| `splash` peak | 0.5 +- 0.01 at every size | same |
| `splash` `at` 0.5 | first nonzero sample at 0.5000 s | `splash_at_delays_and_seeds_differ` |
| seeds 1 vs 2: energy of the difference / energy | 0.85 | same |
| output `lufs -20`, water flow 0.3 / 0.6 (+splash 0.2) / 1, density 40, density 3000 | reached to within 0.01 LU, no warning (peak -1.0 .. -6.3 dBFS) | `typical_settings_reach_lufs_minus_20_without_a_warning` |
| same, sparse: flow 0.05 / 0.1 / 0.2 | -24.6 (1 warning) / -20.8 (none) / -20.0 | same |
| `splash` at `lufs -24`, sizes 0.02 / 0.1 / 0.5 | reached, no warnings at all (peaks -2.3, -7.1, -9.7 dBFS) | same |
| `splash` at `lufs -20`, sizes 0.1 / 0.5 / 0.02 | -20.0 (peak -3.1 dBFS), -20.0 (-5.7), -21.9 with a missed-target warning (the 0.17 s pebble; crest 31 dB) | same |
| LUFS over seeds 1..8, flow 0.5, density 300 / 40 | spread 0.70 / 0.70 dB (asserted under 1.5) | `level_does_not_depend_on_a_lucky_big_bubble` |
| `kind: loop` 4 s, flow 0.5 / 0.9 + splash 0.5 / 0.15, `lufs -20`: default checks | all PASS; `loop_seam` 0.30x / 0.28x / 0.17x (max 2), spectral change 1.1 / 1.0 / 2.7 dB (max 6) | `a_loop_passes_the_default_checks_including_loop_seam` |
| 30 s, flow 1, density 300 + splash 0.5 / density 3000 | 0.27 s / 1.13 s (test profile; budget asserted 10 s) | `thirty_seconds_at_max_flow_renders_inside_the_budget` |
| flow 0/1 x depth 0/1 x density 0/3000 at splash 1; +-1e9 flow ramp; splash 0.005 and 1; 1 ms loop; 8 kHz | all finite | `extremes_are_finite` |
| two renders (pool vs one thread), loop and not | same bits | `water_and_splash_are_bit_identical` |

## Honest limits

- None of this has been heard. The centroid, level and density figures are
  measurements; whether it reads as a brook is the human's call.
- The onset counter saturates when bubbles overlap (density 300 at flow 0.35
  and above): it was measured at density 60 for that reason. Bubble births
  are a thinned Poisson process, but no test reads them directly.
- Flow is the only link between rate, size and level: they cannot be set
  independently except through `density` and `depth`.
- `loop` wraps tails and primes the hiss filters. The seam is clean for a
  constant `flow`; a `flow` that differs between the end and the start of the
  loop leaves a step in rate, size and hiss level there.
- A lone `splash` is a transient. Since the round-1 redesign sizes 0.1 and 0.5
  reach `lufs -20`; the 0.17 s pebble (size 0.02) lands at -21.9 and the output
  warns (-24 is reached by all). A sparse trickle (flow 0.05) has the same
  limit as a short sound.
- Bubbles under 0.7 mm are not made (they alias); the largest stream bubble
  is 20 mm (165 Hz), the largest splash bubble 60 mm (55 Hz), so there is no
  low rumble in `water`; mix one in for a river.
- The splash is an idealised model: impact burst, a cloud of bubbles whose
  radius grows with time, a main bubble last (the downward glide and the
  ordering are what make the centroid fall, not physics; bubbles in real water
  glide up). It has no sheet, jet or rebound. `size` is a pitch/length
  knob, not a physical dimension.
- Cost grows with bubbles a second times ring time (bigger bubbles ring
  longer): the worst case measured is 1.13 s for 30 s at density 3000, and
  the 800000-bubble cap errors before a render can run away.

## Water in a loop

`water` is periodic over the document length by itself (`kind: loop` makes
`loop` true): bubble tails wrap and the hiss filters and DC high-pass are
primed from the end. Measured, default checks including `loop_seam`, seeds
1..40 x flow 0.2 / 0.5 / 0.575 / 0.9 x density 60 / 150 / 400: 0 of 480 fail
at 1 s and at 3 s (2 of 480 at 8 s, both a loud sparse bubble ringing over the
wrap, which `loop_seam`'s whole-loop 99th-percentile step reads as 2.2x; the
sample steps there climb smoothly 0.057 .. 0.123, 0.166 (the wrap), 0.039).

A node after `water` is not periodic at the wrap: a filter starts from zero
state. `water` -> `filter` hp 250 Hz -> `filter` lp 6 kHz failed `loop_seam`
in 169 of 480 cases at 3 s (35 %). End the chain with `crossfade_loop`
(`fade` 0.25): 1 of 480 fail. `splash` is sparse and random, so in a short loop
one end window may hold a splash and the other not: the spectral half of
`loop_seam` fails in about 17 % of cases at `splash` 0.5 (0 at flow 0.9 with
the crossfade); raise `flow`, or crossfade.

## `splash` tail / burst, `thunder` tail, `water` tone

Three new optional fields. Each default leaves the render bit-identical to
before (`splash_defaults_are_bit_identical`, `thunder_tail_off_is_bit_identical`,
`water_tone_default_is_bit_identical` in `crates/sfx_analysis/tests/it/water.rs`; the
older hash-pinned tests are untouched). Tests: `crates/sfx_analysis/tests/it/water.rs`.

### `splash`: `tail`, `burst`

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `tail` | number | 1 | 0..3 | times the length of the bubble cloud after the burst and times its bubble count (the main cavity bubble ends the cloud, so it comes later); 0 is no cloud; error outside the range |
| `burst` | number | 1 | 0..2 | level of the opening burst against the main bubble; 2 makes the burst the loudest moment, 0 drops it; the splash is then levelled to its usual peak (0.5), so a louder burst quiets what follows; error outside the range |

A splash never rings past 2.5 s (the buffer cap), so a big `size` with `tail` 3 is cut there.

```json
[
  {"type": "splash", "name": "s", "size": 0.15, "tail": 2, "burst": 2, "seed": 3},
  {"type": "output", "name": "out", "input": "s", "peak_db": -3}
]
```

Macros worth giving: `tail` and `burst` on the node (no claim measured: the
sweeps of claims such as `decay` or `crest` were not run on this node).

Measured (`splash_tail_and_burst_reshape_it`, 0.15 m, seed 3, 3 s document):

- default: peak at 284 ms, 2 fine onsets after 0.2 s.
- `tail` 2, `burst` 2: peak at 2.8 ms, 12 fine onsets after 0.2 s.
- `splash_tail_lengthens_the_cloud_monotonically`: last fine onset at 0.14 / 0.27 / 0.54 s for `tail` 0.5 / 1 / 2.
- `splash_burst_moves_the_peak_into_the_opening`: peak in the first 20 ms is 0.135 / 0.461 / 0.500 for `burst` 0 / 1 / 2 (peak after 20 ms: 0.500 / 0.500 / 0.271).

### `water`: `tone`

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `tone` | number | 0.5 | 0..1 | a high shelf at 2 kHz of `(tone - 0.5) * 24` dB (so +-12 dB) over everything the node makes: hiss, bubble pops, splashes; error outside the range |

The shelf is part of the render. In a loop it is primed with the last 0.25 s of
the (periodic) buffer, so the node's output stays periodic: put the knob here,
not a `filter` after the node (which breaks the seam). The level is not
re-normalised: `tone` 1 is brighter and louder.

```json
[
  {"type": "water", "name": "w", "flow": 0.5, "tone": 0.8, "seed": 3},
  {"type": "output", "name": "out", "input": "w", "lufs": -20}
]
```

as a `kind: loop` with `duration` 3. Macro: `tone` (claim `centroid`, 1).

Measured (flow 0.5, seed 3, 4 s):

- `water_tone_raises_the_band_level_by_six_db`: `tone` 1 against 0.5 moves 2-20 kHz by +9.5 dB and 50-500 Hz by +0.0 dB.
- `water_tone_tilts_the_spectrum_above_2k`: the share of power above 2 kHz is 0.059 / 0.234 / 0.609 at `tone` 0 / 0.5 / 1: +4.2 dB from 0.5 to 1 (the share saturates: it cannot gain 6 dB from 0.234), +10.1 dB from 0 to 1, -6.0 dB from 0.5 to 0.
- `water_tone_keeps_loop_seam_at_seeds_1_to_4`: 3 s `kind: loop`, `lufs` -20, `loop_seam` PASS for `tone` 0 and 1 at seeds 1-4 (wrap discontinuity 0.27-0.66x, spectral change 0.8-2.1 dB).

Limits: the shelf is a biquad, not a model of a brighter bubble population: it
cannot add content the node does not have, and the pops sit mostly at 0.8-4 kHz
(their upper part is what it lifts). Not heard.

## Water hiss and splash after the batch-2 blind labels

Labels (docs/history/labels/batch2_findings.md): `water` heard as "a stream with wind
noise" (bubbles fine); `splash` 3/3 bad: "no idea", "drop kick", "weird
crunchies". Not heard after the fix; these are the measured causes and what
moved. Tests: `crates/sfx_analysis/tests/it/water.rs`.

### splash

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

### water

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

### Tests updated

- `water.rs`: centroid by tenth now checked by thirds (the denser random swarm
  wiggles, up to 4 reversals); "bright" burst is the share above 2 kHz (> 15 %)
  instead of above 8 kHz; size centroid ratio 0.02 / 0.1 is 2.9x (pin 2.5x
  from 3x).
- `r4d.rs`: the slap is the loudest instant from burst 1, so burst 1 and 2
  both peak at 0.5 and a louder burst shows as a quieter rest; tail 2 puts
  4 fine onsets after 0.2 s (default 1; the old cloud 10+).
