# R3-C: `modal` `f_min` / `f_max` / `f0`

Round-3 evidence: glass_clink, ice, coin and keys, four rounds running: no way to
keep the modes in a band, and no way to say "lowest mode 2 kHz" without solving
`size` by hand. Both fields are off by default; a `modal` without them renders
bit-identical to before (the pinned hashes in `the_default_plate_is_the_plate`
hold, and a band around the whole table is bit-identical to none).

## Fields (new, all optional)

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

## Errors

- `f_min 100 / f_max 200: no mode of this plate falls in the band; its 64 modes span 424.2 to 19240.4 Hz at this size, material and thickness: widen the band, or change `size` / `f0` / `thickness``
- `f_min 3000 is not below f_max 2000: give a band, f_min < f_max`
- `f_max: -5: give a frequency in Hz above 0`
- `size and f0 are both given: ...`; `give `size` (metres) or `f0` (Hz of the lowest mode)`
- `f0: 10: give the lowest mode in Hz, 20..18000`
- A band whose modes the strike does not excite (a node at `strike_position`)
  errors too and says to widen it or move `strike_position`.

## Measured (`crates/sfx_analysis/tests/m7.rs`, 48 kHz)

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
