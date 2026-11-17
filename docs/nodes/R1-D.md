# R1-D: `modal` `loss`, `tilt` and the wider size range

Round 1 (keys_jingle, glass_break, glass_clink, 6 sounds): no way to change
the ring time of a named material, a glass plate that put 98 % of its energy in
one mode, and a size hint that stopped at a 10 kHz lowest mode (3 mm glass was
refused under 0.039 m, 5 cm metal under 0.16 m).

## New fields (all optional; omitted = bit-identical to before)

- `loss` (number, 0.01..100, default 1): multiplier on the material's loss,
  for a named material or `{young, ...}`. `"material": "wood", "loss": 0.5`
  rings twice as long. A mode's T60 is still capped at 20 s (and floored at
  2 ms). Out of range is an error naming the range.
- `tilt` (dB per octave, -12..12, default 0): each kept mode's gain is
  multiplied by `10^(tilt * log2(f / f_lowest) / 20)`, f_lowest the lowest kept
  mode. Positive lifts the upper modes (flattens a table that is all in its
  first mode), negative darkens. It acts after the modes are chosen and before
  the level normalization, so a unit impulse still peaks near 0 dBFS.

## Size range

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

## Measured (tests in `crates/sfx_analysis/tests/m7.rs`)

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
