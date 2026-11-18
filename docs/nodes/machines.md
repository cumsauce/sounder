# Engine, motor

Two machine models in `crates/sfx_graph/src/nodes/machines.rs`, tests in
`crates/sfx_analysis/tests/machines.rs`, a worked render in
`examples/engine.rs` (writes `engine.wav/png` and `motor.wav/png`). Both take
`rpm` as a signal, so a `ramp` or `wander` revs them and every internal
frequency follows. Both are seeded (`random()` true) and causal.

## `engine`

A piston engine (model, source): one combustion pulse per firing, an
open-ended exhaust pipe, and an intake noise path, all driven by `rpm`. The
pitch is the firing frequency `rpm / 60 * cylinders / (stroke / 2)` Hz.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `rpm` | signal | required | 0 .. 20000 | Crankshaft speed. 0 is silence. A number outside the range is an error; a signal is clamped per sample (a non-finite sample counts as 0). |
| `cylinders` | whole number | `4` | 1 .. 16 | Firings per cycle; pitch is proportional. Each cylinder has its own seeded level error of up to 6 %. |
| `load` | signal | `0.5` | 0 .. 1 | Throttle. Up: each pulse is stronger (0.25 + 0.75 load) and shorter (valve open 0.30 rev at load 0, 0.12 rev at load 1), the intake noise opens (low-pass 300 + 2500 load Hz) and gets louder, the cycle-to-cycle level variation shrinks (+-16 % at 0, +-4 % at 1). A number outside 0..1 is an error; a signal is clamped. |
| `stroke` | whole number | `4` | 2 or 4 | Strokes a cycle. Two-stroke fires every revolution: twice the pitch of a four-stroke with the same cylinders and rpm. |
| `exhaust_length` | number | `1.2` | 0.05 .. 10 m | An inverting comb (reflection -0.6, low-passed at 2.5 kHz in the loop) with round trip `2 L / 343` s: a pipe open at one end, resonating at odd multiples of `343 / (4 L)` Hz. Longer is lower. |
| `seed` | whole number | `0` | | Mixed with the document seed and node name: cylinder level errors, the +-2 %-of-a-period timing jitter and level jitter of each firing, and the intake noise. |

Firing order: the cylinders fire at equal spacing in order 0, 1, 2 ...
(mono output has no use for the physical order); what the order carries is
the per-cylinder level error, which repeats every cycle and puts lines at
the cycle rate `rpm / 60 / (stroke / 2)`.

Caps and errors (the work is `n` samples plus pulses at most a quarter of a
sample apart, each at most 0.25 s and one firing period wide, and a delay
line of at most 10 m; nothing else scales with a field):

- `cylinders: 20: give a whole number from 1 to 16`
- `stroke: 3: give 2 or 4`
- `exhaust_length: 11: give metres from 0.05 to 10`
- `rpm: 30000: give a number from 0 to 20000 (a signal is clamped to that range)`
- `load: 1.5: give a number from 0 to 1`

### Recipe

An engine revving up with the throttle opening and the top rolled off. Give
the human macros on the `to` of `revs` (how far it revs), on `exhaust_length`
and on the `throttle` ramp's `to`.

```json
[
  {"type": "ramp", "name": "revs", "from": 1200, "to": 5200, "time": 2.0, "curve": "exp"},
  {"type": "ramp", "name": "throttle", "from": 0.2, "to": 0.9, "time": 2.0},
  {"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 6, "load": "throttle", "exhaust_length": 1.5, "seed": 1},
  {"type": "filter", "name": "body", "input": "eng", "mode": "lp", "freq": 6000}
]
```

`ramp` defaults to an `exp` curve; add `"curve": "linear"` for a linear rpm
ramp. Set the sound's `duration` (the ramps hold their last value, so an
auto-length render would run to 30 s).

### Measured

48 kHz. Pitch is the lowest spectral peak above 30 Hz within 15 dB of the
strongest (the second harmonic is sometimes the strongest line).

| What | Measured | Test |
|---|---|---|
| pitch vs `rpm/60 * cylinders/(stroke/2)`: 8 settings (1800/3000/4800 rpm 4 cyl, 6 cyl, 8 cyl, 4 cyl 2-stroke, 1 cyl 2-stroke, 3 cyl) | worst error 0.01 % (limit 2 %) | `engine_pitch_is_the_firing_frequency` |
| cylinders 4 -> 8; stroke 4 -> 2 | pitch ratio 2.000 and 2.000 | `engine_pitch_doubles_with_cylinders_and_two_stroke` |
| linear rpm ramp 1800 -> 5400 over 2 s, 0.5 s windows | pitch 73.1, 88.4, 104.5, 119.5, 134.7, 149.9, 165.8 Hz against rpm-implied 75 ... 165 Hz (worst 2.5 %, limit 3 %); strictly rising | `engine_ramped_rpm_gives_a_rising_pitch_track` |
| `load` 0.1 / 0.5 / 0.9 | centroid 369 / 471 / 690 Hz; rms -26.9 / -22.6 / -21.2 dB | `engine_load_raises_level_and_centroid` |
| `load` ramp 0.05 -> 1 over 2 s (signal) | rms -26.6 dB first half, -22.7 dB second | `engine_load_signal_changes_the_sound_over_time` |
| `exhaust_length` 1.6 / 2.4 / 3.2 m: echo delay by correlation with the first pulse | 9.00 / 13.73 / 18.62 ms against 2L/c 9.33 / 13.99 / 18.66 ms (up to 0.33 ms early, limit 0.4) | `engine_exhaust_pipe_echo_arrives_after_the_round_trip` |
| seeds 1 and 2 | all samples differ; pitch 100.00 and 100.00 Hz; rms -22.43 / -22.34 dB | `engine_seed_changes_the_cycle_jitter_not_the_pitch` |
| `rpm` 0 | peak 0 (exact silence) | `engine_rpm_zero_is_silent` |
| rpm {0, 1, 20000} x cylinders {1, 16} x stroke {2, 4} x load {0, 1} x length {0.05, 10}; and a +-1e9 rpm sweep with a +-50 load sweep | all finite, peak at most 0.53 | `engine_is_finite_at_the_extremes` |
| two renders, one thread vs the pool (also an rpm ramp, 6 cyl 2-stroke) | same bits | `engine_is_bit_identical` |
| recipe above, 2 s | 96000 samples, rms -25.0 dB, second half 2.2 dB louder than the first | `machines_recipe_renders` |
| example (6 cyl, 900 -> 5200 rpm, 3 s, -18 LUFS) | peak -1.0 dBFS (ceiling limiter), crest 17.4 dB; default checks all PASS | `examples/engine.rs` |
| a 30 s render with 5 nodes of the example | 1.4 s wall time (debug profile, optimized) | `examples/engine.rs` run once with no duration |

### Honest limits

- The firing pulse is a skewed raised cosine, not a measured pressure trace;
  the exhaust is one inverting comb, not a muffler with chambers; the intake
  is low-passed noise modulated by the pulses. Nothing here was compared with
  a recording, and none of it has been heard.
- The pitch line is exact (the firing rate) because the timing jitter is
  zero-mean; a lot of jitter would not move it but would raise the noise
  between the lines.
- Below rpm ~900 for 4 cylinders the pitch is under 30 Hz; the pulses are
  then separate thumps, and the 20 Hz DC blocker removes part of the
  fundamental.
- The output level follows `load` only (about +5.7 dB from load 0.1 to 0.9),
  not rpm; normalise with `output.lufs`. At -18 LUFS the crest of 17 dB
  reaches the output ceiling limiter.
- Values are finite at every extreme listed above. A `rpm` signal is clamped
  to 0..20000, so a faster engine is not possible.

## `motor`

An electric motor with a gearbox (model, source): the electrical hum at the
pole-pair rate and its harmonics, a PWM drive whine with sidebands, a
gear-mesh tone at `rpm / 60 * gear_teeth` Hz and brush noise, all driven by
`rpm`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `rpm` | signal | required | 0 .. 20000 | Rotor speed. 0 is silence; the whole output is scaled by `rpm / (rpm + 500)`, so a spin-up fades in. A number outside the range is an error; a signal is clamped. |
| `poles` | whole number | `4` | even, 2 .. 64 | The electrical rate is `rpm / 60 * poles / 2` Hz; hum lines at 1, 2, 3, 4 times it (amplitudes 0.22, 0.12, 0.08, 0.04), plus a 0.06 line at the shaft rate. |
| `gear_teeth` | whole number | `0` | 0 .. 400 | 0: no gearbox. Else a mesh tone at `rpm / 60 * gear_teeth` Hz (amplitude 0.6 x torque factor), its second harmonic (0.4 x) and sidebands at mesh +- shaft rate (from a 30 % shaft-rate amplitude modulation). Partials above 0.47 x sample rate fade to nothing (fully off from 0.47) instead of aliasing. |
| `load` | signal | `0.3` | 0 .. 1 | Up: mesh tone and whine louder (torque factor 0.3 + 0.7 load), hum slightly louder (0.7 + 0.3 load), brush noise louder (0.3 + 0.7 load). |
| `whine` | signal | `0.3` | 0 .. 1 | Level of the PWM carrier (0.12 x whine x torque); it is amplitude-modulated at 2 and 4 times the electrical rate (depths 0.8 and 0.4), which puts sidebands at carrier +- 2 f_e and +- 4 f_e. 0 is none. |
| `carrier` | number | `4000` | 200 .. 0.45 x sample rate, Hz | The PWM carrier frequency (not on the card; added so the whine can be placed). |
| `seed` | whole number | `0` | | Brush noise only. |

Caps and errors (nothing scales with a field except the document length):

- `poles: 3: give an even number from 2 to 64`
- `gear_teeth: 401: give 0 to 400 teeth`
- `carrier: 100: give Hz from 200 to 21600 (0.45 x the sample rate)`
- `rpm: -5: give a number from 0 to 20000 (a signal is clamped to that range)`
- `whine: 2: give a number from 0 to 1`

### Recipe

A motor spinning up with a 24-tooth gear. Give the human macros on the
`ramp`'s `to` (top speed), `load` and `whine`.

```json
[
  {"type": "ramp", "name": "revs", "from": 800, "to": 6000, "time": 2.0, "curve": "exp"},
  {"type": "motor", "name": "mot", "rpm": "revs", "poles": 4, "gear_teeth": 24, "load": 0.4, "whine": 0.4, "carrier": 6000, "seed": 1}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| strongest peak vs mesh frequency `rpm/60 * teeth` (1800/3000/6000 rpm x 20 teeth; 3000 rpm x 10 and 40) | all within 0.001 % (limit 1 %); the mesh line is the strongest at -23.5 to -24.7 dB | `motor_peaks_at_the_gear_mesh_frequency` |
| mesh peak on a linear 1500 -> 4500 rpm ramp, 20 teeth, 0.2 s windows, strongest peak above 300 Hz | 555.3 / 674.1 / 802.3 / 924.1 / 1050.3 / 1174.3 / 1299.5 / 1425.0 Hz against 550 ... 1425 Hz (worst 1.0 %, limit 3 %) | `motor_mesh_peak_follows_a_ramp` |
| hum (no gearbox, whine 0): poles 2/4/8 at 3000 rpm, 4 poles at 1800 and 6000 rpm | strongest peak at `rpm/60*poles/2` within 0.011 % (limit 1 %) | `motor_hum_follows_rpm_and_poles` |
| `whine` 0 / 0.3 / 1: band 3990-4010 Hz | -76.3 / -45.0 / -34.6 dB | `motor_whine_sets_the_carrier_level_with_sidebands` |
| whine 1, 3000 rpm 4 poles (f_e 100 Hz): bands at 4000, 4000 +- 200, 4000 +- 400 Hz | -34.6; -42.6 and -42.6; -48.5 and -48.6 dB; 3900 Hz is more than 20 dB below the 3800 Hz line | same |
| `load` 0.1 / 0.5 / 0.9 (20 teeth) | centroid 758 / 947 / 1022 Hz; rms -20.3 / -16.6 / -13.9 dB | `motor_load_raises_level_and_centroid` |
| `rpm` 0 | peak 0 (exact silence) | `motor_rpm_zero_is_silent` |
| rpm {0, 1, 20000} x poles {2, 64} x teeth {0, 1, 400} x load {0, 1} x whine {0, 1} at carrier 21600; and a +-1e9 rpm sweep | all finite, peak at most 0.70 | `motor_is_finite_at_the_extremes` |
| two renders, one thread vs the pool | same bits | `motor_is_bit_identical` |
| example (24 teeth, 800 -> 6000 rpm, 2.5 s, -20 LUFS) | peak -11.6 dBFS, crest 9.3 dB; default checks all PASS | `examples/engine.rs` |

### Honest limits

- The mesh and hum are sums of sine partials at exact multiples of one phase
  each; real gears have profile errors, ghost frequencies and resonances this
  does not model. The gear phase is advanced separately from the shaft phase,
  so mesh and shaft lines are not phase-locked.
- The rotor's own tone and the shaft-rate sidebands are fixed fractions, not
  physical eccentricity.
- The PWM carrier is a sine; real drives have a square-ish carrier with its
  own harmonics.
- A partial that would pass 0.47 x the sample rate is faded out between 0.4
  and 0.47, so a fast, many-toothed gear loses its mesh tone (3000 rpm x 400
  teeth = 20 kHz) at 48 kHz rather than aliasing.
- Level depends on `rpm` through `rpm / (rpm + 500)` and on `load`; normalise
  with `output.lufs`. None of this has been heard.

## Engine and motor after the batch 2 blind listen

Labels: engine 2 / 4 cylinders "digital riser", "motorcycle revving, really low
resolution"; 6 cylinders "racecar, a bit trombonish"; motor "artificial /
lo-fi", "digital". No field was added; defaults changed.

### engine

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

### motor

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
