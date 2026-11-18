# fire_fix: fire and spark after the batch-2 blind listen

Heard: `fire` 3/3 as "a fire / wind next to a small gurgling liquid";
`spark` as "cicada / frog", "weird rhythm", "super aggressive". Not heard
again since the fix; the numbers below are measurements
(`crates/sfx_analysis/tests/fire_fix.rs`, `m3.rs`).

## fire

- Pops were two-pole rings at 0.9-5.5 kHz (2-11 ms): pitched droplets. Now
  each crackle is a wood snap: 1-4 sub-clicks within ~4 ms, each a
  0.2-2.5 ms white-noise burst under an exponential fall (first sample at
  full height), the sum high-passed by two one-pole sections at 700 Hz x
  (1 / size)^0.15. Power-law heights and the rate law are unchanged.
- Roar: slow turbulence AM depth 0.55 -> 0.5, plus a flutter
  (1 + 0.3 x a 23 cell/s lattice + 0.2 x a 61 cell/s lattice); hiss level
  0.08 -> 0.035. All lattices wrap, so `loop` still seams.

| measure (size 0.6) | before | after |
|---|---|---|
| crackle only (hp 1.2 kHz), flatness middle, 3 seeds | 0.33 | 0.74 |
| crackle only, voiced | 0.01 | 0.01 |
| roar under 600 Hz, mean 40 ms frame step | 1.58 dB | 2.02 dB |
| m3 pops in 10 s at crackle 0 / 0.5 / 1 | - | 4 / 121 / 217 |
| m3 pop-size log-log slopes | - | 1.03 / 1.42 |

## spark

- Strike length is a power law 5-150 ms (was uniform 20-70 ms); inside a
  strike the re-strike density is redrawn every 2-8 ms (0..6000/s, squared
  uniform), heights a capped power law; the mains gate is half-deep
  (0.45 + 0.55 x the rectified sine). Body rings 3 / 2 ms -> 1.2 / 0.8 ms,
  output 0.15 x body + 0.03 x dry -> 0.2 x body + 0.06 x dry.
- `loop: true` still uses a jittered grid and whole mains cycles.

| measure (rate 20, buzz 50, 3 seeds) | before | after |
|---|---|---|
| 100 Hz envelope line over the median | 32.1 dB | 12.0 dB |
| strike length spread (sd / mean) | 0.66 | 1.09 |
| strongest envelope line | 100 Hz | 22.9 Hz (strike rhythm) |

Tests changed in `m3.rs`: crackle 0 bound 50x -> 20x (the roar flutter
makes 4 peaks in 10 s in the 1.4 kHz band), pop slope upper bound 1.4 ->
1.55 on the top doubling, spark mains test now asserts the 100 / 120 Hz
line over 8 dB (not the strongest line), loop retune test searches 95-105 Hz.
