# rain

`{"type": "rain", "surface", "rate", "roof", "roof_height", "seed", "loop"}`.
Code: `crates/sfx_graph/src/nodes/rain.rs`; tests:
`crates/sfx_analysis/tests/it/rain.rs`.

A port of the rain bed in the foley project (github.com/cumsauce/foley,
`crates/foley-dsp/src/rain.rs`, MIT / Apache-2.0). Drops near the listener
(0.5 m2 of the ground's flux, at most 2 500 a second) are heard one by one:
diameters from Marshall-Palmer for the rate (lambda = 4.1 R^-0.21 per mm),
energy D^3 v^2 with Gunn-Kinzer terminal speeds, 0.3-1.5 m away. Each is a
struck surface (two damped modes in the surface's band and a 0.25 ms click)
or, on water, a bubble: 0.8-1.1 mm drops trap a ~14 kHz Minnaert bubble
(Medwin and Pumphrey's regular entrainment, the sizzle), drops over 2.2 mm
now and then a lower plink. The far drops are a hiss low-passed at 1-8 kHz
by the surface's brightness (600 Hz under a roof); water adds a sizzle
band. Under a `roof`, 85 % of the near drops land on it.

| surface | modes | heard (batch 8) |
|---|---|---|
| water | bubbles ~14 kHz + soft splash | - |
| stone | 3-7 kHz, fast | - |
| leaf | 0.5-1.8 kHz | - |
| wood | 280-900 Hz | - |
| soft (grass, dirt; `ground`) | 250-1200 Hz, very quiet | - |
| glass (`window`) | 2-6 kHz, ringing | - |
| metal | 0.7-3.2 kHz, long ring | - |

Measured, 4 s at rate 10: stone centroid 4.9 kHz, 72 % in 2-8 kHz; leaf
1.1 kHz, 91 % in 200 Hz-2 kHz; water 10.4 kHz (57 % over 8 kHz); under a
wood roof the centroid falls from 4.9 kHz to 644 Hz. Levels differ by 20 dB
(soft is -48 dB rms, metal -25): set `lufs` on the output.

Replaces a port of Farnell's RAIN patches (a few drops a second), which a
listener heard as static, popcorn and tearing tape, also with a dense noise
"wash" added (`docs/history/labels/batch6_findings.md`).
