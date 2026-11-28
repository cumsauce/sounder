# Nodes: the short reference

One section per node family: fields, a recipe, measured limits. The long form
of each (tables, every error, every measured number) is the family's own file
in this folder; `describe_op <type>` has the schema and an example.

## Physical models: modal, contact, friction

```json
{"type": "contact", "name": "blow", "speed": 2, "mass": 0.1, "material": "wood", "hardness": 0.7}
{"type": "modal", "name": "panel", "excite": "blow", "material": "wood", "size": 0.5, "strike_position": 0.3}
{"type": "friction", "name": "hinge", "speed": "swing", "pressure": 0.8, "roughness": 0.3}
```

- `plate` {excite, material, `size` or `f0`, [thickness] [loss 1] [modes 128] [strike_position 0.3] [tilt 0] (as `modal`), [drive 1] (0..10, alias `strength`; for the loudest strike in `excite`), [cascade 1] (0..4), [gain_db], [seed]}: `modal`'s plate with its modes coupled, so a hard strike brightens within tens of ms, glides down in pitch and darkens as it decays; drive 0 is `modal`. Not causal (the loudest strike is found first). Details: `docs/nodes/plate.md`.
- `structure` {excite, material, size (m, plate side / shell diameter), [shape plate|shell], [length], [thickness], [loss 1], [edge_loss 0.1], [panel], [tilt 0], [low_modes 32], [gain_db], [seed]} (`docs/nodes/structure.md`): a body too big to hear as partials, the excitation convolved with noise whose decay follows T60(f) from the body's loss (material, radiation, joints); the few lowest modes discrete where sparse; a unit impulse peaks at 0 dBFS.
- `rumble` {[top 120], [bottom 25], [flutter_rate 12], [flutter_depth 0.5], [onset 0.04], [decay 3], [energy 1], [follow] or [at], [gain_db], [seed]} (`docs/nodes/structure.md`): pitchless low noise under `top` with a slow random flutter, one hit or following a signal.
- Limits: `impact` masses 0.001..100000 kg, `speed` 0.01..300 m/s, `height` 0.001..1000 m, `radius` 0.0001..100 m; `rig` body `mass` 0.0001..100000 kg, `velocity` -300..300 m/s, contact `gap` 0..1000 m, `radius` 0.0001..100 m (`docs/nodes/modal.md`, `docs/nodes/rig.md`).
- `rig` {bodies: [{name, mass, [fixed], [velocity], [drive], [material ...
  as modal], [level_db], [waveguide {kind string | tube, f0 | length
  [tension], [ends open | closed | stopped], [radius 0.02], [decay],
  [damping 0.3]}]}], joints: [{kind contact | spring | air, a, b | "ground",
  [at_a], [at_b], [gap], [hardness] [radius] [bounce] [damp] [rebound] [slide]
  [friction] [stribeck] | [stiffness] [damping] | [strength 1]}], [gravity 9.81], [oversample], [at], [gain_db], [seed]}: several
  bodies joined by contacts and springs (a spring with a `gap` is a rattle),
  stepped together; a body may be a string or tube waveguide, and an `air`
  joint drives a tube from a body's vibration. Details: `docs/nodes/rig.md`.
- `impact` {material, shape, `size` or `f0`, [loss] [tilt] [noise] [noise_width]
  [strike_position] [modes 32] (as `modal`), [object_mass 0.5], [striker_mass]
  (left out: the object is dropped onto [floor "stone"]), [hardness 0.7] (a
  striker's), `height` (gravity on; a drop defaults to 0.3) or `speed`,
  [radius 0.01], [bounce 0.5], [scatter 0.3], [max_bounces 40], [oversample 4],
  [at], [gain_db], [seed], [drive] (a force signal on the body), [excite]
  (audio on the modes), [pitch] [damping] [stiffness] (signals), [slide] (m/s
  along the surface), [roughness] [grain] [octaves] [wobble] (a roll), [friction] [stribeck]
  (stick-slip: scrape, creak, bow)}: the strike solved as one system, a point mass and
  the modes coupled per sample by a Hunt-Crossley contact. Bounces, chatter and
  buzz on the floor come from it; port `force` is the contact force above the resting load (0 at rest), `contact` the raw force; [rebound "exact"]. Heard in batches 22-24, 30:
  details in `docs/nodes/modal.md`, "`impact`".
- `modal` {excite, material, **`size` or `f0`** (one of the two), [strike_position
  0.3], [modes 48], [loss 1], [tilt 0], [thickness], [f_min], [f_max], [seed]}:
  a simply supported square plate `size` metres a side. `material` is a foley
  name (`stone wood metal glass ice earth granular plant soft snow`) or
  `{young, density, poisson, loss, thickness}`; the `thickness` field
  (metres, 0.0001..0.5, read once) overrides the material's own, so a small
  thin object needs no custom material (every mode frequency goes linearly
  with it; giving the material's own value changes nothing). Mode Hz = pi/2 * thickness *
  c_L / sqrt(12) * (m^2 + n^2) / size^2, so pitch goes as 1 / size^2 and a
  plate's thickness sets its register (wood 2.5 cm, size 0.5: lowest mode
  ~425 Hz; glass 1 cm, size 0.5: ~200 Hz). T60 per mode = ln(1000) / (pi f
  loss): high modes die first, metal (loss 0.004) rings several times longer
  than wood (0.025). `strike_position` weights each mode by
  |sin(m pi x0) sin(n pi y0)| (0.5 is the centre: modes with a node there
  vanish); a seeded 1.5 % mistuning breaks the symmetry. `size`,
  `strike_position` are read once (at the start), so `"$size"` / `"=expr"`
  work (a `voices` patch can randomize them) but a ramp will not glide them.
  The `excite` signal is the strike: modal is linear, so a `contact`
  pulse, an `impulse`, a noise burst or a `friction` stream all work.
  Stereo excite rings each channel alone.
  **`f0`** (Hz, 20..18000, a number, `"$param"` or `"=expr"`, read once) sets
  `size` so the lowest mode of the shape's table sits at `f0` ("lowest mode 2
  kHz" is one field; measured within 0.22 % for a plate, 0.04-0.10 % for the
  other shapes, in `tests/it/modal.rs`). It replaces `size`: giving both is an error,
  giving neither is an error. The seeded mistuning can move the lowest mode by
  up to 1.5 % (plate). A macro that maps `f0` must not also map `size`.
  **`f_min` / `f_max`** (Hz, default none) keep only the modes inside the band,
  applied after the mode table and before the `modes` cap (so `modes` counts
  kept modes); the strongest kept mode is the level reference. An empty band is
  an error naming the table's range ("widen the band, or change `size` / `f0` /
  `thickness`"), and so is a band whose modes the strike does not excite (a
  node at `strike_position`). With neither field the output is bit-identical to
  before. Use them for glass, ice, coins and keys: `f0` where the object rings,
  `f_max` to keep the top of the table out. Detail: `docs/nodes/modal.md`.
  **`modal` normalizes its output**: a unit impulse (1.0 at one sample, or a
  `contact` of mass x speed = 0.1) peaks near 0 dBFS (measured within 0.01 dB
  over 10 materials x 6 shapes x 3 sizes); a stronger strike is louder. It has
  `shape` (`plate bar tube membrane string shell`) and `stiffness`; the lowest
  mode falls with size (the `modes` bank always keeps the lowest half).
  See "Modal shapes and `shatter`" below for the other shapes.
  **`loss`** (0.01..100, default 1) multiplies the material's loss, named or
  `{young, ...}`: `"material": "wood", "loss": 0.5` rings twice as long (the
  20 dB decay time measured 1.95x), 2 half as long; a mode's T60 is capped at
  20 s. **`tilt`** (dB per octave, -12..12, default 0) multiplies each kept
  mode's gain by `10^(tilt log2(f / f_lowest) / 20)`: positive lifts the upper
  modes (a glass plate at `strike_position` 0.31 puts 98.6 % of its energy in
  the lowest mode; `tilt` 6 brings that to 91 %, 12 to 57 %), negative
  darkens; the unit-impulse level stays near 0 dBFS. **`size` range**: the
  lowest mode must fall between 24 Hz and 90 % of the top of the band (18 kHz
  at 48 kHz), which makes 3 mm glass usable from 0.029 m to 0.78 m and a metal
  plate from 0.12 m; outside it the error gives the range, and a size that
  leaves no mode in band says which thickness would fix it (pass `{young,
  density, poisson, loss, thickness}`). Modes above the band are dropped, not
  folded. **Honest limit of `thickness`**: it is the plate formula, a pitch
  knob, not a measured coin or key. A 25 mm metal coin of 1.5 mm has one mode,
  at 11.5 kHz, and none at 3-9 kHz; an 8 cm disc of the same thickness has four
  between 3 and 9 kHz. An object that must ring in that band needs a `size` and
  `thickness` whose lowest mode sits near 1 kHz (give the human a macro on
  `thickness`, exp curve).
- `contact` {speed, mass, material, [hardness 0.7], [at]}: a half-sine force
  pulse as long as the Hertz contact time (foley's `hertz_time`) of a
  2000 kg/m3 ball of that mass whose stiffness runs from rubber (hardness 0)
  to steel (1) against the struck material. Heavier, slower, softer = longer
  and duller. Area is mass * speed / 0.1 (0.1 kg at 1 m/s = a unit impulse).
  `speed` and `mass` are read at `at`. `gain_db` (-60..60, default 0 =
  unchanged) scales the pulse; `modal` takes the same `gain_db` on its
  (normalized) output, so a macro can balance a struck layer with one field.
- `friction` {speed, pressure, [roughness 0], [grain 200], [seed]}: a
  stick-slip sawtooth (90 % climb, a smooth 10 % slip) at `speed * grain` Hz,
  times `pressure`; speed 0 is silence, negative speeds count as their
  magnitude. `roughness` 0..1 jitters each cycle's length and size and adds
  noise. Both fields are signals: a `ramp` on speed is a creak that rises, a
  `wander` on pressure makes it breathe.
- Recipes: door hinge = `ramp` -> `friction` -> `modal` wood size ~0.45
  (`examples/door_creak.rs`); glass = `contact` -> `modal` glass cut by a fast
  `decay`, then `poisson` (a ramped rate) -> `voices` of a thin-glass `modal`
  with `$rand` size (`examples/glass_break.rs`); a bell = `impulse` -> `modal`
  metal with a small `size`.
- Honest limits: I cannot hear these. Plates are idealized (square, simply
  supported); real doors, bells and glass differ, so treat `size` as a
  pitch knob (give the human a macro on it) rather than a physical dimension.
  Foley's mode table is private, so this is a reimplementation of its
  formulas (plus a gentle high-frequency tilt), not a call into it; the
  material table and `hertz_time` are foley's.

## Foley models

Six nodes in `nodes/foley.rs` wrap the user's `foley` crates (pinned rev in
`Cargo.toml`). Foley renders at the sample rate it is given (`*_at`), so they
follow the document's rate. `describe_op <type>` has the schema.

| Node | Fields (`[]` optional) | Notes |
|---|---|---|
| `bubble` | `radius_mm`, `[loudness 0.3]`, `[rise 0.1]`, `[at]`, `[jitter]`, `[seed]` | Minnaert resonance `f = 3.29 kHz / radius_mm` (2 mm 1.64 kHz, 4 mm 0.82 kHz), decays at pi f 0.045, rises in pitch; level ~ r^1.5. The radius is a number (foley cannot retune a ringing bubble): in `voices` use `map {"r": "1 + $rand * 4"}` and `"radius_mm": "$r"`. Under ~0.7 mm it rings above Nyquist and aliases. |
| `air` | `input`, `distance` (signal), `[humidity 50]`, `[celsius 15]`, `[level true]` | A 12 dB/oct low-pass at foley's `absorption_cutoff` (where `distance` m cost 6 dB; none while 20 kHz loses under 6 dB, i.e. below ~10 m) and a 1/distance level drop (0 dB inside 1 m). Both only fall as distance grows. Animate `distance` with a `ramp` for a fly-by. |
| `glottis` | `f0` (signal), `[tension 0.65]`, `[breath 0.05]`, `[vibrato_cents]`, `[vibrato_hz 5.5]`, `[amp]`, `[seed]` | Foley's Liljencrants-Fant flow derivative (`lf_pulse`; tension 1 = pressed Rd 0.3, 0 = breathy Rd 2.7) with seeded breath noise. Pitch reads exactly (YIN). Aliases above ~1.5 kHz. |
| `tract` | `input`, `[vowel a]`, `[vowel_to]`, `[morph]`, `[formants [[hz, bw]..]]`, `[scale]` | **Not foley's waveguide** (its `Tract` is private): a 4-formant Klatt cascade at Peterson-Barney vowels `a e i o u ae uh schwa`. `vowel_to` + `morph` (signal) glide; `scale` > 1 shortens the tract (child, small creature), < 1 lengthens. Feed it a `glottis`. |
| `syrinx` | `species` or `pitch`; `[harmonic] [waver]` (whistle); `[rate] [at]` (species); `[amp] [seed]` | `species`: foley's `birdsong2`, any of 57 (`robin`, `cardinal`, `dove`, `crow`, `wood_thrush`, `boreal_owl`...; a bad name lists all), a phrase of its own length placed at `at`, cut/padded to the document; each `seed` is another individual; `rate` resamples (pitch and speed). `pitch`: a whistle from a contour signal (sine + 2nd harmonic + waver, as foley's songbirds). |
| `thunder` | `[distance 1500]`, `[celsius]`, `[humidity 90]`, `[exposure 1]`, `[brightness 0.5]`, `[length 1]`, `[rumble 0]`, `[tail 0]`, `[at]`, `[amp]`, `[seed]` | Foley's `strike` (Ribner-Roy N-wave over a jagged channel). Silent until `distance / 343` s, then clap and a rumble of several seconds with centroid ~30-50 Hz: set `duration` to fit (a 1.5 km flash: ~14 s). Foley levels every flash to a similar peak, so distance changes timing, brightness and length, not loudness. `exposure` 0 = shut in (low-pass to 200 Hz). `brightness` 0..1 (0.5 = foley's spectrum, bit-identical) is a +-24 dB high shelf at 100 Hz, peak kept: above 0.5 the centroid rises. `length` 0.5..3 (1 = as foley makes it, bit-identical) stretches the rumble after the first 0.1 s clap by overlap-add of 120 ms grains, so the spectrum stays and the rumble lasts `length` times as long (a longer `duration`). `rumble` 0..1 (0 = off, bit-identical) adds a continuous bed of seeded noise band-limited to 25-90 Hz from the first sound to the end of the audible tail, so the tail is not isolated spikes; its rms is `sqrt(rumble)` times the rms of the flash's loudest 0.4 s (0.6 is 2.2 dB under) and falls 3 dB over the tail; it is added before `exposure`, `brightness` and `amp` and the level is not re-normalised, so the flash gets louder with `rumble`. `tail` 0..1 (0 = off, bit-identical) is an exponential fade of the whole output: the gain falls in dB, linearly from the first sound, reaching -40 x `tail` dB at the end of the audible tail (last sample over -60 dB of the peak) and carrying on at that slope; it runs after `length` / `rumble` and before `exposure`, `brightness`, `amp`, and lowers the peak (re-level with `amp` or the output target). Measured at 1500 m, 16 s, audible length (10 ms rms over -45 dBFS) 7.19 / 6.54 / 6.53 / 4.80 / 4.80 s at `tail` 0 / .25 / .5 / .75 / 1: non-increasing, not strict (foley's tail is spikes). `listen`'s `decay -20 dB` is NOT monotone from `tail` 0 to 0.25 (1.84 s to 4.93 s): it counts from the loudest window, which sits in the rumble with no fade and moves to the clap with one; so read the audible length or the loudness curve when tuning a tail (a `decay` claim on a `tail` knob can fail verify's sweep at the 0 to 0.25 step: map `tail` from 0.25 up, or leave the claim off). Measured at 1500 m: the 400 ms rms across the mid-tail spans 18-50 dB at 0, 2.2-5.9 dB at 0.6; centroid stays under 100 Hz (`docs/nodes/thunder.md`). |

`species` / `thunder` are finished one-shots, so they are not causal for a
live runtime (`summary` says so); the rest are. All seeded nodes are
bit-identical at any thread count.

Recipes: **drips** = `poisson` -> `voices` of a `bubble` with a `$rand` radius,
then `air` (`examples/water_drips.rs`); **a vowel** = `glottis` (f0 from a
`wander` or `ramp` for intonation) -> `tract`; **a creature call** = a
`glottis` with a falling `ramp` f0, `tract` `scale` 0.6, `vowel_to` + a
`morph` envelope; **a distant storm** = `thunder` `distance` 3000 through
`air` (more highs gone) and `reverb`.

`glottis` and `tract` have more fields (roughness, subharmonic, fry; nasal,
frication, growl): see "Voice upgrades" below.

Honest limits: none of this has been heard. Measured: bubble peak within
0.2 % of Minnaert, glottis f0 within 1 cent, `tract` /a/ peaks at 768 Hz and
/i/ at 223 Hz (f0 110: harmonic spacing), `air` 100 m takes the white-noise
centroid from 12 kHz to 3.3 kHz and the level by 46 dB, thunder at 1.5 km
arrives at 4.4 s with centroid 32 Hz. Whether they sound like water, a voice,
a bird or a storm is the human's call.

## More nodes, by group

Each group below: the fields (`[x]` is optional, a default after it), one
recipe, and the limits that were measured. The long form (tables, every error
message, every measured number) is the fragment in `docs/nodes/`; `describe_op
<type>` has the schema and an example. A signal field takes a number,
`"$param"`, `"=expr"` or a node; a **number** outside a field's range is an
error that names the range, a **signal** is clamped per sample. Every number
quoted is a measurement of a test signal; none of it has been heard, and a
name like "bell" or "engine" says what the node computes, not how it sounds.

**Batch 2 blind-label fixes** (2026-11-18; defaults changed, not yet re-heard):
`fire` crackle is unpitched noise snaps high-passed at 700 Hz (the old ringing
pops read as "gurgling liquid"), the roar flutters, the hiss is lower; `spark`
and `splash` were later rebuilt from Farnell's recipes (book check 1, below in
their sections); `cloth` is a continuous rubbing bed with slow swells (crest 26 to
13-16 dB, fine onsets 30/s to under 1/s; it read as metal pellets); `water`'s
hiss no longer reads as wind; `glottis`, `tract`, `bubble`
take `natural` (default true: jitter, shimmer, drift, alias-free pulse, breath,
wider drifting formants, a noise tick on each bubble; `false` is the old
output); a `suppressed` `gunshot` is a gas puff plus action clacks with no
pitched thump, and a gunshot's air low-pass is `16 kHz / (1 + d / 400 m)`;
`engine` fires vary per cycle with combustion noise and a softer exhaust comb,
`motor` drifts in speed with brush and bearing noise. Details and before/after
numbers: `docs/nodes/{elements,surface,water,voice,blast,machines}.md`.

Blocks marked `<!-- recipe -->` (here, in `docs/graph.md` and in `AGENTS.md`) are rendered by
`crates/sfx_analysis/tests/it/recipes.rs` (an `output` is added when the
block has none), so they stay valid.

### Sources and oscillators (`docs/nodes/sources.md`, `synth.md`)

- `osc` new fields: `sync` (a mono signal; each rising zero crossing resets
  the phase, polyBLEP, crossing placed to a fraction of a sample), `unison`
  1..16 (voices are **summed**: 7 voices measure 5.45 dB over one, so set `amp`
  near 1/sqrt(unison)), `detune` cents 0..1200 (voices spread evenly from -d
  to +d), `spread` 0..1 (above 0 the output is stereo), `pm` (phase
  modulation in cycles; an `osc` here is FM), `seed` (unison start phases;
  more than one voice makes the node random).
- `noise` `color`: `white pink brown blue violet velvet crackle`. `blue` is
  +3 dB/oct, `violet` +6 dB/oct (measured 2.98 and 5.99 dB/oct). `density`
  (velvet default 2000, crackle 50 impulses a second) is a signal; crackle is
  Poisson with random sign and height.
- `fm` {freq, [ratio 1], [index 0 radians, signal], [feedback 0], [mod_ratio
  1], [mod_shape sine|saw|square|tri], [amp]}: two-operator; sideband k is
  J_k(index) (measured to 0.01 dB). Negative `feedback` errors.
- `wavetable` {freq, harmonics [up to 256 amplitudes], [harmonics_to],
  [morph 0..1, signal], [phases], [amp]}: additive, harmonics fade out toward
  Nyquist so a sweep never aliases (5 kHz, 32 harmonics: alias -63.6 dB).
- `impulse` `at` is a number or a list of times (seconds).

<!-- recipe -->
```json
[
  {"type": "ramp", "name": "pitch", "from": 400, "to": 180, "time": 0.4, "curve": "exp"},
  {"type": "osc", "name": "stack", "shape": "saw", "freq": "pitch", "unison": 7, "detune": 18, "spread": 0.6, "amp": 0.3},
  {"type": "decay", "name": "fall", "time": 0.25},
  {"type": "math", "name": "zap", "op": "mul", "a": "stack", "b": "fall"}
]
```

Macros: `detune` (width), `spread`, the ramp's `to`. Limits: a synced saw adds no
aliasing beyond a plain polyBLEP saw, but that saw already reads -41.6 dB off
the grid at 110 Hz and -29.8 dB at 1999 Hz; `fm` and large `pm` are not
oversampled, so a big index at a high `freq` aliases (not measured); YIN reads
the slave's rate on a sync sweep far above the master.

### Filters, EQ, ladder (`docs/nodes/filters.md`)

- `filter` new `slope` 12 | 24 | 48 dB/oct (`lp` / `hp` only; not in a
  feedback loop). `q` retunes the last section, the default is Butterworth.
  Measured one octave above the cutoff: -12.4 / -24.2 / -48.4 dB.
- `ladder` {input, freq, [resonance 0], [drive 1], [mode lp|bp|hp]}: four-pole
  ladder with a `tanh` per stage. `resonance` runs 0..1.2 and about 1
  self-oscillates at `freq` (measured within 0.14 %) but only if something
  excites it. Passband gain falls as 1 / (1 + 4 resonance) (no make-up).
- `eq` {input, bands: [{mode lp|hp|bp|notch|peak|lowshelf|highshelf, freq,
  [gain_db 0], [q 0.707]}, ...]} up to 16 bands; plain numbers, so a macro
  cannot reach inside a band (use `filter` nodes for a knob).

<!-- recipe -->
```json
[
  {"type": "noise", "name": "n", "color": "white", "amp": 0.3},
  {"type": "ramp", "name": "sweep", "from": 6000, "to": 300, "time": 0.6, "curve": "exp"},
  {"type": "ladder", "name": "acid", "input": "n", "freq": "sweep", "resonance": 0.85, "drive": 2},
  {"type": "eq", "name": "voiced", "input": "acid", "bands": [{"mode": "hp", "freq": 80}, {"mode": "peak", "freq": 1000, "gain_db": 6, "q": 2}]}
]
```

Limits: the ladder's `tanh` is not oversampled (high `drive` folds back); at
the default `q` an `eq` peak is wide (+0.78 dB two octaves away), narrow it
with `q` 4 or more; a 48 dB filter at a few Hz has an f32 noise floor.

### Dynamics (`docs/nodes/dynamics.md`, `nonlinear.md`)

- `transient` {input, [attack_db 0], [sustain_db 0], [speed 0.02]}: gain on
  rising and on falling level (+-24 dB); a steady plateau is untouched.
- `gate` {input, threshold_db, [ratio] (omit: full gate), [attack 0.001],
  [release 0.05], [hold 0.01], [key]}: downward expander with sidechain.
- `sub` {input (mono), [octaves 1|2], [mix 0.5], [lowpass 120]}: adds a sine
  one or two octaves below the input's low band (monophonic divider).
- `exciter` {input, [freq 3000], [drive 2], [mix 0.3]}: saturates the band
  above `freq` and adds it back (odd harmonics, no oversampling).
- `compress` new `key`: a node that drives the detector (ducking, pumping);
  the gain is applied to `input`.

<!-- recipe -->
```json
[
  {"type": "pattern", "name": "hits", "every": 0.5, "count": 4},
  {"type": "trigger_env", "name": "thump", "events": "hits", "attack": 0.001, "decay": 0.2},
  {"type": "osc", "name": "tone", "freq": 60},
  {"type": "math", "name": "kick", "op": "mul", "a": "tone", "b": "thump"},
  {"type": "osc", "name": "pad_osc", "shape": "saw", "freq": 110, "amp": 0.3},
  {"type": "compress", "name": "duck", "input": "pad_osc", "key": "kick", "threshold_db": -24, "ratio": 6, "attack": 0.002, "release": 0.15},
  {"type": "mix", "name": "both", "inputs": ["duck", "kick"]}
]
```

Measured: `compress` keyed by a pulse at 4:1 gave -15.00 dB in the pulses (20 dB
over) and -0.09 dB between; `gate` took a -50 dB bed to -95 dBFS and moved the
bursts 0.000 dB; `transient` +6 dB attack raised a noise burst's crest 9.89 to
14.45 dB. Limits: `transient` ripples on steady tones under ~100 Hz; `gate` has
no lookahead; `sub` needs the note under ~`lowpass`.

### The finishing bus (`docs/nodes/finish.md`)

- `finish` {input, [punch 0..1], [tail_db -12..12], [glue 0..1], [warmth 0..1],
  [weight_db -12..12], [air_db -12..12], [width 0..2, stereo input only]}: the
  bus of "Layering a designed sound" in one node. Each stage runs only when
  its field is set (all off: the input bit for bit): `transient` attack
  (8 dB x punch, 5 ms detector), `transient` sustain (`tail_db`), `compress`
  (threshold 4 + 16 x glue dB under the peak, ratio 1.5 + 2.5 x glue, instant
  attack), `shape` tube oversampled 4x plus a 20 Hz hp, shelves at 120 Hz and
  6 kHz, and on a stereo input the band under 150 Hz summed to mono with
  `width` above. No reverb. `width` on a mono input is an error.

<!-- recipe 0.6 -->
```json
[
  {"type": "noise", "name": "n", "color": "pink", "seed": 4},
  {"type": "pattern", "name": "taps", "times": [0.01, 0.3]},
  {"type": "trigger_env", "name": "env", "events": "taps", "attack": 0.001, "decay": 0.25},
  {"type": "math", "name": "hits", "op": "mul", "a": "n", "b": "env"},
  {"type": "finish", "name": "fin", "input": "hits", "punch": 0.5, "tail_db": -6, "glue": 0.3, "warmth": 0.5, "weight_db": 3}
]
```

Measured (`tests/it/finish.rs`): crest 22.08 / 23.71 / 24.71 dB at punch 0 / .5 /
1 and 22.08 / 19.84 / 19.33 dB at glue 0 / .5 / 1; `tail_db` -6 took the tail
down 3.9 dB (two thirds: noise jitter reads as rising level) and left the
attack; shelves +5.9 dB where set, 0.01 dB at 1 kHz; width 2 doubled the side
above 150 Hz (+6.0 dB) with the side under 60 Hz 34 dB under the mid.

### Exaggeration and liveliness (`docs/nodes/exaggerate.md`)

- `modal` `noise` (0..1) and `noise_width` (0.05..2, default 0.25): each
  mode becomes (1 - noise) x its clean ring + noise x a band of white noise
  at its frequency, `noise_width` x f wide, following the ring's envelope
  and decay (at 1 no tone is left): **use it on real wood, stone and
  metal**, which a clean
  `modal` makes tonal (a crate read as a marimba, a lid as a tuned gong).
  Starting points the listener moved toward: **wood** `noise` 0.6, `tilt` 4,
  `loss` about 1.8 (3 was "a bit too short"); **thin metal** `noise` 0.85,
  `noise_width` 0.5, `tilt` 6 (a lid with less read tonal; at `noise` 1
  and width 1 its flatness went 0.40 to 0.74). Measured in
  `tests/it/modal.rs` and `docs/nodes/exaggerate.md`. Off by default
  (bit-identical). `tilt` takes down the one dominant partial; `loss` sets
  how long wood rings.
- `modal` `strike_glide` (cents, 0..1200): every mode starts sharp at the
  ring's loudest and settles with the level; `rattle` (0..1): the ring above
  a gap (its loudest 5 ms level at 0, 30 dB under at 1) added back
  high-passed above 1.5 kHz, a buzz that stops when the ring falls under the
  gap. Both off by default (bit-identical). Listened to: `strike_glide`
  about 35 cents made a hit sound harder; `rattle` on wood read as plastic
  (keep it off wood). Measured: `strike_glide` 100 on
  a metal bar, +45.5 cents at 20-110 ms and +2.1 at 1-1.5 s (the full glide
  only at the loudest instant); `rattle` 0.5 on a wood body, +14.7 dB at
  2-12 kHz in the first 100 ms, untouched once the ring is under the gap. A
  rattle reads on dull bodies; a metal bar already has the highs.
- `subdrop` {[from 90], [to 32], [time 0.6], [attack 0.004], [decay 1.2],
  [at], [drive 0..1], [amp 0.8]}: a sine sweeping down under a hit (`drive`
  adds harmonics for speakers with no sub; 90 to 32 Hz measured 86.3 Hz in
  the first 50 ms and 32.0 Hz after).
- `riser` {at (the hit), [length 1.5], [from 200], [to 2000], [noise 0.6],
  [shape 2], [release 0.01], [amp 0.8], [seed]}: noise through a rising
  band-pass plus a rising tone, swelling as (time / length)^shape to `at`
  and stopping (rms -40 / -20 / -11 dB across the swell, -120 dB after).
- Tonal on purpose: a cinematic hit with a `subdrop` read voiced 0.58, so
  the `layers` `impact` preset (realistic hits) fails it; give `layers` its
  own `voiced_max` and `body` for an exaggerated hit.

<!-- recipe 2.5 -->
```json
[
  {"type": "riser", "name": "rise", "at": 1.0, "length": 0.9, "from": 300, "to": 3000, "amp": 0.3},
  {"type": "noise", "name": "n", "color": "white", "seed": 2},
  {"type": "decay", "name": "ce", "time": 0.02, "at": 1.0},
  {"type": "math", "name": "crack", "op": "mul", "a": "n", "b": "ce"},
  {"type": "decay", "name": "be", "time": 0.012, "at": 1.0},
  {"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "be"},
  {"type": "modal", "name": "body", "excite": "burst", "material": "wood", "size": 0.6, "loss": 1.5, "rattle": 0.4},
  {"type": "subdrop", "name": "drop", "from": 90, "to": 30, "time": 0.8, "decay": 1.5, "at": 1.0, "drive": 0.3},
  {"type": "mix", "name": "all", "inputs": ["rise", {"input": "crack", "gain": 0.5}, "body", {"input": "drop", "gain": 0.7}]}
]
```

### Shape, oversampling, ring modulation (`docs/nodes/nonlinear.md`, `ringmod.md`)

- `shape` {input, [mode tanh|clip|fold|bitcrush|rectify|asym|diode|tube|table],
  [drive 1, signal], [bits 8] (bitcrush), [bias 0.5] (asym), [points [[x, y],
  ...]] (table, x strictly increasing), [oversample 1|2|4|8], [mix 1, signal]}.
  `oversample` is an error inside a feedback loop. `asym`, `diode`, `tube` leave
  DC: follow with an `onepole` hp.
- `ringmod` {input, freq (signal, +-sr/2), [shape sine|tri|square|saw], [mix 1]}:
  partials at f - freq and f + freq; the input frequency is removed.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "tone", "shape": "saw", "freq": 110},
  {"type": "shape", "name": "grit", "input": "tone", "mode": "asym", "drive": 4, "oversample": 4, "mix": 0.6},
  {"type": "onepole", "name": "clean", "input": "grit", "mode": "hp", "freq": 30},
  {"type": "ringmod", "name": "clang", "input": "clean", "freq": 90, "mix": 0.5}
]
```

Measured: a 5 kHz sine hard-clipped at drive 8 has 24.8 dB less off-grid
energy at `oversample` 4 and 45 dB less at 8; `asym` at drive 2 puts the 2nd
harmonic at -15.5 dBc (`tanh`: -115 dBc). Limits: oversampling removes content
above 0.45 x the sample rate and leaves up to half a sample of offset at `mix`
1; `bitcrush` and `fold` are meant to alias; a square or saw `ringmod` carrier
aliases (not measured).

### Modulation effects (`docs/nodes/modfx.md`)

- `chorus` {input (mono if `spread` > 0), [rate 0.8 Hz], [depth 0.003 s],
  [delay 0.02 s], [voices 3, 1..8], [spread 1], [mix 0.5]}.
- `flanger` {input, [rate 0.25], [depth 0.002], [delay 0.003], [feedback 0,
  -0.95..0.95], [mix 0.5], [manual] (seconds; replaces the LFO)}.
- `phaser` {input, [rate 0.5], [stages 4|6|8|12], [center 800 Hz], [depth 1
  octave], [feedback 0], [mix 0.5]}.
- `freq_shift` {input, shift (Hz, signal, +-sr/2), [mix 1]}: moves every
  partial by the same Hz, so harmonic sounds turn inharmonic (single sideband).

`mix` 0 returns the input bit for bit.

<!-- recipe -->
```json
[
  {"type": "noise", "name": "wash", "color": "pink", "seed": 3, "amp": 0.4},
  {"type": "phaser", "name": "swirl", "input": "wash", "rate": 0.3, "stages": 8, "center": 600, "depth": 1.5, "feedback": 0.5, "mix": 0.6},
  {"type": "mix", "name": "summed", "inputs": ["swirl"], "to": "mono"},
  {"type": "chorus", "name": "wide", "input": "summed", "spread": 1, "voices": 4, "mix": 0.5}
]
```

Limits: `freq_shift` image rejection was measured at 1 kHz only (48.5 dB); its
dry path is not delay-matched for `mix` between 0 and 1; the phaser's notches
are up to 2 % below the maths at high frequency; with the LFO running a 3 ms
echo reads 0.880 instead of 1.0 (cause not isolated).

### Space: reverb models and impulse responses (`docs/nodes/space.md`)

- `reverb` new `model` `fdn` (default, bit-identical to before) | `room` |
  `plate` | `spring`; `er_mix` 0..1 (`room` only: early reflections vs tail;
  an error for other models); `width` 0..2 (wet stereo width). Ports `out`
  (dry + wet) and `wet`. Fixed internal geometry: `size` only scales it.
- `ir` {[model room|tube|box|cabinet|phone], [size m], [absorption], [length
  s], [seed]}: a source whose output is an impulse response for `convolve`
  (`room` is stereo). `size` and `absorption` are errors for models that have
  no such thing (`phone`; `cabinet` has no `absorption`).

<!-- recipe -->
```json
[
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "decay", "name": "e", "time": 0.004},
  {"type": "math", "name": "snap", "op": "mul", "a": "n", "b": "e"},
  {"type": "ir", "name": "hall", "model": "room", "size": 14, "absorption": 0.15},
  {"type": "convolve", "name": "inroom", "input": "snap", "ir": "hall", "mix": 1.0},
  {"type": "reverb", "name": "boing", "input": "snap", "model": "spring", "size": 0.4, "decay": 1.2, "mix": 1.0},
  {"type": "mix", "name": "both", "inputs": ["inroom", "boing"], "to": "stereo"}
]
```

Limits: damping makes the broadband T60 up to 13 % shorter than `decay`
(it is the low-frequency time); `room` reflections are nearest-sample with
frequency-independent walls and the tail is the fdn (matched in T60, not in
spectrum); `tube` and `box` are comb filters; whether the spring's chirp
matches a real spring was not measured. An `ir` is at most the document's length.

### The camera: distance and space for a video (`docs/nodes/camera.md`)

- `camera` before `output`: `distance` (m, a number or signal), 1/r level
  past `ref_distance` (`inverse_law`, default on), ISO 9613-1 `air`
  absorption, optional `delay` (Doppler with a moving distance), a ground
  reflection from `source_height` / `camera_height` / `ground`, and `space`
  none | outdoor | street | room | hall (stereo; the room does not fall
  with distance). All off = bit-identical passthrough. For video / film
  renders only: game sounds stay dry. A `peak_db` / `lufs` output undoes the
  1/r level (use `gain_db`).

### Motion and stereo (`docs/nodes/motion.md`)

- `doppler` {input (mono), [speed 30 m/s], [closest 5 m], [at s], [pan true],
  [air true]}, or {input, distance (signal, 0..10290 m)}: a source passing on a
  straight line; pitch bends by the radial speed, level 1/r, optional air
  low-pass and bearing pan. The start is silent for the starting distance / 343 s.
- `haas` {input, [delay 0.012 s, 0..0.05], [side right|left]}: mono to stereo
  by delaying one channel (the other is the input bit for bit).
- `decorrelate` {input, [amount 1], [seed]}: two channels with the same
  spectrum and no zero-lag correlation (all-pass FIRs, 3.4 to ~14 ms delay).

<!-- recipe 4 -->
```json
[
  {"type": "noise", "name": "engine", "color": "brown", "amp": 0.5},
  {"type": "filter", "name": "growl", "input": "engine", "mode": "bp", "freq": 300, "q": 2},
  {"type": "doppler", "name": "pass", "input": "growl", "speed": 25, "closest": 4, "at": 2.0}
]
```

Limits: a point listener in still air at 343 m/s; no reflections, wind or
directivity; `decorrelate` does not keep a mono sum (3 dB down for
uncorrelated channels) and smears a click over ~14 ms.

### Modulators and control (`docs/nodes/modulators.md`)

- `chaos` {[model logistic|lorenz|henon], rate, [param], [range [lo, hi]],
  [seed]}: a chaotic control signal in -1..1 or `range`.
- `fbm` {rate, [octaves 4, 1..12], [gain 0.5], [range], [seed]}: layered smooth
  random motion (like `wander` in octaves).
- `curve` {input, points [[x, y], ...], [curve linear|smooth]}: lookup table,
  clamped at the ends.
- `switch` {inputs [2..64 sounds], select (signal), [fade 0.005 s], [blend
  false]}: picks an input by `select` (rounded), linear crossfade.
- `pitch_follow` {input (mono), [min 50], [max 2000], [smooth 0.02]} and
  `centroid_follow` {input, [window 0.02]}: audio to a Hz control signal; both
  look ahead (not causal) and hold the last confident value.

<!-- recipe -->
```json
[
  {"type": "fbm", "name": "drift", "rate": 1.5, "octaves": 4, "range": [-1, 1], "seed": 5},
  {"type": "chaos", "name": "wobble", "model": "lorenz", "rate": 6, "range": [0, 1], "seed": 2},
  {"type": "curve", "name": "pitch", "input": "wobble", "points": [[0, 180], [0.5, 240], [1, 520]], "curve": "smooth"},
  {"type": "math", "name": "bend", "op": "mul", "a": "drift", "b": 30},
  {"type": "math", "name": "hz", "op": "add", "a": "pitch", "b": "bend"},
  {"type": "osc", "name": "voice", "shape": "saw", "freq": "hz", "amp": 0.4}
]
```

Limits: Lorenz `rho` under ~24.7 settles to a constant; `fbm` output sits well
inside its range ends (a 20 s run used 123..270 of 100..300); `switch`'s
crossfade is linear, so uncorrelated inputs dip in level; `pitch_follow` is
monophonic and can make octave errors on a weak fundamental.

### Events to control (`docs/nodes/sequence.md`)

Event streams (`pattern poisson bounce burst once on_threshold shatter`) used
to feed only `grains` and `voices`. These make control signals from them. An
event at time t first affects sample ceil(t x sr).

- `trigger_env` {events, [attack 0.002], [decay 0.2], [curve exp|linear|smooth],
  [scale_by_value true], [retrigger restart|add]}: an envelope per event;
  `exp` is a 60 dB fall that reaches exactly 0 at `attack + decay`.
- `event_gate` {events, [length 0.05]}: 1 for `length` seconds after each event.
- `seq` {events, values [..], [mode cycle|random|once], [glide 0 s], [seed]}:
  steps through `values` on each event and holds.
- `notes` {notes [[pitch, beats], ...] (a name `C4`, `Bb3`, semitones from
  `root`, or `"rest"`), [bpm 120], [root 440], [glide], [gate 0.9]}: ports
  `out` (Hz, held), `gate` (0/1) and `events` (one per pitched note, value =
  Hz), read as `"tune"`, `"tune.gate"`, `"tune.events"`.

<!-- recipe 4 -->
```json
[
  {"type": "notes", "name": "tune", "bpm": 150, "glide": 0.03, "gate": 0.8,
   "notes": [["A3", 1], ["C4", 1], ["E4", 1], ["rest", 1], ["A4", 2]]},
  {"type": "osc", "name": "voice", "freq": "tune", "shape": "saw", "amp": 0.4},
  {"type": "trigger_env", "name": "pluck", "events": "tune.events", "attack": 0.003, "decay": 0.25, "scale_by_value": false},
  {"type": "math", "name": "line", "op": "mul", "a": "voice", "b": "pluck"}
]
```

Limits: `notes` plays once (no loop field; use a `pattern` and `sound`
instances); `seq` `glide` is linear in Hz, `notes` glide is linear in
semitones; `random` can repeat a value; `trigger_env` `restart` with `attack`
0 steps to the new peak.

### Lo-fi and loops (`docs/nodes/lofi.md`)

- `crossfade_loop` {input, [fade 0.25 s]}: makes the wrap of a loop
  continuous (equal-power crossfade of the end into the start; same length,
  the middle is untouched). Put it last in a loop's chain. Not causal.
- `resample` {input, rate (Hz, signal), [antialias false]}: sample-rate
  reduction with hold.
- `tape` {input, [wow 0.2 %], [flutter 0.1 %], [drive 1], [hiss_db -60], [tone
  8000 Hz], [seed]}: speed wobble, soft saturation, hiss, tone roll-off. The
  output is delayed by ~0.5 ms to 40 ms (the wobble's mean delay).
- `stutter` {input, events, [slice 0.05 s], [repeats 4], [reverse false]}: the
  last slice repeats at each event (`reverse` is not causal).

<!-- recipe 2 -->
```json
[
  {"type": "noise", "name": "air", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 0.3, "range": [300, 2500], "seed": 2},
  {"type": "filter", "name": "band", "input": "air", "mode": "bp", "freq": "sweep", "q": 3},
  {"type": "crossfade_loop", "name": "seamless", "input": "band", "fade": 0.4}
]
```

as a `kind: loop` with `duration: 2`. Measured: `crossfade_loop` passes the
wrap-jump half every time; the spectral half failed on 2 to 5 of 24 loops whose
filter centre moves once a second or faster and on 12 of 24 at three a second.
It changes the first and last `fade` seconds (a blend of material from
elsewhere), can move DC (follow brown noise with a high-pass), and a steady
drone can swell 3 dB mid-ramp.

### Physical models and finished recipes

Models take a few physical fields and run tuned DSP inside. Most are seeded
(`seed` mixed with the document seed and node name), bit-identical at any
thread count, and **not normalized**: set `output.lufs` or `output.peak_db`.

**Blasts** (`docs/nodes/blast.md`): offline one-shots, mono, exactly the
document's length (like `thunder`).

- `explosion` {size (fireball radius m, 0.1..100), distance (m, 0..30000),
  [debris] (an event node: a tick per event, value = level), [seed]}. Since
  book check 2 it carries Farnell's fireball rumble (`explosion-factory1.pd`):
  noise through a band-pass and two one-pole low-passes, all at 40 + 100 x a
  line that rises in 100 ms and falls over 8 s x (size / 2)^0.35, level the
  line^4 smoothed at 5 Hz. The listener preferred the book's long rumble but
  not its crackly opening, so the opening is unchanged; set a
  `duration` that fits.
- `gunshot` {weapon pistol|revolver|smg|carbine|rifle|sniper|shotgun|lmg or
  caliber (mm, 2..60), [barrel 0.4 m], [suppressed false], [punch 0.5],
  [drive 0.7], [action none|semi], [space none|open|forest|urban|canyon|room|
  hall|hangar], [space_size 1], [tail_db 0], [layer all|close|crack|mech|tail],
  [events], [distance 0], [seed]}: Farnell's fig. 53.9 open shot (a cartridge
  chirp, a barrel wave and a noise burst through an 8-band weapon body) plus
  an atonal low punch (50-250 Hz), soft saturation (`drive`: recorded shots
  have a 15-18 dB crest), the slide's clacks, an N-wave crack for a supersonic
  bullet (it leads the blast by the travel-time difference, 135 ms at 100 m),
  and a report tail built for the space (a dense, darkening field with
  slapbacks; not `camera`, whose discrete slaps read as "a digital glitch" on
  a shot). `events` fires one varied round per event (automatic fire). A
  ten-shot burst 80 ms apart of the plain Farnell shot was heard as "perfectly
  like an smg". For a game keep `space: none` (or export `layer: close` and
  `layer: tail` apart); for a video set the space. With `space: open` a pistol,
  rifle and shotgun sit 7-9 dB from the nearest of four recordings (dry: 15-20);
  not heard yet (`docs/nodes/blast.md`).
- Both start 2.5 ms in (so the default fade-in does not cut them), delay by
  `distance / 343` s, darken and quieten with distance, and end in `tanh`, so
  every size peaks near 0.8: size and caliber move the sub band and the length,
  not the level. A document shorter than the delay renders silence.

**Elements** (`docs/nodes/elements.md`): `fire`, `wind`, `spark`, all with `loop`
(bool; unset follows the document, so a `kind: loop` document renders
periodic in its duration; `true` forces it, `false` forces a plain render).

- `fire` {size (m, 0.02..10), [intensity 0.5, signal], [crackle 0.5], [seed],
  [loop]}: roar, hiss and Poisson crackle pops under one control.
- `wind` {speed (m/s, signal, 0..60), [gust 0.3], [whistle 0], [diameter
  0.004 m], [seed], [loop]}: five moving resonant bands plus an optional
  Aeolian tone. Speed 0 is exact silence.
- `spark` {rate (snaps/s, 0.1..200), [buzz 50|60 Hz], [body 3000 Hz], [seed],
  [loop]}: adapted from Farnell's faulty transformer (Designing Sound ch. 39):
  a hum of two saws at twice the mains +-0.2 Hz (beating), clipped to -0.9..0.1
  under a slow random level; arcing chirps under a random ~3 Hz gate, only
  while the hum is loud; Poisson snaps (noise with a 0.5 s quartic fall plus a
  7 kHz -> 20 Hz sweep) through a formant bank (4.6, 7.2, 0.48, 0.72 kHz,
  then 2.5 kHz; `body` / 3000 scales it); all in a 20 ms comb. Rated "SO much
  better" than the old model; a closer port of humandsparks.pd was rated
  worse (see "Porting Farnell's Designing Sound patches").
- Not normalized (a default `fire` is about -30 LUFS, peak -8 dBFS; a pop-heavy
  one is peaky: crest 23.5 dB). Memory: fire holds six buffers of the
  document length.

**Water** (`docs/nodes/water.md`):

- `water` {flow (signal 0..1; 0 is silence), [depth 0.5], [splash 0], [density
  300 bubbles/s, 0..3000], [tone 0.5, 0..1], [loop], [seed]}: a population of
  Minnaert bubbles (rate density x flow^1.5, median radius 0.8 mm x 5^flow)
  plus hiss. Capped at 800000 bubbles a render (an error that says so).
  `tone` is a high shelf at 2 kHz of (tone - 0.5) x 24 dB over everything the
  node makes (0.5 = off, bit-identical); it is primed from the periodic
  buffer in a loop, so put the brightness knob here and not in a `filter`
  after the node (which breaks the seam). The level is not re-normalised.
- `splash` {size (m, 0.005..1), [tail 1, 0..3], [burst 1, 0..2], [impact
  "plunge" | "noise"], [at 0], [seed]}: the default `impact: "plunge"` opens
  with four large cavity bubbles and a pour of moving sines after Farnell's
  `pour1.pd`, a soft hiss that fades in and no noise burst, then droplets
  falling back from `0.16 s * k` on (short plips: a noise tick and a rising
  bubble at 1.8-5 kHz; a first version with Farnell's longer sine drops was
  heard as "digital tones"); `tail` scales the droplets as well as the bubble cloud. It rises to its
  peak in about 19 ms. With these droplets the listener rated sizes 0.05 and 0.2 good
  ("splash with bubbles"), dry and in a room, and `tail` near 0 good ("deep
  bubble pop"); size 0.6 was rated bad, dry and in a room, with no note on
  why.
  `impact: "noise"` is the earlier opening, which the listener heard as "a
  snare, then bubbles" and rated bad; `water`'s own `splash` field still uses
  it. The noise opening: Farnell's raindrop and bubble recipes (Designing Sound ch. 35, 38),
  fitted to `refs/splash_set`: an impact bump (a 5 ms x k raised-cosine
  pressure pulse, k = sqrt(size / 0.1)), a slap (noise, 60 ms x k, band-pass
  3.5 kHz) and spray (the same noise, 350 ms x k x tail, band-pass 3.2 kHz),
  then 140 x k x tail bubbles over 0.5 s x k x tail, dense early, each a sine
  whose pitch RISES 1.7x over its life (bigger: lower, longer, louder; 350 Hz
  to 3 kHz). Starts 2.5 ms in (the default fade-in does not cut it); scaled to
  a **peak of 0.5 whatever the size**. A peaky one-shot (crest about 32 dB):
  use `peak_db`. `tail` scales the bubble cloud and the spray (0 = neither);
  `burst` the impact against the bubbles (0 = bubbles only). Measured at
  size 0.2: all eight `reference_set` features inside the set on 3 seeds,
  11.7-12.2 dB from the nearest recording (the set's own spread 12.5). The
  centroid falls from the slap (4-5 kHz) to the bubbles (about 1 kHz); the
  A/B listener preferred this to the fixed-band spray model ("diminishing").
- Measured: `flow` 0.1 / 0.35 / 0.8 at density 60 gave 1.9 / 9.4 / 17.4 onsets
  a second and centroids 3102 / 2607 / 1971 Hz; no energy under 165 Hz (mix in
  a rumble for a river).

**Machines** (`docs/nodes/machines.md`): `rpm` is a signal, so a `ramp` or
`wander` revs them; set a `duration` (the ramps hold their last value).

- `engine` {rpm (0..20000), [cylinders 4, 1..16], [load 0.5, signal], [stroke
  4|2], [exhaust_length 1.2 m], [seed]}: Farnell's car engine (Designing
  Sound ch. 45), preferred in an A/B (book check 2): one 1 / (1 + k x^2)
  pulse per cylinder on the crank phase, each with its own timing and level
  jitter (uneven firing), transmission overtones at 1, 2 and 4 x the firing
  rate, combustion noise riding the pulses (more with `load`), then a
  warping exhaust: two feedback delays at 0.87 and 1.33 x 2 L / 343 s whose
  times the engine signal modulates, high-passed at 45 Hz, saturated (drive
  with load), low-passed at 2-6 kHz by load. Pitch is the firing rate rpm /
  60 x cylinders / (stroke / 2) Hz. Measured on a 1200 -> 4800 rpm rev: all
  eight `refs/engine_set` features inside, 9.7-10.4 dB from the nearest
  recording (the set's own spread 10.2).
- `motor` {rpm, [poles 4, even], [gear_teeth 0..400], [load 0.3], [whine 0.3],
  [carrier 4000 Hz], [seed]}: Farnell's motor (Designing Sound ch. 44),
  preferred in an A/B: rotor clicks (brush noise band-passed at 4 kHz plus
  DC, shaped by the quartic decay of each turn), the stator pulse
  `1 / (cos^2(2 pi 2 p) + 1) - 0.5` on the electrical phase (its strongest
  line is 4x the pole-pair rate), and the housing as Farnell's FM body
  (`cos~(0.5 osc~ 178 + drive)`, high-passed at 180 Hz); plus a gear-mesh tone
  at rpm / 60 x teeth and the PWM whine. Scaled by rpm / (rpm + 500), so a
  spin-up fades in.
- `clock` {[rate 8 beats/s], [size 1], [escapement 1], [distance 0 m], [at], [seed]}:
  Farnell's clock (`clock-all1.pd`): per beat a 3 ms step burst; the tick
  (four clicks, 6543 / 3245 / 1356 Hz at Q 30) on beats 1 and 5 of 8, the tock
  (five clicks, 7543 / 3988 / 2765 Hz) on 3 and 7, eight 10 / 8 kHz
  escapement pings a beat, the hand (300 / 3245 / 5356 Hz) on beat 1, and the
  case (two delay loops at 900 and 1100 Hz / size, Q 3). Heard as "very close
  up to a clock or car blinker", rated good, and as "a mechanical thing":
  a real clock heard across a room is `distance` about 3 with `rate` 4 (a
  tick each second, faint tocks between; the escapement and the hand fade
  out, the whole is low-passed at 12 kHz / (1 + d / 3)).
- `fan` {speed (signal 0..1), [rate 30], [air 1500 Hz], [duct 0.5], [seed]}:
  adapted from Farnell's fan (ch. 46): blade pulses `1 / (1 + (4 cos)^2)` at
  2 x rate x speed, air noise band-passed at `air` and chopped by the
  blades, a small motor hum, a duct (9 and 23 ms echoes). 24 passes a second
  read as a big propeller, so the default is 30 (60 a second). A closer port
  of fan5.pd was rated worse.

**Waveguides** (`docs/nodes/waveguide.md`): causal, mono.

- `tube` {length (m, 0.029..20), [excite reed|lip|jet|noise], pressure
  (signal 0..1), [bore 0.01 m], [open_end true], [seed]}: pitch c / (2L) open,
  c / (4L) closed at one end (odd harmonics).
- `string` {freq (20..sr/16 Hz, signal), [excite pluck|strike|bow], [position
  0.15], [damping 0.3], [stiffness 0], [bow_pressure 0.5, signal], [seed]}.
- Limits: the bow locks onto a higher harmonic at some pitch / position pairs
  and low pressures (`listen` before trusting one); the pluck and strike are
  fixed 2 ms excitations; `tube` has no toneholes or bell (pitch error up to 1.8 %).

**Surfaces** (`docs/nodes/surface.md`):

- `rolling` {speed (m/s signal, 0..50), [roughness 0.5], [size 0.002 m], [seed]}:
  the force of rolling over a rough surface, an excitation for `modal`; level is
  proportional to speed (up to +34 dB at 50 m/s): scale it before a `modal`.
- `cloth` {motion (signal 0..4; 0 is silence), [weight 0.5], [seed]}: rustle
  bursts with power-law sizes in four bands (cloth, paper, foliage).
- `footstep` {surface concrete|gravel|grass|wood, [weight 0.5], [pace walk|run|
  sneak or 0..1], [shoe hard|boot|sneaker|bare], [body 1], [swish 0.5], [at 0],
  [seed]}: one heel and one toe (40-90 ms apart), a finished sound; a default
  step peaks near -12 dBFS. Build a walk with several nodes or `sound`
  instances. `body` (0.5..2) multiplies the resonant body frequency apart from
  `weight` (wood: all three modes; concrete: the low thud; gravel: the thump;
  grass: the low band; 0.5 an octave down). `swish` (0..1) trades the swish
  for the ticks: grass swish bands x `1 + 2 (swish - 0.5)` against the stalk
  ticks x `1 - 2 (swish - 0.5)`; gravel stone ticks fewer and a swish bed
  fading in above 0.5; concrete and wood ignore it. Both defaults are
  bit-identical to before the fields (pinned hashes); give the human macros on
  them. Measured (`docs/nodes/surface.md`): grass energy above 2 kHz at swish 0 /
  .5 / 1 is 0.10 / 0.56 / 0.77; the wood low mode at weight 0.5 goes 78 Hz to
  305 Hz from `body` 0.5 to 2.

- `buckle` {[load 0.5 signal] or [rate signal], [size 1 m], [thickness 0.002 m],
  [material steel|aluminium|copper|titanium], [exponent 2], [ring 0.5], [seed]}
  (`docs/nodes/sheet_metal.md`): sheet metal snapping through under load.
  Ports `out`, `force` (for a body), `events` (value = strength, $size = facet
  m). Measured at load 0.6: foil 0.2 m 120 events/s, centroid 5.6 kHz; steel
  1 m 1 mm 15/s, 283 Hz; 3 mm 5.8/s, 190 Hz. Not heard.
- `tear` {[speed 10 m/s signal], [thickness 0.001 m], [material aluminium],
  [grain], [jitter 1], [pitch_m], [pop_db 6], [hiss 0.5], [rumble 0.3], [seed]}
  (`docs/nodes/sheet_metal.md`): ductile metal tearing. Ports `out`, `force`,
  `events` (ligament snaps, rivet pops). Measured: centroid 2.8 / 3.2 / 3.5 kHz
  and 3.3 / 2.5 / 2.4 % under 200 Hz at 5 / 10 / 20 m/s, voiced 0; `pitch_m`
  0.025 at 10 m/s is voiced at 400 Hz. Not heard.
- `crumple` {density (signal, bursts/s, 0..400; 0 is silence), [size 0.5],
  [hardness 0.5], [seed]} (`docs/nodes/crumple.md`): paper, foil, plastic or
  leaves as a population of short band-passed noise bursts plus sparse louder
  snaps, arrivals on a jittered grid so the count follows `density` all the way
  (`cloth` saturates: its fine onset count stops rising from `motion` 1).
  `size` lowers the burst centres and lengthens the decays, `hardness` raises
  the centres (1.5-4.5 kHz soft, 3-9 kHz hard) and shortens them. Measured:
  fine onsets per second 20 / 40 / 80 / ~155 at density 20 / 40 / 80 / 160;
  centroid 4.5 kHz at hardness 0 to 7.2 kHz at 1; 3.9 kHz at size 1 to 8.1 kHz
  at 0. Above density 40 each burst shortens so the energy stays level. Not
  normalized and peaky when sparse: `output.lufs` -20 is met at density 40,
  `peak_db` for a few bursts a second. The fine onset counter reads low above
  about 200 per second (checked to 200). Not heard: macros on `hardness`
  (claim `centroid`, 1) and `size` (claim `centroid`, -1) work.

<!-- recipe -->
```json
[
  {"type": "wander", "name": "squeeze", "rate": 3, "range": [30, 140], "seed": 2},
  {"type": "crumple", "name": "foil", "density": "squeeze", "size": 0.4, "hardness": 0.85, "seed": 3},
  {"type": "output", "name": "out", "input": "foil", "lufs": -20}
]
```

**Mechanism clicks** (`docs/nodes/click.md`):

- `click` {kind latch|switch|button|lock|clasp, [size 0.5], [hardness 0.7],
  [at 0], [double true], [seed]}: one or two transients (a press, then a
  release or second snick 24-72 ms later by kind: latch 45, switch 24, button
  70, lock 72, clasp 35 ms) through three or four small body resonances at
  1.5-8 kHz. `size` 0..1 scales the modes by 4^(0.5 - size) (bigger is lower and
  longer; a `switch` centroid ran 4.7 kHz at 0.1 to 2.1 kHz at 0.9),
  `hardness` 0..1 is the contact sharpness; the seed moves modes and gap by
  +-3 %. The press peaks at 0.5 (-6 dBFS) whatever the settings: scale it with
  a `gain`, or `peak_db` on the output. **A click at `at: 0` starts at sample
  0 and the default fade-in cuts it** (peak 0.50 became 0.30): give the
  `output` `fade_in: 0.0005`, or `at: 0.005`.
- Recipe: a door latch is `click` `kind: "latch"` (size 0.6, hardness 0.8)
  after the thud of a `modal` panel; a light switch is `kind: "switch"` size
  0.35 hardness 0.85. Macros on `size` (claim `centroid`, -1) and `hardness`
  (claim `centroid`, 1) work. The descriptors' onset table merges the 24 ms
  `switch` double: count it with `expect onsets {spacing_ms: 5}`.

<!-- recipe -->
```json
[
  {"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.0},
  {"type": "gain", "name": "hit", "input": "latch", "gain_db": -2},
  {"type": "output", "name": "out", "input": "hit", "fade_in": 0.0005, "peak_db": -3}
]
```

**From the book, ported straight from the patch** (`nodes/farnell.rs`,
`docs/nodes/farnell.md`): each was built from its `.pd` file and then
compared with that patch rendered by real Pd (2008-era compatibility,
Pd-extended `pow~`), using exact copies of Pd's `bp~`, pre-0.44 `hip~`,
`lop~` and `vcf~`. STFT distance to the Pd render: `cricket` 1.9, `bell` 3.6,
`twang` 4.8, `ball_bounce` 6.5 dB. Not yet heard.

- `bell` {[freq 750 Hz, 50..3000], [at 0.05 s or a list], [ring 1, 0.1..10],
  [seed]}: BELL/A4-bell-telephone.pd. Partials at 0.5001 / 1 (720 ms), 2.002 /
  3 / 9.6 (540 ms), 2.5 / 2.571 / 11 (300 ms, x^4), 3 / 6.242 / 12.483 (86
  ms, x^2) x `freq`, a strike (16 and 24 x `freq` plus noise, 67 ms, x^4),
  the strike through a 59 ms body loop (band-passes 287 / 431 / 1243 Hz, Q
  12, clipped -0.2..0.5), the sum clipped at +-0.9, an old `hip~` at 10 Hz.
  Every strike in `at` restarts all envelopes.
- `ball_bounce` {[length 3 s], [gap 0.3 s], [pitch 80 Hz], [at 0.01]}:
  BOUNCINGBALL/bb1.pd. A line L falls 1 -> 0 over `length`; a bounce every
  `gap` x L, each a sine of `pitch` + 130 L v^e Hz (v falls over 2/3 of the
  gap, after a 1 ms rise) wobbled +-70 L Hz at 120 Hz, level 0.2 L v^2.
- `cricket` {[tick 0.017 s], [pulses 7], [period 41], [pitch 4500 Hz], [ring 0.3, 0.05..1]}:
  INSECTS/cricket2.pd. Ticks every `tick`; the first `pulses` of each
  `period` fire a 0.1 ms pulse of height (k + 2) / 9, `lop~` 1 kHz, into
  `bp~` 4500 / 4550 / 4600 Hz Q 1200 (x 100) and 9000 Hz Q 500 (x 42), all x
  `pitch` / 4500, old `hip~` 4 kHz, x 0.5. `ring` scales both Qs (band
  levels kept): at 1 each pulse rings ~85 ms, longer than the gap, so the
  close bands blur into a tone beating at 50 Hz, which the listener heard as
  metallic. Shorter rings make the ~59 pulses a second stand out instead
  (roughness 17.7 / 36.7 / 51.5 % at 1 / 0.3 / 0.12). The listener picked
  0.3 (the default); 0.12 read as more like a cicada.
- `twang` {[freq 426 Hz, 20..800], [vibrato 6 Hz], [at 0.01 s], [ring 1],
  [seed]}: MRBOINGY/twang.pd. A falling phasor at `freq` / 4 + 6 e^15 (e
  the 7 s envelope ^4) drives clamped modes cos(m p), m 1 / 6.267 / 17.55 /
  34.39 (gains 1/2 .. 1/16) under a 1.5 s envelope ^4; times p^2 and noise,
  `lop~` 4 kHz, it excites six `vcf~` modes (Q 80) at 1 / 2.7565 / 5.404 /
  8.933 / 13.34 / 18.64 x (`freq` + `vibrato` at 5 Hz), plus 0.2 x the
  clamped modes, old `hip~` 90 Hz, x 4. Its strongest line is the phasor at
  `freq` / 4.

Round 2 (STFT distance to the Pd render: `red_alert` 4.2, `siren` 6.1, `jet`
6.3, `door_creak` 8.0 dB; not yet heard). Feedback through `s~` / `r~` in a
patch adds one 64-sample block to the loop, and the ports include it.

- `siren` {[rate 3], [low 300], [high 1100], [echo 0]}: POLICE/police3.pd.
  `logosc~` is `1 - (1 - 2p)^e` then `(2 - 2p)^(e/2)` (Pd-extended pow~); one
  at `rate` sweeps another between `low` and `high`, clipped +-0.2 into `bp~`
  1500 Hz Q 4. `echo` 1 is environment.pd (33 / 121 / 165 ms cross-fed
  echoes); the default 0 is dry. Its output carries DC (the `dc` check reads
  about -23 dB): follow it with an `onepole` hp at 20 Hz.
- `door_creak` {force (signal), [seed]}: CREAKING/doorcreaker.pd. Over 0.3, a
  tick every `3 + 60 (1 - f)` ms + up to `6 (1 - f)` ms; height `sqrt(sqrt(e)
  + 0.1)` and fall `sqrt(e)` ms (e = ms since the last / 100), squared;
  wood `bp~` 62.5 / 125 / 250 / 395 / 560 / 790 Hz plus 0.2 dry; eight panel
  delays 4.5-16 ms (feedback 0.05); old `hip~` 125, x 0.2. Density 239 / 588
  events a second at force 0.4 / 0.9.
- `jet` {speed (signal 0.1..1), [seed]}: JETENGINE/jetengine1.pd. `lop~` 0.2
  Hz on speed; turbine sines 3097 / 4495 / 5588 / 7471 / 11000 x s (gains
  0.25 / 0.25 / 1 / 0.4 / 0.4, clip 0.9); burner noise `bp~` 8 kHz Q 0.5 ->
  `vcf~` 150 s^2 Hz Q 1 -> old `hip~` 120 -> x 120 clip -> `vcf~` 12000 s Hz Q
  0.6; burner + 0.5 turbine, `lop~` 11 kHz, x 0.1. Centroid 744 / 2507 Hz at
  speed 0.3 / 0.9.
- `red_alert` {[at 0.01 or a list], [pitch 1]}: REDALERT/TOS_redalert.pd. A
  phasor at `360 + 487 sqrt(line)` Hz (900 ms rise), shaped `p - 0.5 + 0.3
  cos(4 pi p)`, gated 890 ms, into five feedback delays (15 / 35 / 11 / 22 /
  24 ms, x 0.6) summed through old `hip~` 12 and fed back through a 61 ms one
  (x 0.006), clipped at x 4, formants `bp~` 740 / 1400 / 1500 / 1600 Hz Q 12.

**Modal shapes, `shatter`, `collapse` and `debris`** (`docs/nodes/modal.md`, `docs/nodes/collapse.md`):

- `modal` `shape` `plate` (default) | `bar` (free-free: 1 : 2.757 : 5.404) |
  `tube` (ring modes of a pipe wall; `size` = diameter) | `membrane` (Bessel
  zeros; `strike_position` 0 is the centre) | `string` (sin(n pi x) / n; takes
  `stiffness` 0..1, B in f_n = n f_1 sqrt(1 + B n^2); an error for other
  shapes) | `shell` (the idealized bell series). `modes` 1..256: the lowest
  half are always the lowest modes of the object (so the fundamental falls with
  `size`: wood plate 424 / 166 / 88 / 54 / 42 Hz at 0.5 / 0.8 / 1.1 / 1.4 / 1.6
  m). Output is normalized (unit impulse peaks near 0 dBFS). A `size` outside the
  usable range (lowest mode 24 Hz .. 90 % of the top of the band) errors with
  the range for that material and shape. `loss` and `tilt` work on every shape
  (see "Physical models: modal, contact, friction").
- `shatter` {pieces (1..200000), [spread 0.4 s], [at 0], [exponent 2, 0.5..6],
  [seed]}: an event stream, a crack at `at` (value 1) then fragments whose
  sizes follow a power law (value 0.01..1) and whose times crowd after the
  break. Feed `voices` (a small `modal` per fragment) or `grains`.
  The first event is the break itself, not a fragment: silence it in the
  `voices` map (`"lvl": "$index < 1 ? 0 : ..."`), or the whole pane's tone
  rings on after it has broken (a listener heard exactly that). The pane's
  own ring belongs before the break: strike a glass `modal` at the impact and
  choke it with an `env` to 0 over ~12 ms where `shatter` starts (60 ms on). Shards whose
  pitch is all above 2 kHz read as one clink: keep the big pieces near
  0.6-1.5 kHz and give `modal` a negative `tilt`.
- Limits: `size` is a pitch knob; `bar` / `tube` use the material's
  `thickness` (metal is a 5 cm plate: pass `{young, density, poisson, loss,
  thickness}` for a thin pipe wall); sizes and times in `shatter` are drawn
  independently (a big piece is as likely late as early).
- `collapse` {[rows 10], [cols 10], [brick 0.075], [mode topple | crumble],
  [lean], [looseness 0.3], [restitution 0.25], [bounces 2], [pile 0.4], [hero
  0], [at], [seed], ...}: a wall falling, its impacts timed by the fall
  (sqrt(2h / g), bounces, a pile); value = impact speed; ports `ground` `pile`
  `bounce` `hero`. `debris` {events, [material stone], [size | f0 900],
  [banks 6], [spread 0.3], [contact_ms 0.6], [ground], [bed_db], ...}: many
  hits through a few shared `modal` bodies, a contact pulse per hit (faster
  is shorter and brighter); 100 or 10,000 hits cost the same. Heroes: a `rig`
  patch in `voices` on `collapse.hero`. Full: `docs/nodes/collapse.md`.
- `contacts` {file, [columns], [fps], [reference], [min_strength], [kinds],
  [materials], [listener], [at]}: events from a physics log (CSV or JSON:
  time or frame, strength, body, size, material, position, kind); ports
  `impact` `slide` `roll`. Events carry detail that `voices` reads as `$size
  $pan $distance $material $kind $body`; `collapse` fills size and pan
  (`pan_width`, `distance`); `debris` falls off with distance and takes
  `stereo: true`. Full: `docs/nodes/contacts.md`.
- `cascade` {[count 2000], [duration 6], [surge 0.3], [floors 1], [peak], [hero]}:
  a large mass failing over seconds; `debris` `scale` 0..1 lowers the bodies
  and makes the bed a long rumble (`docs/nodes/collapse.md`).

**Breakage** (`docs/nodes/breakage.md`): `fracture_on` {events, [threshold 0.4], [sharpness], [chips 12], [spread], [exponent]} turns hard hits into chip event bursts; `dust` {events, [material masonry | wood | glass], [settle 0.8], [density], [gain_db -20]} is grit following the hits' energy.

**Voice upgrades** (`docs/nodes/voice.md`): every new field is off by default
(the old renders are hash-pinned).

- `glottis` new `roughness` 0..1 (period jitter up to +-4 %, level +-50 %),
  `subharmonic` 0..1 (every second cycle scaled by 1 - x: a line at f0 / 2),
  `fry` 0..1 (strong jitter, alternating long / short periods, pressed): all
  signals, per glottal cycle.
- `tract` new `nasal` 0..1 (a 250 Hz pole and 1 kHz zero), `frication` {freq,
  level} (noise through a band-pass added after the formants; gate it with
  `level`), `growl` 0..1 (AM at ~45 Hz), `seed`.
- Limits: `glottis` still aliases above ~1.5 kHz; the nasal and growl rates are
  fixed; the frication centroid sits above `freq` (2258 Hz for 1500).

<!-- recipe 3 -->
```json
[
  {"type": "ramp", "name": "revs", "from": 1200, "to": 5200, "time": 2.0, "curve": "exp"},
  {"type": "ramp", "name": "throttle", "from": 0.2, "to": 0.9, "time": 2.0, "curve": "linear"},
  {"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 6, "load": "throttle", "exhaust_length": 1.5, "seed": 1},
  {"type": "shatter", "name": "bits", "pieces": 40, "spread": 0.5, "at": 0.02, "seed": 4},
  {"type": "voices", "name": "shards", "events": "bits", "length": 0.3, "max": 50,
   "map": {"size": "0.05 + $value * 0.35"},
   "patch": [
     {"type": "impulse", "name": "tap", "at": [0.0]},
     {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
     {"type": "modal", "name": "piece", "excite": "strike",
      "material": {"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.006, "thickness": 0.003},
      "size": "$size", "modes": 16}]},
  {"type": "gain", "name": "quiet_shards", "input": "shards", "db": -12},
  {"type": "mix", "name": "scene", "inputs": ["eng", "quiet_shards"]}
]
```

More recipes: a wind loop is `wind` `speed` 12 `gust` 0.6 in a `kind: loop`
document with `output.lufs` -26; a campfire is `fire` size 0.7 with an
`onepole` hp at 30 Hz and `lufs` -28; a rifle heard at 120 m is `gunshot`
weapon rifle distance 120 space open with `output.peak_db` -1; a brook is
`water` with `flow` from a `wander` 0.35..0.7 plus a `splash` `at` 1.5; a
struck pipe is a hard `contact` into `modal` `shape: "tube"` metal `size` 0.12;
a creaky nasal vowel is `glottis` `fry` from a `ramp` into `tract` `vowel: "o"`
`nasal` 0.5. Macros worth giving: engine `exhaust_length` and the rpm ramp's
`to`; explosion `size` and `distance`; fire `size` and `intensity`; wind `speed`
and `gust` (claim `swing`); water `depth` and `splash`.

### Spectral additions (`docs/nodes/spectral.md`)

Offline, acausal, exactly the document's length; the old modes and
`pitch_shift` / `time_stretch` without `formant` are bit-identical to before.

- `spectral` new `mode`s: `shift` {hz (signal, +-sr/2)}: single-sideband shift
  of every component (a 200/400/600 Hz tone at +50 Hz is 250/450/650);
  `stretch` {[inharmonicity 0.01, 0..2], [f0 100 Hz]}: partial n of a series
  at f0 lands at n f0 sqrt(1 + b n^2) (stiff-string inharmonicity);
  `smear` {time (s, -2..2)}: an all-pass whose group delay rises linearly to
  Nyquist (a click becomes a chirp; the tail past the document is cut, give a
  `duration`); `hps` {[kernel 17, odd 3..101]}: median-filter split with ports
  `out` (= `harmonic`), `harmonic`, `percussive`, read as `"name.harmonic"`;
  the two sum back to the input. A field for another mode is an error.
  `window` applies to `stretch` and `hps` only.
- `pitch_shift` new `formant: true`: keeps the input's spectral envelope
  (cepstral true envelope, +-30 dB per bin) instead of shifting it; about 44
  extra FFTs a frame.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "saw", "shape": "saw", "freq": 180, "amp": 0.4},
  {"type": "decay", "name": "fall", "time": 0.4},
  {"type": "math", "name": "note", "op": "mul", "a": "saw", "b": "fall"},
  {"type": "spectral", "name": "split", "input": "note", "mode": "hps"},
  {"type": "spectral", "name": "bent", "input": "split.harmonic", "mode": "stretch", "inharmonicity": 0.03, "f0": 180},
  {"type": "mix", "name": "both", "inputs": ["bent", "split.percussive"]}
]
```

Limits: `stretch` moves peaks by whole bins, so noise smears by a bin (8 clean
partials stayed within 0.2 dB); `hps` splits by soft masks, not exactly where a
click and a partial share a bin and frame; `formant` assumes envelope features
wider than ~500 Hz (a pitch above ~400 Hz partly follows the harmonics, not
measured); `shift` leaks slightly (-58 dB) on an abrupt start.

### Bands and vocoder (`docs/nodes/bands.md`)

- `filterbank` {input, bands (1..64 log-spaced from `low` to `high`), [gains
  (one per band, linear), default all 1], [q], [low 80], [high 12000]}.
- `vocoder` {carrier, modulator (mono), [bands 16], [attack 0.01], [release
  0.04], [q], [low 80], [high 12000]}: the modulator's level per band scales
  the same band of the carrier. The output is as quiet as the modulator
  (amplitude 0.1 gives -20 dB): follow with a `gain` or `normalize`.
- `multiband` {input, crossovers [1..7 Hz, ascending], bands: [{[drive 0],
  [threshold_db] (turns the compressor on), [ratio 4], [attack 0.01], [release
  0.1], [gain_db 0]}, ...]}: Linkwitz-Riley 4th-order split, per-band drive,
  compressor and gain, summed flat. Ports `out` and `band0`..`band7`.
  `ratio` / `attack` / `release` without `threshold_db` is an error.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "buzz", "freq": 110, "shape": "saw", "amp": 0.5},
  {"type": "noise", "name": "speech", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 3, "range": [300, 2500]},
  {"type": "filter", "name": "formant", "input": "speech", "mode": "bp", "freq": "sweep", "q": 4},
  {"type": "vocoder", "name": "voice", "carrier": "buzz", "modulator": "formant", "bands": 16, "attack": 0.005, "release": 0.03},
  {"type": "multiband", "name": "thick", "input": "voice", "crossovers": [150, 1000], "bands": [{"threshold_db": -30, "ratio": 4, "gain_db": 3}, {}, {"drive": 2, "gain_db": -3}]}
]
```

Limits: with all gains 1, `filterbank` raises white noise 2.4-3.0 dB (the bands
overlap) and a single band-pass has 6 dB/oct skirts; `vocoder` has no
sibilance path (noisy consonants pass only where the carrier has energy) and a
harmonic carrier leaves bands without a harmonic quiet; `multiband` settings are
numbers (no automation), the compressor has no lookahead, `drive` is not
oversampled; all hold the whole document per band in memory.

### Grains and clouds (`docs/nodes/grains.md`)

- `grains` {source (a node or a `.wav`), events, size, [position 0..1, signal],
  [pitch 1], [window hann|tri|rect], [pan_spread], [jitter], [seed]}: `size`
  and `pitch` each take a number, a signal (read at the grain's start), or a
  range `{min, max, [curve linear|exp|smooth]}` that draws a value per grain.
- `cloud` {source, density (grains/s, signal, 0..sr), size, [position], [jitter],
  [pitch], [pitch_spread 0 semitones, 0..120], [mode random|regular], [window],
  [pan_spread], [seed]}: grains scheduled from a density signal, no event
  stream (Poisson starts in `random`, even in `regular`). At most 200000 grains
  a sound. Not normalized: the level rises with density x size.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "tone", "freq": 330, "amp": 0.5},
  {"type": "ramp", "name": "swell", "from": 4, "to": 160, "time": 2.0, "curve": "exp"},
  {"type": "cloud", "name": "swarm", "source": "tone", "density": "swell",
   "size": {"min": 0.02, "max": 0.08}, "position": 0.1, "pitch_spread": 2, "pan_spread": 0.6}
]
```

Limits: size and pitch signals are read once at a grain's start; a grain that
reaches the end of the source stops there (no wrap); `pitch_spread` is uniform
in semitones, a `grains` `pitch` range with `curve: "linear"` is uniform in
ratio.

## Worked recipes

Moved from AGENTS.md, and still rendered by `tests/it/recipes.rs`.

A sword clash (metal, so here the body **should** ring and be inharmonic:
two `modal` bars a non-integer ratio apart; macro: `ring` -> both bars'
`loss`, exp, 1.5 to 0.25, unclaimed: the thump and crack hold the whole-sound
`decay` at the low end):

<!-- recipe 0.6 -->
```json
[
  {"type": "env", "name": "scr_env", "points": [[0, 0], [0.02, 0], [0.04, 1], [0.25, 0.3], [0.32, 0]], "curve": "smooth"},
  {"type": "friction", "name": "scr_f", "speed": "scr_env", "pressure": "scr_env", "roughness": 0.6, "grain": 1800},
  {"type": "filter", "name": "scr", "input": "scr_f", "mode": "bp", "freq": 4000, "q": 0.8},
  {"type": "gain", "name": "scr_g", "input": "scr", "db": -8},
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "decay", "name": "crk_e", "time": 0.012, "at": 0.005},
  {"type": "math", "name": "crk_raw", "op": "mul", "a": "n", "b": "crk_e"},
  {"type": "filter", "name": "crk", "input": "crk_raw", "mode": "hp", "freq": 2500},
  {"type": "gain", "name": "crk_g", "input": "crk", "db": -6},
  {"type": "decay", "name": "hit_e", "time": 0.004, "at": 0.005},
  {"type": "math", "name": "hit_burst", "op": "mul", "a": "n", "b": "hit_e"},
  {"type": "modal", "name": "blade_a", "excite": "hit_burst", "material": "metal", "shape": "bar", "f0": 1750, "loss": 0.6, "tilt": 3, "gain_db": -4},
  {"type": "modal", "name": "blade_b", "excite": "hit_burst", "material": "metal", "shape": "bar", "f0": 2630, "loss": 0.8, "tilt": 3, "gain_db": -8},
  {"type": "noise", "name": "thd_n", "color": "brown"},
  {"type": "decay", "name": "thd_e", "time": 0.15, "at": 0.005},
  {"type": "math", "name": "thd_raw", "op": "mul", "a": "thd_n", "b": "thd_e"},
  {"type": "filter", "name": "thd", "input": "thd_raw", "mode": "lp", "freq": 250, "slope": 24},
  {"type": "gain", "name": "thd_g", "input": "thd", "db": 4},
  {"type": "mix", "name": "dry", "inputs": ["scr_g", "crk_g", "blade_a", "blade_b", "thd_g"], "to": "mono"},
  {"type": "gain", "name": "trim", "input": "dry", "db": -8},
  {"type": "transient", "name": "punch", "input": "trim", "attack_db": 4},
  {"type": "shape", "name": "warm", "input": "punch", "mode": "tube", "drive": 1.5, "oversample": 4, "mix": 0.2},
  {"type": "onepole", "name": "dc", "input": "warm", "mode": "hp", "freq": 40},
  {"type": "output", "name": "out", "input": "dc", "peak_db": -1.5}
]
```

A UI confirm (tonal on purpose: a tick, two rising notes a fifth apart, a short
low body, and an echo as the after-layer instead of a reverb; give each note a
3 ms `env` attack, since a `decay` starting at `at` clicks and the echo repeats
the click; macro: `brightness` -> both notes' `freq`, exp, claim `centroid` 1):

<!-- recipe 0.8 -->
```json
[
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "decay", "name": "tick_e", "time": 0.004, "at": 0.005},
  {"type": "math", "name": "tick_raw", "op": "mul", "a": "n", "b": "tick_e"},
  {"type": "filter", "name": "tick", "input": "tick_raw", "mode": "hp", "freq": 4000},
  {"type": "gain", "name": "tick_g", "input": "tick", "db": -14},
  {"type": "osc", "name": "note1_o", "shape": "tri", "freq": 880},
  {"type": "env", "name": "note1_env", "points": [[0, 0], [0.005, 0], [0.008, 1], [0.3, 0]]},
  {"type": "math", "name": "note1", "op": "mul", "a": "note1_o", "b": "note1_env"},
  {"type": "osc", "name": "note2_o", "shape": "tri", "freq": 1320},
  {"type": "env", "name": "note2_env", "points": [[0, 0], [0.075, 0], [0.078, 1], [0.45, 0]]},
  {"type": "math", "name": "note2", "op": "mul", "a": "note2_o", "b": "note2_env"},
  {"type": "osc", "name": "body_o", "freq": 220},
  {"type": "decay", "name": "body_e", "time": 0.09, "at": 0.005},
  {"type": "math", "name": "body", "op": "mul", "a": "body_o", "b": "body_e"},
  {"type": "gain", "name": "body_g", "input": "body", "db": -12},
  {"type": "mix", "name": "notes", "inputs": [{"input": "note1", "gain": 0.5}, {"input": "note2", "gain": 0.5}]},
  {"type": "delay", "name": "echo", "input": "notes", "time": 0.11, "feedback": 0.25},
  {"type": "filter", "name": "echo_lp", "input": "echo", "mode": "lp", "freq": 3000},
  {"type": "gain", "name": "echo_g", "input": "echo_lp", "db": -12},
  {"type": "mix", "name": "dry", "inputs": ["tick_g", "notes", "body_g", "echo_g"], "to": "mono"},
  {"type": "output", "name": "out", "input": "dry", "peak_db": -3}
]
```


## More ready-made nodes by brief

Moved from AGENTS.md "Which ready-made node for which sound" to keep that file
agent-sized; AGENTS.md keeps one row pointing here. Same columns.

| Brief | Node | Start with | Fits |
|---|---|---|---|
| a telephone bell, a ringing bell | `bell` | `freq` (750), `at` (strike time or list), `ring` | Farnell's telephone bell: partial groups, a strike and the bell body; 3.6 dB from the patch played by Pd |
| a bouncing ball | `ball_bounce` | `length` (to rest), `gap` (first interval), `pitch` | Farnell's ball: sine thumps that come faster, shorter and quieter |
| a cricket, night insects | `cricket` | `tick`, `pulses`, `period`, `pitch`, `ring` (0.3; lower is more cicada) | Farnell's cricket: chirps of rising pulses ringing narrow bands at 4.5 / 9 kHz |
| a ruler twang, a sprung tongue, "boing" | `twang` | `freq` (overhang pitch), `vibrato`, `ring` | Farnell's twanged ruler: a phasor-driven clamped bar exciting free modes |
| a police siren, an emergency vehicle | `siren` | `rate` (3 = yelp, 0.1 = wail), `low` / `high` Hz, `echo` (the patch's street; 0 dry) | Farnell's siren: a log-shaped oscillator swept through a clipped plastic horn |
| a creaking door, a hinge | `door_creak` | `force` (signal; creaks over 0.3) | Farnell's stick-slip hinge through wood resonances and a square panel |
| a jet engine, a turbine | `jet` | `speed` (signal 0.1..1, spools up through a 0.2 Hz low-pass) | Farnell's jet: turbine tones and a clipped, swept burner roar |
| a sci-fi alarm, red alert | `red_alert` | `at` (whoop times), `pitch` | Farnell's klaxon: a rising buzz through feedback delays and a formant horn |
| a stream, a brook; pouring into a vessel | `stream`; `pour` | `flow`, `depth`, `range`, `layers`, `loop`; `fill_time`, `vessel`, `bubbles` | heard: `stream` ok (bubbles), at `flow` 2 / `layers` 4 a gurgling tube (good); `pour` ok (chemistry-set bubbles); `pour` needs a `duration` of `fill_time` + 1.5 s |
| a helicopter; a rocket motor | `helicopter`; `rocket` | `rotor_rate`, `engine`, `tail`, `distance` (signals); `thrust`, `ignition`, `burn` | heard: helicopter good (near and far); rocket as a fake game weapon; set a `duration` |
| a housefly; cicadas | `fly`; `cicada` | `freq`, `res`, `flutter`; `kind` buzz / rattle / call, `pitch`, `rate` | heard: `fly` as a fly (good); `cicada` as a rattlesnake / scissors (both parts). Both never decay: set a `duration`. (`roar` is the older creature, heard as a cow: use `creature`) |
| waves on a shore, surf | `surf` | `shore` sand / shingle, `wave_height`, `period`, `distance`, `loop` | foley's shore: roar, plunge bubbles, foam hiss, backwash; quiet: set `lufs`. Picked: shingle, dry, over sand and storm |
| lava; a cave | `lava`; `cave` | `size`, `activity`; `depth`, `mass`, `drips`, `openness` | foley's beds: rumble plus ringing bubbles; ceiling drips over a sub rumble (a `cave` loop needs `onepole` hp 20 Hz then `crossfade_loop`). Picked: `lava` (default size); `cave` rebuilt (pool drips in a long echo): the default picked, dry |
| frogs, a pond chorus | `frog` | `size` cm, `rate` calls/s, `chorus` | foley's sac-resonator call, repeated and layered (birdsong by species is `syrinx` `species`). Rebuilt with per-call pitch and rasping pulses: a small frog (3 cm) picked |
| wind through grass, leaves, dry stalks; underwater | `leaves`; `underwater` | `wind` (signal), `foliage`, `gust`; `depth` (signal), `bubbles`, `pressure` | foley's plant brush and submerged bed; `underwater` loops want `bubbles` 4. Picked: `leaves` trees, `underwater` dry |
| a chattering robot, a beam-up shimmer, a sci-fi torpedo, a hand-held beam | `droid`, `transporter`, `torpedo`, `phaser_beam` | `chatter`, `pitch`, `mood`; `chime`, `reverb`; `sweep`, `ring`; `mode` kill / stun; each a `length` | heard: transporter good, beam good (an alert), droid ok (too random), torpedo ok (a space weapon) |
| a telephone: dial, busy, ringback, touch tones, rotary; a two-tone alarm | `phone_tone`; `two_tone` | `kind`, `digits`, `line` 0..1, `tempo`; `low`, `high`, `rate`, `shape` | heard: dial, busy, ringback, touch tones, two_tone good; pulse_dial as distant gunfire |
| thunder, a distant storm | `thunder` | `distance`, `brightness`, `length`, `duration` | foley's N-wave and rumble; `brightness` tilts above 100 Hz, `length` stretches the rumble; a `duration` that fits |

## `grind`

Scrape, grind and roll texture whose level follows a hit-driven activity (settles over `settle`) and/or a speed signal; `rolling` + `modal` + micro-impacts, by material. See `pile.md`.
