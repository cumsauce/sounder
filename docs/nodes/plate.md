# Plate

## `plate`

A thin plate struck hard enough to stretch (physical models, nonlinear). It is
`modal`'s plate (same material names, size / `f0`, thickness, loss, modes,
strike position, mistuning and normalization) with its modes coupled, so a
harder strike is not only louder: the mode frequencies rise with the energy and
glide down as it decays, and energy moves from the struck low modes into higher
ones within the first tens of milliseconds, where their larger loss lets them
die first. At `drive` 0 it is `modal`'s plate. Code:
`crates/sfx_graph/src/nodes/plate.rs`; tests: `crates/sfx_analysis/tests/it/plate.rs`.

Why it exists: three passes of "massive metal" (aircraft colliding, structure
tearing) built from the linear `modal` / `impact` / `rig` were rejected by ear:
damped modes read as wood, clean ringing modes as a gong or a UI tone. A hard
hit on big sheet metal is a broadband burst with no stable partials, which a
linear plate cannot make (louder is all a harder hit does there).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `excite` | audio | required | | The strike: `contact`, `impulse`, a noise burst, a `pattern` of hits. Its overall scale does not matter (see `drive`). |
| `material` | name or constants | required | as `modal` | `metal` is the one this is for. |
| `size` / `f0` | signal, read once | one required | as `modal` | Side of the square plate (pitch as 1 / size^2), or the lowest mode in Hz (20..18000). Both, or neither, is an error; a size with no mode in the band names the range. |
| `thickness` | signal, read once | the material's | 0.0001..0.5 m | As `modal`. |
| `loss` | number | 1 | 0.01..100 | Multiplier on the material's loss. |
| `modes` | integer | 128 | 1..256 | The lowest half and the strongest of the rest. The coupling needs modes above the struck ones to move energy into. |
| `strike_position` | signal, read once | 0.3 | 0..1 | As `modal` (0.5 is the centre). |
| `tilt` | number | 0 | -12..12 dB/oct | As `modal`. |
| `drive` | number | 1 | 0..10 | How hard the **loudest** strike in `excite` drives the plate. Alias `strength`. 0 is linear, 0.25 measures as linear, 1 a hard hit, 2-4 a crash. The nonlinearity goes as drive^2 x (this strike's level / the loudest's)^2, so a strike 20 dB under the loudest is at 1 / 100 of the strength. |
| `cascade` | number | 1 | 0..4 | How much of the nonlinearity moves energy between modes. 0 keeps the tension only: a pitch glide with no crash. |
| `gain_db` | number | 0 | -60..60 | Output level; does not change how hard the plate is driven. |
| `seed` | integer | 0 | | Mixed with the document seed and the node name: the mistuning (as `modal`) and the coupling. |

Errors: `drive: 11: give 0..10 (0 = the linear plate, 1 = a hard hit)`,
`cascade: -1: give 0..4 (0 = pitch glide only, 1 = the default crash)`, and
every `modal` error for the shared fields.

The node is random (`seed`) and not causal: the loudest strike in the whole
input is found first (a linear pass), so it cannot run live.

### How it works

The modal von Karman plate's nonlinear energy is a sum over in-plane stress
modes of the square of a quadratic form in the transverse modes (Ducceschi and
Touze; Bilbao, "Numerical Sound Synthesis", the chapter on nonlinear plates).
This node keeps that shape and makes it cheap:

- a tension form s_0 = sum_k (f_k / f_1) q_k^2 (Berger's plate): every mode
  stiffens with the total stretching, which is the pitch glide;
- 24 sparse random forms of 48 mode pairs each, a pair (p, q) allowed when its
  wavenumbers can meet the form's (|k_p - k_q| <= K <= k_p + k_q, k = sqrt f),
  weighted k_p^2 k_q^2 and tapered as 1 / (1 + (f / 5 kHz)^2) (a quarter of
  the band at 48 kHz): they carry energy between modes;
- V = kappa / 4 (s_0^2 + 400 cascade sum s_n^2), never negative.

It is stepped with the scalar auxiliary variable method (Bilbao, Ducceschi and
Zama, "Explicit exactly energy-conserving methods for Hamiltonian systems"):
psi = sqrt(2 V) is a state of its own, and each sample solves a rank-one
linear system exactly (Sherman-Morrison), O(modes + pairs). The discrete
energy (kinetic + linear potential + psi^2 / 2) is a sum of non-negative terms
that only the loss lowers and only the input raises, so the state stays bounded
for any `drive`. psi is clamped into 0..sqrt(2 V) every sample (it is
integrated and drifts; above that it once pushed a late burst of highs), which
only lowers the energy. Each mode's linear part is its exact two-pole resonator
written as a damped Verlet step, stable for every mode under the top of the
band. A non-finite sample (only an input beyond f64) resets the state to rest.

The input is scaled so the loudest strike's linear ring stretches the plate to
s_0 = 1, and the output scaled back: `drive` means the same for a light tap and
a heavy `contact`, and at 0 the output is `modal`'s.

### Recipe

A steel sheet hit hard by a heavy mallet (`peak_db`: a hit). Knobs on `drive`
(how hard), `size` or `f0` (how big) and `loss`.

```json
[
  {"type": "contact", "name": "blow", "speed": 4, "mass": 1, "material": "metal", "hardness": 0.5},
  {"type": "plate", "name": "sheet", "excite": "blow", "material": "metal", "f0": 60,
   "thickness": 0.002, "drive": 2, "modes": 128},
  {"type": "output", "name": "out", "input": "sheet", "peak_db": -1, "fade_in": 0.0005}
]
```

### Measured

Tests in `crates/sfx_analysis/tests/it/plate.rs`, 48 kHz, the recipe's strike
(`contact` 1 kg at 4 m/s, hardness 0.5, on metal, `f0` 60, thickness 2 mm,
128 modes, seed 1) unless the row says otherwise. Centroids are of the
whole-window spectrum.

| What | Measured | Test |
|---|---|---|
| `drive` 0 against `modal` with the same fields | relative rms difference 7.9e-4 over the first 100 ms, 5.6e-3 over 1 s (modal's resonators are f32, these f64) | `plate_soft_strike_is_the_linear_plate` |
| `drive` 0.25 against `modal` | centroid 30-80 ms 315 Hz (modal 314), level -0.14 dB | same |
| centroid 30-80 ms at `drive` 0 / 1 / 2 / 4 | 314 / 509 / 819 / 1004 Hz (each step > +10 %) | `plate_centroid_rises_with_drive` |
| centroid over 0-20, 30-80, 100-300, 300-600, 1000-1500 ms, `drive` 2 | 1118, 819, 164, 116, 73 Hz (linear: 393, 314, 280, 141, 85) | `plate_hard_strike_darkens_as_it_decays` |
| lowest mode, `drive` 2, `cascade` 0 | 64.35 Hz over 0-0.4 s, 60.75 Hz over 2-3 s, linear 60.65 Hz: 103 cents sharp early, 3 cents late | `plate_pitch_glides_down` |
| two strikes 20 dB apart in one input (`loss` 4, 2 s apart), `drive` 2: centroid 5-50 ms after each | loud 371 Hz (linear 284), soft 199 Hz (linear 199) | `plate_softer_strike_in_one_input_is_more_linear` |
| `drive` 0 / 1 / 10 x `cascade` 0 / 4 x (`loss` 0.01, 256 modes, 0.5 mm), (`loss` 100, 1 mode, 2 cm), (1, 64, 2 mm), struck at 0.9 by an impulse of 100 and by white noise, 0.5 s | every sample finite; largest peak 100.1 (the impulse of 100 at drive 0) | `plate_is_finite_across_the_parameter_range` |
| two renders, one thread vs the pool; seed 1 vs 2 | same bits; different | `plate_is_deterministic` |
| largest: `f0` 20 Hz, 1 mm, 256 modes, `drive` 10, `cascade` 4, 3 s | 0.59 s to render (test profile, opt-level 3, run alone), peak 5.57, finite | `plate_largest_size_and_modes_render_in_time` |

While tuning (same strike): forms weighted k_p k_q barely moved the
centroid (20-50 ms: +14 % at `drive` 4 with a share of 30); weighted
k_p^2 k_q^2 without the taper, `drive` 4 put the energy at the top of the band
within 20 ms and one seed held a 16-23 kHz centroid for the whole second; a
taper at 10 kHz (instead of 5) held 3.5-7 kHz past 300 ms at `drive` 8,
`cascade` 4.

### Honest limits

- None of this has been heard. The numbers show a burst that brightens with
  `drive` and darkens as it decays, and a glide; not that a listener will hear
  big sheet metal rather than noise or a gong.
- The coupling is random, not the plate's own coupling coefficients: which
  modes trade energy is chosen by a wavenumber rule and a seed. Different
  seeds give different crashes (the 30-80 ms centroid at `drive` 2 was 819 Hz
  for seed 1 and about 920 Hz for seed 2 while tuning).
- The brightening is strong at the start: at `drive` 2 the first 20 ms are the
  brightest window, and it is already falling by 30-80 ms. The "rise over
  20-80 ms" of a real crash is not a separate stage here.
- At `drive` 2 the 100-300 ms window is darker than the linear plate's (164 vs
  280 Hz): energy drawn out of the mid modes into highs that then die. More
  `loss` makes the bright part shorter still.
- At `drive` 8 and above the bright part may not rise further (30-80 ms: 1004
  Hz at 4, about 950 at 8 for seed 1 while tuning): energy pushed into highs
  dies there sooner.
- The taper corner follows the sample rate (a quarter of the band), so a
  44.1 kHz render couples a little less above 4.5 kHz than a 48 kHz one.
- Square, simply supported plate only (`modal`'s plate table): no shell, no
  free edges, no rectangle.
- Not causal: the loudest strike is found over the whole input.
