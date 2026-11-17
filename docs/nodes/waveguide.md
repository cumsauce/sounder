# M5: `tube`, `string`

Two physical-model nodes in `crates/sfx_graph/src/nodes/waveguide.rs`; tests
in `crates/sfx_analysis/tests/waveguide.rs`; worked graph in
`examples/waveguide.rs`. Both are digital waveguides: delay lines
(`dsp::DelayLine`, third-order Lagrange read) closed into a loop through a
lossy reflection filter, with the player's nonlinearity where the loop is
excited. Sample by sample, no lookahead, so both are causal; both are
`random` (seeded) only where they read the seed (`tube` always, `string`
for `pluck`). Mono output.

## `tube`

A wind-instrument bore (source/model family): a loop of one round trip,
2 L / c with c = 343 m/s, an end-reflection low-pass (a one-pole, twice when
both ends are open), friction loss from the bore radius, a DC blocker
(`dsp::Biquad` high-pass), and one of four exciters. The loop delay is
corrected for the phase delay of every filter in it at the pitch, so the
pitch is c / (2 L) open at both ends and c / (4 L) closed at one (the loop
sign flips once per round trip).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `length` | number | required | 0.029 .. 20 m (the lower bound is 8 samples of loop: 4 c / sample rate) | Acoustic length, no end correction added. |
| `excite` | word | `reed` | `reed` `lip` `jet` `noise` | Reed: Smith's clarinet table (offset 0.7, slope -0.3). Lip: the returning wave through a resonant band-pass at the pitch (`Biquad`) into a cubic valve biased by the pressure (even harmonics). Jet: the returning wave through a one-pole low-pass at 1.5 x the pitch, a jet delay of 0.32 of the loop and a cubic table `s - s^3/3` (odd harmonics only). Noise: seeded noise into the bore, no self-oscillation. |
| `pressure` | signal | required | 0 .. 1 | Blowing pressure. A number outside 0..1 is an error; a signal is clamped and smoothed over 8 ms. |
| `bore` | number | `0.01` | 0.001 .. 0.5 m radius | Friction loss (about exp(-3e-5 sqrt(f) / r x 2 L), kept in 0.6 .. 0.999) and the end low-pass at 0.8 c / (2 pi r), never under 3 x the pitch. The output is the wave after the first end-filter stage, so a wider bore is a darker output. |
| `open_end` | bool | `true` | | `true`: open at both ends, pitch c / (2 L), all harmonics. `false`: closed at one end, pitch c / (4 L), odd harmonics. |
| `seed` | integer | `0` | | Breath noise (2 % of the pressure) and the `noise` excitation. |

Work cap: the longest tube is 20 m (a delay line of 2 L / c, 5600 samples at
48 kHz). Errors name the field and the range:
`length: 0.001: give a length in metres from 0.029 (a loop of 8 samples at 48000 Hz) to 20`.
A pitch above 40 % of the sample rate is refused too.

### Recipe

```json
[
  {"type": "ramp", "name": "up", "from": 0.0, "to": 0.8, "time": 0.08, "curve": "linear"},
  {"type": "tube", "name": "pipe", "length": 0.6, "excite": "reed", "pressure": "up", "open_end": false, "seed": 1}
]
```

Give the human a macro on the `pressure` envelope's peak (level and, past the
threshold, brightness) and on `bore` (`centroid`, falls as it rises).

### Measured

| What | Measured | Test |
|---|---|---|
| Pitch, reed/lip/jet x open/closed x L 0.3, 0.6, 1.2 m, strongest peak vs c/(2L) or c/(4L) | worst error 1.81 % (jet, closed, 0.6 m); reed within 0.07 % | `tube_pitch_follows_length_and_the_ends` |
| Noise-driven comb peak nearest the pitch | -0.01 % open, -0.59 % closed | `tube_noise_drive_rings_at_the_resonances` |
| Level at pressure 0.1 / 0.8 / 1.0 (rms, second half of 2 s) | reed -75.8 / -7.3 / -6.3 dB; lip -72.8 / -6.4 / -4.2; jet -72.8 / -6.0 / -3.8 | `tube_pressure_has_a_threshold_and_then_a_steady_tone` |
| Steadiness: rms at 1 s vs 2 s windows | 0.00 dB for all three | same |
| A 0 -> 1 pressure ramp over 2 s passes -30 dB at pressure | reed 0.54, lip 0.31, jet 0.35 | `tube_a_pressure_ramp_crosses_the_threshold` |
| Closed reed harmonics 1 / 2 / 3 | -6.2 / -46.2 / -38.5 dB | `tube_open_and_closed_ends_change_the_harmonics` |
| Open reed harmonics 1 / 2 | -7.3 / -37.1 dB | same |
| Noise-driven centroid, bore 4 / 20 / 80 mm | 5149 / 1543 / 938 Hz | `tube_bore_darkens_the_tone` |
| Pitch at 44.1 and 96 kHz (closed reed, 0.6 m) | 142.9 Hz both (formula 142.9) | `tube_and_string_follow_the_sample_rate` |
| Extremes (L 0.03 and 20 m, bore 0.001 and 0.5, pressure 0 and 1, all four exciters, wild pressure signal) | finite, peak under 4 (reed L 0.03 bore 0.001: 0.134) | `tube_is_finite_at_the_extremes` |

### Honest limits

None of this has been heard. The loop is a lumped reflection model, not a
bore with toneholes or a bell: `lip` is a simplified valve (no lip-mass
dynamics), `jet` is odd-harmonic only, and the lip and jet loops are tuned
for the fundamental (a brass-like overblown register is not reachable). The
pitch error grows slightly with the jet and with very long or narrow tubes
(1.8 % at worst here); a bore so narrow and long that the loop gain stays
under one never sounds (the clamp keeps the loss at 0.6 or more per round
trip). The threshold depends on loss, so it moves with `length` and `bore`.

## `string`

A string (source/model family) as two delay lines that meet at `position`
and carry the round trip to the nut and to the bridge. The nut reflects
rigidly; the bridge reflection has a one-pole loop low-pass, a gain set from
`damping`, and four first-order all-pass sections for `stiffness`. The
player acts where the lines meet: a burst of noise (`pluck`), one cycle of
a 2 ms sine (`strike`), or the stick-slip friction table of the Synthesis
ToolKit's bowed string (`bow`, `slope = 5 - 4 p`, table
`(|slope x| + 0.75)^-4`, bow speed 0.25 with a 50 ms attack, scaled by
`min(1, 2p)`). Delay lengths and filters are refreshed every 16 samples, so
`freq` can be a signal.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `freq` | signal | required | 20 .. sample rate / 16 Hz (3000 at 48 kHz) | Pitch. A number outside the range is an error; a signal is clamped. |
| `excite` | word | `pluck` | `pluck` `strike` `bow` | A pluck and a strike happen at time 0; the bow plays from 0 until `bow_pressure` lifts it. |
| `position` | number | `0.15` | 0 .. 1 | Where the string is excited, as a fraction of its length from the bridge (clamped to keep both segments over 2 samples). 0.5 cancels the even harmonics. |
| `damping` | number | `0.3` | 0 .. 1 | Decay to -60 dB at the pitch from 12 s (0) to 50 ms (1), exponential in between; the loop low-pass from 62 x the pitch (0) to 2 x (1). |
| `stiffness` | number | `0` | 0 .. 1 | All-pass coefficient `-0.85 stiffness^0.25`, within a delay budget (0.6 of the loop): upper partials land above whole multiples; the pitch is kept (the delay at the pitch is subtracted). |
| `bow_pressure` | signal | `0.5` | 0 .. 1 | A number outside 0..1 is an error; a signal is clamped and smoothed over 10 ms. 0 lifts the bow. Only `bow` reads it. |
| `seed` | integer | `0` | | The pluck's noise burst. |

Work cap: the delay lines are 1/20 of a second (2400 samples at 48 kHz) each;
nothing else scales with a field.

### Recipe

```json
[
  {"type": "string", "name": "g", "freq": 196, "excite": "pluck", "position": 0.18, "damping": 0.35, "stiffness": 0.1, "seed": 3}
]
```

Macros worth giving the human: `position` (brightness: moves the comb of
harmonics), `damping` (length: decay), `stiffness` (inharmonicity).

### Measured

| What | Measured | Test |
|---|---|---|
| Partials 1..3 vs f, 2f, 3f (pluck, strike, bow; 55 .. 1760 Hz) | worst error 0.35 % (bow, 1760 Hz); pluck and strike under 0.01 % | `string_pitch_matches_freq` |
| Glide 200 -> 400 Hz in 2 s (bow) | 250.1 Hz at 0.5 s (want 250), 380.2 Hz at 1.8 s (want 380) | `string_freq_takes_a_signal` |
| Time to fall 20 dB, damping 0 / 0.3 / 0.6 | 2.60 / 0.57 / 0.13 s | `string_damping_shortens_the_decay` |
| Inharmonicity (partials 5-8 above n x f1), stiffness 0 / 0.1 / 0.25 / 0.5 / 1 | 0.0000 / 0.0010 / 0.0030 / 0.0097 / 0.0637 (fundamental 220.00 Hz at every setting) | `string_stiffness_stretches_the_partials` |
| Harmonics 1-4 at position 0.15 / 0.25 / 0.5 | 4th: -29.8 / -43.2 dB; at 0.5 the 2nd: -52.2 dB (against -29.3) and the 4th -46.7 | `string_position_puts_nulls_in_the_spectrum` |
| Bow pressure 0 / 0.5 / 0.9, rms of the second second | -120 / -14.2 / -13.7 dB; fundamental 220.01 and 220.06 Hz; steady within 0.00 dB | `string_bow_pressure_sets_the_bow_down` |
| Bow lifted at 1 s (pressure ramp to 0, damping 0.6) | -19.9 dB before, -120 dB at 1.8 s | `string_lifting_the_bow_lets_it_ring_down` |
| Centroid, pluck / strike (220 Hz) | 633 / 471 Hz; peaks 0.725 / 0.742 | `string_pluck_and_strike_differ_and_seeds_differ` |
| Pitch at 44.1 and 96 kHz (bow, 220 Hz) | 220.04 and 219.73 Hz | `tube_and_string_follow_the_sample_rate` |
| Extremes (20 and 3000 Hz, position 0 and 1, damping 0 and 1, stiffness 1, bow pressure 0 and 1, wild freq signal) | finite, peak under 4 (pluck 20 Hz stiffness 1: 0.641) | `string_is_finite_at_the_extremes` |

### Honest limits

None of this has been heard. The pluck and the strike are fixed 2 ms
excitations (no hardness field). The nulls from `position` are finite
(13 to 23 dB deep) because only the bridge side is lossy. The bow reaches a
steady fundamental at pressure 0.5 and 0.9 for 220 .. 880 Hz with the default
position, but lower pressures (0.1 to 0.3) and some pitch/position pairs lock
to a higher harmonic; `listen` before trusting a bow at a new pitch. A bow at
110 Hz and `position` 0.15 gives the fundamental, but at other positions the
strongest partial is a multiple of it (the comb), which a spectral peak
picker reads as a higher pitch. At 3000 Hz a loop is 16 samples, so
`stiffness` has almost no delay budget left (it shrinks). The output is level-matched
by fixed gains (peaks 0.3 to 0.75 in the tests), not normalized.
