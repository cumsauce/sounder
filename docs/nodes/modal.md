# `modal` shapes, the round-0 modal fixes, `shatter`

## `modal` (new field `shape`, new field `stiffness`, round-0 fixes)

A struck-object model (physical model): the `excite` audio rings a bank of
decaying modes computed from a material, a `size` and a `shape`. This task
fixes three round-0 bugs and adds `shape`.

### What was wrong, and what changed

Round 0 (footstep_wood_floor, keys_jingle, knock_wood, metal_pipe_drop):

1. **The fundamental went missing on big plates.** The node kept the 48
   strongest modes, and the strike weight includes a radiation term that
   rises with frequency, so once a plate was big enough to have more than 48
   modes the lowest ones were dropped: wood at 0.8 m gave a lowest mode of 408
   Hz (the table says 166 Hz), at 1.4 m 537 Hz (table 54 Hz). Now the lowest
   half of `modes` (24 of 48) are always the lowest modes of the object, and
   the other half the strongest of the rest. Measured lowest spectral peak of
   a wood plate, `strike_position` 0.3: 424.2 / 165.7 / 87.6 / 54.1 / 41.5 Hz
   at 0.5 / 0.8 / 1.1 / 1.4 / 1.6 m, falling at every step.
2. **The level.** The raw sum of up to 48 unit-gain modes peaked at about +22
   dBFS for a unit impulse. The bank is now scaled so that a unit impulse
   (1.0 at one sample, or a `contact` of area 1: 0.1 kg at 1 m/s) peaks at
   0 dBFS in the first 8192 samples. Measured over 10 materials x 6 shapes x 3
   sizes: every peak within 0.01 dB of 0 dBFS. A stronger strike is louder than
   that, as before (`contact` area scales with mass x speed).
3. **The size hint.** "try a size between about 0.05 and 5 m" ignored the
   material. The error now names the range for that material and shape,
   computed from the same constants the table uses: the lowest mode falls
   from 10 kHz (or half the top of the band) at the small end to 40 Hz at
   the large end, rounded inward to two figures. Wood plate 0.11 to 1.6 m,
   metal plate 0.16 to 2.4 m.

The default (`shape` plate) is the same node as before apart from these fixes:
the mode formulas and the mistuning draws are unchanged, so where a table has
fewer modes than `modes` the frequencies are the same as before, only the gain
differs. Hashes (FNV-1a of the f32 bits of the `modal` node, unit impulse at
sample 0, 1 s, seed 2), before the fixes -> after: wood 0.5 m / 0.3
`c83f8e64f34833e7` -> `51c2616548ef09c9`; metal 0.25 / 0.5 `b1cf948b0feb0de5`
-> `6923f210cb19dba8`; glass 1.4 / 0.3 `25e7e9e77b87ddcc` -> `dffbe5913f6239bb`.
All 120 combinations of 10 materials x 6 sizes x 2 positions changed (the
level changes every sample). After the fixes, `shape: "plate"` and no `shape`
are bit-identical (test `the_default_plate_is_the_plate`, which pins the three
hashes).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `excite` | signal | required | mono or stereo audio | The strike: `contact`, `impulse`, noise burst, `friction`. |
| `material` | word or object | required | `stone wood metal glass ice earth granular plant soft snow`, or `{young, density, poisson, loss, thickness}` | Sets pitch scale (via thickness and wave speed) and decay (via loss). |
| `size` | signal (read once) | required | metres, > 0 | Plate side, bar or string length, tube or membrane diameter, bell size. Pitch goes as 1 / size^2 (membrane and string: 1 / size). |
| `shape` | word | `plate` | `plate` `bar` `tube` `membrane` `string` `shell` | Which mode table. |
| `strike_position` | signal (read once) | `0.3` | 0..1, clamped to 0.02..0.98 | Plate: x (y follows). Bar, string: along the length. Membrane: radius, 0 the centre. Tube: around the wall. Shell: ignored. |
| `modes` | integer | `48` | 1..256 (error outside) | Most modes kept: the lowest half always, the strongest for the rest. |
| `stiffness` | number | `0` | 0..1, `string` only (error for other shapes) | Inharmonicity B in f_n = n f_1 sqrt(1 + B n^2). A piano string is about 0.0004. |
| `seed` | integer | `0` | | The mistuning: 1.5 % for plates, 0.3 % for the other shapes. |

Errors:

- `stiffness: 0.1: only the "string" shape takes it (this is "plate"): drop it or set shape "string"`
- `stiffness: 2: give 0..1`
- `field shape: unknown variant cube, expected one of plate, bar, tube, membrane, string, shell`
- `size 0.000001: no mode of the metal plate falls between 20 Hz and the top of the band: for metal plate try 0.16 to 2.4 m (lowest mode from 10 kHz down to 40 Hz)`

The node is random (the mistuning) and not causal-checked beyond what it was.
Work: at most 256 modes per channel; the Bessel zero table (membrane) is
computed once per process (41 orders up to j = 40), the bar roots and the
impulse-peak search (8192 samples x modes) per render.

### The tables and their sources

Every table is built from its defining equation at run time and unit-tested
against the published ratios (`modal.rs`, test
`the_tables_come_out_of_their_equations`). The citations are chapter-level,
from memory, not re-checked against the books.

| Shape | Modes | Ratios (first four) | Source |
|---|---|---|---|
| `plate` | simply supported square plate, f ~ m^2 + n^2 | unchanged | Leissa, Vibration of Plates; Fletcher & Rossing; foley's formulas |
| `bar` | free-free Euler-Bernoulli bar, roots of cos x cosh x = 1, f = x^2 / 2 pi * (t / sqrt 12) * sqrt(E / rho) / L^2 | 1 : 2.757 : 5.404 : 8.933 | Rayleigh, Theory of Sound; Fletcher & Rossing (bars); Blevins (beams, free-free) |
| `membrane` | ideal circular membrane, f(m,n) = f(0,1) j(m,n) / j(0,1), strike weight J_m(j r / R) | 1 : 1.593 : 2.136 : 2.295 (then 2.653) | Abramowitz & Stegun 9.5 (zeros); Fletcher & Rossing (drums) |
| `string` | f_n = n f_1 sqrt(1 + B n^2), weight sin(n pi x) / n | 1 : 2 : 3 : 4 at B = 0 | Fletcher, JASA 36 (1964) |
| `tube` | in-plane flexure of a thin ring (the wall of a pipe or can, `size` = diameter), n(n^2 - 1) / sqrt(n^2 + 1), each n a cos / sin pair weighted \|cos n th\|, \|sin n th\| | 1 : 2.828 : 5.423 : 8.771 | Den Hartog, Mechanical Vibrations; Blevins (rings); Love |
| `shell` | the idealized church-bell series, hum 0.5, prime 1, tierce 1.2, quint 1.5, nominal 2, 2.5, 3, 4, 5, 6 | 1 : 2 : 2.4 : 3 (from the hum) | Rossing, Science of Percussion Instruments; Fletcher & Rossing (bells) |

Pitch anchors: `plate` and `shell` use the plate's (1,1) constant
`pi t c / sqrt 12 / size^2` (the shell's prime is that frequency, so the hum
is half of it); `bar` and `tube` use their own beam / ring constants (`c` from
Young's modulus and density, `t` the material's thickness); `membrane` and
`string` use the same constant over `size` (not size squared), so at 1 m their
lowest mode equals the plate's.

### Recipe

A struck steel pipe: a hard `contact` into the tube shape, with a macro on
`size` (pitch) and on `strike_position` (which ring modes speak).

```json
[
  {"type": "contact", "name": "hit", "speed": 2, "mass": 0.2, "material": "metal", "hardness": 0.9, "at": 0.01},
  {"type": "modal", "name": "pipe", "excite": "hit", "material": "metal", "shape": "tube", "size": 0.12, "strike_position": 0.1}
]
```

A bar (xylophone-style): `"shape": "bar"` with `"size": 0.3` and a `material`
of `wood`; a drum skin: `"shape": "membrane"`, `"strike_position": 0.5`.

### Measured

Tests in `crates/sfx_analysis/tests/m7.rs` (48 kHz) and `modal.rs` unit tests.
Ratios are from the spectral peaks of 1.5 s of the node's output (a steel-like
material with a tenth of its loss, so every mode of the table rings for the
whole window), nearest peak to f1 x table ratio. The lowest-peak search used
spectral peaks, not `extract_modes`: the extractor misses the fundamental of
dense tables (it returned 1444 Hz for a membrane whose (0,1) is at 480 Hz, and
135 Hz for the 54 Hz wood plate).

| What | Measured | Test |
|---|---|---|
| unit impulse peak, 10 materials x 6 shapes x 3 sizes | within 0.01 dB of 0 dBFS (limit 6 dB) | `a_unit_impulse_peaks_within_6_db_of_full_scale` |
| wood plate lowest peak at 0.5 / 0.8 / 1.1 / 1.4 / 1.6 m | 424.2 / 165.7 / 87.6 / 54.1 / 41.5 Hz | `the_lowest_mode_falls_with_size_down_to_40_hz_in_wood` |
| lowest mode falls strictly with size over the hint range, 10 materials x 6 shapes, step 1.12x | all pass; lowest at the large end 40.7 .. 54.2 Hz | `the_lowest_mode_falls_with_size_for_every_material_and_shape` (unit) |
| bar f1 1040.6 Hz: ratios | 2.753 / 5.405 / 8.927 (table 2.757 / 5.404 / 8.933), worst 0.15 % | `bar_ratios_match_the_free_bar_table` |
| membrane f1 481.2 Hz | 1.599 / 2.138 / 2.292 (table 1.593 / 2.136 / 2.296), worst 0.35 % | `membrane_ratios_match_the_ideal_membrane_table` |
| string f1 478.4 Hz | 2.009 / 3.018 / 4.020, worst 0.59 % | `string_is_harmonic_and_stiffness_stretches_it` |
| string `stiffness` 0.01, f1 481.3 Hz | 2.036 / 3.132 / 4.304 (formula 2.029 / 3.117 / 4.287), worst 0.49 % | same |
| tube f1 519.8 Hz | 2.845 / 5.447 / 8.844 (table 2.828 / 5.423 / 8.771), worst 0.83 % | `tube_ratios_match_the_thin_ring_table` |
| shell f1 (hum) 751.9 Hz | 2.0 : 2.4 : 3.0 within 0.14 % | `shell_ratios_match_the_bell_series` |
| default plate | `shape: "plate"` bit-identical to none; hashes pinned | `the_default_plate_is_the_plate` |
| size hint | metal plate 0.16 to 2.4 m, wood plate 0.11 to 1.6 m; both ends render, 0.4 x the small end errors | `the_size_hint_names_a_range_...` |
| all shapes at sizes 0.05 .. 400 m, 1 and 256 modes, strike 0 and 1 | finite, peak under 6 dB, or an error naming a range; bit-identical at 1 and many threads | `every_shape_is_finite_deterministic_and_bounded_at_the_extremes` |

### Honest limits

- None of this has been heard. These are frequency tables and a level; whether
  a tube sounds like a pipe is for the listener.
- `size` is a pitch knob, not a measurement. The bar and tube pitches use the
  material's `thickness` constant, which for the built-in `metal` is 5 cm (a
  plate's), so a pipe wall needs `{young, density, poisson, loss, thickness}`
  with a real wall thickness.
- The ring table is the inextensional theory: it over-predicts for high n
  (shell-like bending gives way to stretching), and a real pipe also has beam
  modes along its length, which `tube` does not have (use `bar` for those).
- The shell is a fixed bell partial series, the same for bowls and helmets;
  `strike_position` does nothing for it. The tierce 1.2 is the idealized
  minor third; measured bells scatter around it by a few percent.
- Membrane: no air loading, no stiffness, a single struck angle. Plates are
  ideal and simply supported. Strike weights are heuristics (shape function
  times a gentle high-frequency tilt), not radiation efficiencies.
- A size past the large end of the hint still renders (modes under 20 Hz are
  dropped; the table keeps what is left) but its lowest mode is under 40 Hz.
- Two macros that write `size` and `strike_position` are the right knobs;
  neither glides inside a render (both are read once).

## `shatter`

An event generator (events family) for breaking things: one crack at `at`,
then fragments whose sizes follow a power law and whose times crowd right
after the break. Each event's `value` is a fragment size in 0.01 .. 1; feed it
to `voices` (a small `modal` per fragment, the size from `$value`) or `grains`.
`examples/shatter.rs` is a worked example (a glass pane).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `pieces` | integer | required | 1 .. 200000 (error outside) | Events, including the first crack. Events past the end of the document are dropped, like every generator. |
| `spread` | number | `0.4` | seconds, >= 0 | The cascade runs from `at` to `at + spread`. |
| `at` | number | `0` | seconds, >= 0 | Time of the crack (event 0, value 1). |
| `exponent` | number | `2` | 0.5 .. 6 (error outside) | Power-law exponent b: the number of fragments larger than s goes as s^-b (2 is a flat sheet breaking, bigger means fewer big pieces). |
| `seed` | integer | `0` | | Mixed with the document seed and node name. |

Sizes are a Pareto law truncated to 1/100 .. 1 (inverse-CDF draws); times are
a truncated exponential over the spread with rate 5 (density falls to 0.7 %
at the end). Events are sorted, numbered 0..n in time order, and the first is
always the crack at `at` with value 1.

Errors: `pieces: 0: give 1..200000`, `exponent: 9: give 0.5..6`,
`spread: -1: give a number >= 0`.

### Recipe

```json
[
  {"type": "shatter", "name": "pieces", "pieces": 90, "spread": 0.5, "at": 0.02, "seed": 4},
  {"type": "voices", "name": "shards", "events": "pieces", "length": 0.4, "max": 100,
   "map": {"size": "0.05 + $value * 0.35"},
   "patch": [
     {"type": "impulse", "name": "tap", "at": [0.0]},
     {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
     {"type": "modal", "name": "piece", "excite": "strike",
      "material": {"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.006, "thickness": 0.003},
      "size": "$size", "modes": 16}]}
]
```

Give the human macros on `spread` (how fast it all lands) and on `exponent`
(few big pieces or many small ones).

### Measured

Tests in `crates/sfx_analysis/tests/m7.rs`.

| What | Measured | Test |
|---|---|---|
| 5000 pieces: count | 5000 | `shatter_makes_pieces_events_with_a_power_law_of_sizes` |
| log-log slope of N(> size), size 0.02 .. 0.5, exponent 2 / 3 / 1.2 | -1.952 / -2.991 / -1.256 (limit: within 20 % of -b) | same |
| first event | t = `at`, value 1 | `shatter_times_crowd_early_and_start_with_the_crack` |
| share of events in the first / last third of the spread | 0.823 / 0.027 (the truncated exponential gives 0.816 / 0.029) | same |
| `shatter` -> `voices` of glass `modal` | finite, bit-identical at 1 and many threads | `shatter_drives_voices_of_small_plates` |

### Honest limits

- Sizes and times are drawn independently apart from the first crack: a big
  fragment is as likely late as early. Real cascades have large pieces first.
- The power law is truncated at 1/100 of the largest, so the slope is only
  right between about 0.02 and 0.5 (the crack at value 1 and the floor bend
  the ends).
- The sizes are relative: the node says nothing about mass, so a `voices` map
  decides what a size means. Not heard.

## `modal` `loss`, `tilt` and the wider size range

Round 1 (keys_jingle, glass_break, glass_clink, 6 sounds): no way to change
the ring time of a named material, a glass plate that put 98 % of its energy in
one mode, and a size hint that stopped at a 10 kHz lowest mode (3 mm glass was
refused under 0.039 m, 5 cm metal under 0.16 m).

### New fields (all optional; omitted = bit-identical to before)

- `loss` (number, 0.01..100, default 1): multiplier on the material's loss,
  for a named material or `{young, ...}`. `"material": "wood", "loss": 0.5`
  rings twice as long. A mode's T60 is still capped at 20 s (and floored at
  2 ms). Out of range is an error naming the range.
- `tilt` (dB per octave, -12..12, default 0): each kept mode's gain is
  multiplied by `10^(tilt * log2(f / f_lowest) / 20)`, f_lowest the lowest kept
  mode. Positive lifts the upper modes (flattens a table that is all in its
  first mode), negative darkens. It acts after the modes are chosen and before
  the level normalization, so a unit impulse still peaks near 0 dBFS.

### Size range

`size`'s usable range is now where the lowest mode falls between 24 Hz and 90 %
of the top of the band (18 kHz at 48 kHz; was 40 Hz to 10 kHz). The range in
the error comes from the same table (`lowest_mode`) that builds the modes.
Modes above the top of the band (20 kHz or 0.49 x the sample rate) were and are
dropped, never folded. A size that leaves no mode at all is an error that gives
the lowest mode it would have, the thickness that would fix it (the lowest mode
of every shape goes as the material's thickness: pass `{young, density,
poisson, loss, thickness}`) and the range. The lowest mode of a shape sits at
`a / size^p` Hz (p = 2 for plate, bar, tube, shell; 1 for membrane, string), `a`
growing with the material's thickness.

### Measured (tests in `crates/sfx_analysis/tests/m7.rs`)

- `loss` 0.5 / 2 on a one-mode metal plate: 20 dB decay time 0.380 s /
  0.100 s against 0.195 s (ratios 1.95 and 0.51); a `{young, ...}` material at
  `loss` 0.5: ratio 1.97.
- `tilt` +-6 and 3 dB/oct, string (mode ratios 2 and 4) and bar (5.404 and
  8.933): the level of the upper mode over the lowest one, relative to the
  untilted table, matches `tilt * log2(ratio)` within 0.02 dB.
- glass plate 0.15 m, `strike_position` 0.31, 24 modes, the glass_clink case:
  the lowest mode holds 98.6 % of the energy; at `tilt` 3 / 6 / 9 / 12 it holds
  96 / 91 / 79 / 57 %.
- ranges: 3 mm glass plate 0.029 to 0.78 m (was 0.039 up), metal plate from
  0.12 m (was 0.16). 3 mm glass renders at 0.03 m (lowest peak 16.4 kHz) and
  0.028 m (18.8 kHz).
- A 3 cm glass shard needs a thin plate to have several modes in band: with a
  0.3 mm plate the peaks between 1.5 and 8 kHz number 3 or more. A named glass
  (1 cm) or a 3 mm plate at 0.03 m has one mode under 20 kHz.
- 8 sizes across the range x 4 materials x 6 shapes x 4 sample rates (22.05,
  44.1, 48, 96 kHz): all finite, peak within 6 dB of 0 dBFS, no spectral peak
  above the top of the band.

Honest limits: `size` is still a pitch knob; the thickness suggestion in the
error is the plate's scaling and is rough for the shapes whose table is not a
plate. `tilt` is applied to the modes the table keeps, not to the candidates
before the strongest are chosen.

## `modal` thickness

### `modal` (new field `thickness`)

`thickness` overrides the plate, bar, tube, membrane or string thickness of the material, so small thin objects need no custom material.

#### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `thickness` | signal (read once) | the material's own | metres, 0.0001 .. 0.5 | every mode frequency goes linearly with it; the size range in the error and hint uses it; outside the range is an error naming the range |

Giving a custom material's own thickness, or the named material's (wood 0.025, glass 0.01, metal 0.05), changes nothing: renders are bit-identical to leaving the field out, and the pinned hashes in `tests/m7.rs` hold.

#### Recipe

```json
[
  {"type": "impulse", "name": "hit", "at": [0.01]},
  {"type": "modal", "name": "key", "excite": "hit", "material": "metal", "size": 0.08, "thickness": 0.0015},
  {"type": "output", "name": "out", "input": "key"}
]
```

Give the human a macro on `thickness` (exp curve, 0.0008 to 0.004) as the pitch and brightness knob of the object.

#### Measured

- `thickness_scales_every_mode_linearly`: wood plate 0.1 m, thickness 0.002 lowest mode 843.3 Hz, 0.004 1686.5 Hz, ratio 2.000; wood 0.3 m with thickness 0.025 equals the default wood (1171.2 Hz).
- `a_thin_metal_disc_of_8_cm_has_modes_between_3_and_9_khz`: metal 0.08 m x 1.5 mm peaks 1122, 2774, 2787, 4528, 5578, 5615, 7363, 9509 Hz (four between 3 and 9 kHz).
- The card's coin (metal 0.025 m x 1.5 mm) has one mode, at 11.5 kHz, and none between 3 and 9 kHz (the plate formula; test `a_thin_metal_disc_has_modes_between_3_and_9_khz` is `#[ignore]`d with this reason).
- `glass_shard_with_a_thickness_field_renders` (glass 0.03 m x 3 mm) and `thickness_is_stable_across_its_range_for_every_shape` (0.0003 to 0.05, five thicknesses, six shapes, two sizes): finite, peak under +3 dB, or the "no mode" error.

#### Honest limits

The plate formula, not a measurement of coins or keys. Thickness is a pitch knob: a small object that must ring in the 3 to 9 kHz band needs a size and thickness whose product keeps the lowest mode near 1 kHz.

## `modal` `f_min` / `f_max` / `f0`

Round-3 evidence: glass_clink, ice, coin and keys, four rounds running: no way to
keep the modes in a band, and no way to say "lowest mode 2 kHz" without solving
`size` by hand. Both fields are off by default; a `modal` without them renders
bit-identical to before (the pinned hashes in `the_default_plate_is_the_plate`
hold, and a band around the whole table is bit-identical to none).

### Fields (new, all optional)

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `f_min` | number | none | Hz, > 0 | Keep only modes at or above it. |
| `f_max` | number | none | Hz, > 0, above `f_min` | Keep only modes at or below it. |
| `f0` | signal (read once) | none | Hz, 20..18000 | Sets `size` so the lowest mode of the shape's table sits at `f0`. `size` becomes optional; giving both is an error, giving neither is an error. |

- The band is applied after the mode table (mistuning included) and before the
  `modes` cap, so `modes` counts kept modes and its lowest half are the lowest
  kept modes. The strongest mode (the reference for the 1e-4 weight cut and for
  the gains) is the strongest kept one; the output is normalized as always.
- `f0` solves `size = (a / f0)^(1/p)` from the same constants that build the
  tables (`lowest_mode`), with the effective material, `thickness` and shape:
  plate, bar, tube and shell go as 1 / size^2, membrane and string as 1 / size.
  The "lowest mode" is the table's lowest (the shell's hum, the string's
  fundamental); the seeded mistuning moves it by up to 1.5 % (plate) or 0.3 %
  (others), and a lowest mode under 20 Hz is dropped as always (so `f0` near 20
  can lose it). `f_min` above the lowest mode drops it and does not move `size`.
- `f0` is a number, `$param` or `=expr`, so a macro or a `voices` map can drive
  it; a macro that maps `f0` must not also map `size`.

### Errors

- `f_min 100 / f_max 200: no mode of this plate falls in the band; its 64 modes span 424.2 to 19240.4 Hz at this size, material and thickness: widen the band, or change `size` / `f0` / `thickness``
- `f_min 3000 is not below f_max 2000: give a band, f_min < f_max`
- `f_max: -5: give a frequency in Hz above 0`
- `size and f0 are both given: ...`; `give `size` (metres) or `f0` (Hz of the lowest mode)`
- `f0: 10: give the lowest mode in Hz, 20..18000`
- A band whose modes the strike does not excite (a node at `strike_position`)
  errors too and says to widen it or move `strike_position`.

### Measured (`crates/sfx_analysis/tests/m7.rs`, 48 kHz)

| What | Measured | Test |
|---|---|---|
| lowest spectral peak for `f0` 800 / 2000 / 5000, plate / bar / tube / membrane x wood / metal / glass | within 0.22 % (plate), 0.04 % (bar, membrane), 0.10 % (tube); limit 3 % | `f0_puts_the_lowest_mode_at_the_asked_frequency_for_every_shape` |
| string and shell, metal, `f0` 2000 | 0.04 % | same |
| bar `f0` 1500 at default and 2 mm thickness | 1500.5 Hz both | `f0_uses_the_effective_thickness` |
| spectrum outside `f_min` 1500 / `f_max` 4000 (a quarter-octave margin), metal plate 0.5 m | -83.6 dB relative (0.0 dB without the band); limit -40 | `f_min_and_f_max_remove_the_modes_outside_the_band` |
| `f_max` 2000 only; `f_min` 3000 only | -103.2 dB; -60.2 dB | same |
| all six shapes, 2.5 to 6 kHz band | -91.2 / -89.6 / -76.7 / -70.2 / -77.9 / -80.3 dB (plate, bar, tube, membrane, string, shell) | same |
| `modes: 3` inside a band | at most 3 peaks, -83.6 dB outside | `modes_counts_the_kept_modes_and_a_wide_band_changes_nothing` |

Honest limits: none of this has been heard. `f0` sets where the table's
lowest mode is, not what the object is; whether that mode is strong depends on
`strike_position` (a strike on a node leaves it out).
