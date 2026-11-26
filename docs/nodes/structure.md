# Structure

Big bodies as statistics: `structure` (a panel or shell too large to hear as
partials) and `rumble` (the pitchless low roll after a heavy event). Code:
`crates/sfx_graph/src/nodes/structure.rs`; tests:
`crates/sfx_analysis/tests/it/structure.rs` (the figures below are the lines
they print with `-- --nocapture`).

Why: an airliner panel or fuselage has thousands of modes under 5 kHz, so the
ear hears shaped noise whose decay falls with frequency ("structural
reverberation"), not partials. `modal` / `impact` / `rig` stop at 96 modes:
heavily damped they read as wood, lightly damped as a gong. A listener turned
down three passes of airliner collisions built from them. Use `structure` for
anything metres across (a hull, a fuselage, a wing, a girder, a shipping
container); keep `modal` for things you could hold.

## `structure`

The `excite` audio is convolved with the body's impulse response: noise whose
power per Hz and decay per frequency come from the physics below, plus the
lowest modes as discrete resonances where the body is sparse. A unit impulse
peaks at 0 dBFS (the response is normalized to peak 1). A hit (impulse,
`contact`, an `impact` or `rig` output) and a stream (`friction`, gated noise
for a scrape or tear) both work, since it is a convolution.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `excite` | audio | | | The force. Alias `input`. Stereo is convolved per channel. |
| `material` | name or constants | | as `modal` | `stone wood metal glass ice earth granular plant soft snow` or `{young, density, poisson, loss, thickness}`; the same constants as `modal`. |
| `size` | number / `$param` / `=expr` | | m, 0.05 .. 200 | Plate side or shell diameter, read once. Alias `diameter`. |
| `length` | number | 2 x size | m, 0.05 .. 2000 | Shell only (a plate with `length` is an error). |
| `shape` | word | `plate` | `plate`, `shell` (aliases `tube`, `cylinder`) | Changes the area, the density law and the low modes. |
| `thickness` | number / signal read once | the material's | m, 0.0002 .. 0.5 | Skin or wall. Airliner skin 0.002, hull 0.015, girder web 0.03. Note foley's `metal` is 0.05. |
| `loss` | number | 1 | 0.01 .. 100 | Multiplier on the material's loss factor (`metal` 0.004, a game value: structural steel is nearer 1e-4 to 1e-3, so 0.05 .. 0.3 is realistic for built-up metal, whose damping is at the joints). |
| `edge_loss` | number | 0.1 | 0 .. 1 | Share of a bending wave's energy an edge or joint absorbs (rivets, bolts, supports). |
| `panel` | number | the whole body | m, 0.05 .. 200 | Spacing of the frames / stiffeners that bound one bay: the edge loss uses perimeter / area = 4 / panel. |
| `tilt` | number | 0 | dB / octave about 1 kHz, -12 .. 12 | Balance of the response; the peak normalization keeps the level. |
| `low_modes` | integer | 32 | 0 .. 96 | Most discrete low modes (0: all statistical). |
| `gain_db` | number | 0 | -60 .. 60 | After the normalization. |
| `seed` | integer | 0 | | The response noise and the low modes' tuning and strike. |

Errors name the range: `size: 500: give metres in 0.05..200 (plate side or
shell diameter; under about 0.3 m a modal body is the better node)`, `length:
3: only the "shell" shape takes it ...`, and one per field for `loss`,
`thickness`, `edge_loss`, `panel`, `tilt`, `low_modes`, `gain_db`.

### The physics used

Symbols: S area (plate size^2; shell pi d L), P perimeter (plate 4 size; shell
2 pi d), h thickness, c_L = sqrt(E / (rho (1 - nu^2))), m'' = rho h.

- **Modal density.** Plate: n = sqrt(3) S / (h c_L) modes per Hz, the same at
  every frequency (Cremer, Heckl & Petersson, "Structure-Borne Sound").
  Shell: the plate's above the ring frequency f_r = c_L / (pi d), fewer below
  (the wall is stiff in its own plane): n / n_plate = 1.75 sqrt(f / f_r) under
  0.48 f_r, 1.21 to 0.83 f_r, then back toward 1 as (0.83 f_r / f)^1.5
  (Szechenyi 1971, as cited from memory and made continuous).
- **Discrete or statistical.** Below the first frequency where n(f) x CB(f)
  >= 1 (CB Zwicker's critical bandwidth, 25 + 75 (1 + 1.4 (f/kHz)^2)^0.69 Hz)
  the body's own modes are kept: plate f(m, n) = pi / (2 sqrt 12) h c_L
  ((m/a)^2 + (n/b)^2) on a 1.15 : 1 / 1.15 rectangle of area S; shell from
  Donnell's simplified equation (Leissa, "Vibration of Shells"), cos / sin
  pairs for n >= 1; 1 % seeded mistuning. At most `low_modes`; when capped the
  statistical part starts at the last kept mode. The crossover is one octave
  wide, power complementary. Each mode carries the energy of the 1 / n(f) Hz
  it stands for (amplitude sqrt(2 P(f) / n(f))) times a Rayleigh strike
  weight. A 2 mm steel plate is statistical from 20 Hz once it is about
  0.25 m across (n = 0.01 per Hz); a 4 m x 40 m, 2 mm shell has 34 modes per
  Hz even at 20 Hz (0.05 of its ring frequency), so it has no discrete modes
  either: they matter for small, thick bodies.
- **Loss and decay.** eta(f) = eta_material + eta_radiation + eta_edge;
  T60(f) = 2.2 / (eta f), clamped to 3 ms .. 20 s.
  eta_rad = rho0 c0 sigma / (omega m'') with sigma Maidanik's edge term below
  coincidence (f_c = c0^2 / (1.8 h c_L)), 1 / sqrt(1 - f_c / f) above (at most
  3), both times (f / f_L)^2 / (1 + (f / f_L)^2), f_L = c0 / (2 size).
  eta_edge = c_g (P / S) alpha / (pi omega), c_g = 2 sqrt(omega h / sqrt 12
  c_L) (Lyon & DeJong, "Statistical Energy Analysis", damping), P / S = 4 /
  `panel` when given. Material loss alone gives T60 ~ 1 / f; joints alone
  ~ 1 / sqrt(f). Bigger bodies ring longer through the edge term.
- **Spectrum at the start.** P(f) ~ n(f) / n_plate (the energy an impulse
  puts in a band goes as the real part of the drive-point mobility, so as the
  density) x sqrt(min(f, f_c) / f_c) x the size floor (f / f_L)^2 / (1 +
  (f / f_L)^2) x `tilt`, with a 4th-power roll-off under the lowest mode.
  Honest simplification: the edge-radiation constant P c0 / (pi^2 S f_c) is
  left out of the spectrum (it would put everything above coincidence 30 dB
  over the rest; no heavy, soft hit's force reaches up there), only its slope
  is kept. It stays in eta_rad.
- **Synthesis.** The response is built in three STFT layers of Gaussian
  noise (FFT 8192 below 350 Hz, 1024 to 3 kHz, 256 above; one-octave power
  complementary crossovers; hop n/4, Hann). Each bin falls from frame to frame
  at its T60, frames start before t = 0 so the response is at full level from
  its first sample, and each bin's level is corrected for the window's spread
  of the decay. A decay faster than half a window is clamped to what that
  layer can show (8192: about 40 ms). Then the discrete modes are added, the
  response is normalized to peak 1 and convolved by FFT with `excite`. Its
  length is the longest band's T60 (from 20 Hz or the lowest mode), never
  longer than the render; a non-finite sample becomes 0.

Limits: no coupling between the bands (no energy flowing from a hit band to
its neighbours), no nonlinearity (a harder hit is only louder: give a brighter
excitation), no air loading, no pitch glide. Physics constants (rho0 1.21,
c0 343) are fixed.

### Measurements (impulse, `metal`, size 10 m, thickness 3 mm unless noted)

- Unit impulse: peak 1.000.
- Size, 3 mm metal plate: centroid 6688 / 5532 / 5339 / 4736 Hz and 60-250 Hz
  lasting 0.15 / 0.65 / 1.60 / 2.20 s to -30 dB at 0.5 / 2 / 8 / 30 m.
- 20 m, 2 mm plate, band lengths to -30 dB: 60-250 Hz 2.20 s, 250-1000 Hz
  0.55 s, 1-4 kHz 0.15 s, 4-16 kHz under one 50 ms frame.
- No partials: plate 20 m, shell 4 x 40 m, plate 60 m (2 mm): the strongest
  bin from 0.6 to 1.2 s stands 8.4 dB over the median within a sixth of an
  octave; a 1 m `modal` metal plate (loss 0.3) stands 60 dB over.
- Seeds: seed 1 vs 2 correlate 0.063; one seed is bit-identical on one or
  many threads.
- A 2 s gated-noise scrape renders (level 3.5 dB RMS at 1.0-1.5 s, the
  response is not normalized for streams: set the level at the output).
- A 3 s hit (structure + rumble below) renders in 80-125 ms (test build,
  optimized + debuginfo).

### Against the recordings

The listener preferred recordings (`el_midair_hit`, `el_ground_slam`,
`el_wing_snap` in the library's sources; not in git). Measured as the test
measures (99 % roll-off over a stretch; power share under 200 Hz; band length
to -40 dB of the band's own peak, 50 ms frames):

| | roll-off 0-0.1 / 0.1-0.2 / 0.3-0.4 / 0.6-0.7 / 1.1-1.3 s | under 200 Hz | 20-120 Hz | 0.5-2 kHz | 4-16 kHz |
|---|---|---|---|---|---|
| el_midair_hit__1 | 4986 / 3645 / 1008 / 217 / 144 Hz | 98 % | 1.70 s | 0.85 s | 0.55 s |
| el_midair_hit__2 | 3762 / 2836 / 1834 / 1430 / 141 Hz | 97 % | 2.05 s | 0.80 s | 0.60 s |
| el_ground_slam__2 | 10635 / 7336 / 3656 / 105 / 79 Hz | 99 % | 1.60 s | 0.35 s | 0.40 s |
| el_wing_snap__1 | 5619 / 8824 / 4014 / 533 / (noise) | 85 % | 0.70 s | 0.45 s | 0.50 s |
| the test's heavy hit | 13670 / 1658 / 826 / 428 / 264 Hz | 96 % | 2.00 s | 1.05 s | 0.30 s |

The heavy hit (`a_heavy_hit_has_the_shape_of_the_recordings`): an impulse
plus noise x `decay` 0.28 s at 0.07 into a `metal` shell 4 m x 40 m,
thickness 0.002, loss 0.07, panel 3, edge_loss 0.075, tilt -2.5; a `rumble`
top 115, flutter 16 Hz depth 0.6, onset 0.03, decay 2.5, mixed at 1.8. It
was found by a local search on `sfx_analysis::reference::compare` (the
`listen` reference distance, multi-resolution STFT): **9.3 dB** from
`el_midair_hit__1` (10.5 from `__2`, 13.4 from `el_ground_slam__2`, 11.2 from
`el_wing_snap__1`). About 1.5 dB of that is the recording's noise floor: a
pink floor at 0.003 added to ours gives 7.8 dB. Two noises of the same
spectrogram differ by about 6 dB on this measure, so 6 is the floor. The
recordings' highs last longer than this physics gives them (their decay is
flatter in frequency; part of it is the recording's room and floor), and ours
is brighter in the first 0.1 s. These need the listener's ears.

### Recipe

An airliner fuselage struck hard: start from the heavy hit above. For a
scrape along it, feed `friction` or gated noise and leave the rumble to
`follow` the structure.

## `rumble`

Pitchless noise under `top`: white noise through a one-pole at `bottom`
(-6 dB per octave above it), a 2nd-order high-pass at `bottom` and a
4th-order Butterworth low-pass at `top`, normalized to 0.3 RMS, times a
smooth random flutter (new targets at jittered times about `flutter_rate` a
second, joined by raised cosines, up to 9 dB either way at depth 1) and an
envelope: a raised-cosine `onset` from `at`, then an exponential fall of 60 dB
in `decay` x energy^0.25; or, with `follow`, a peak follower of that signal
(attack onset / 3 time constant, release falling 60 dB in the decay). No sine
anywhere.

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `top` | number | 120 | Hz, 20 .. 1000 | Highest frequency. |
| `bottom` | number | 25 (or top / 1.5) | Hz, 10 .. top / 1.5 | Lowest frequency. |
| `flutter_rate` | number | 12 | Hz, 0 .. 60 | How fast the level wanders; 0 steady. Alias `rate`. |
| `flutter_depth` | number | 0.5 | 0 .. 1 | How far: 9 dB either way at 1. Alias `depth`. |
| `onset` | number | 0.04 | s, 0 .. 5 | Rise time. |
| `decay` | number | 3 | s, 0.05 .. 30 | Time to fall 60 dB. |
| `energy` | number | 1 | 0.01 .. 100 | Level x sqrt(energy), decay x energy^0.25. |
| `follow` | audio | none | | Track this signal's level instead of one hit (not with `at`). |
| `at` | number | 0 | s | When the one hit starts. |
| `gain_db` | number | 0 | -60 .. 60 | Level. |
| `seed` | integer | 0 | | The noise and the flutter. |

Measurements: energy above 2 x top 0.05 % (top 40 Hz), 0.01 % (120 Hz), under
0.01 % (400 Hz), with flutter depth 0 or 1 at 20 Hz; the strongest bin in
25-120 Hz stands 7.9 dB over its neighbours' median (no pitch); it lasts
1.75 s (energy 1) and 2.55 s (energy 4, +7.4 dB) to -40 dB in 20-200 Hz;
following a 0.3 s `decay` at 1 s it is silent before (-120 dB) and -19 dB RMS
over 1.1-1.6 s.
