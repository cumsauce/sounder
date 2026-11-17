# R2-E: `modal` thickness

## `modal` (new field `thickness`)

`thickness` overrides the plate, bar, tube, membrane or string thickness of the material, so small thin objects need no custom material.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `thickness` | signal (read once) | the material's own | metres, 0.0001 .. 0.5 | every mode frequency goes linearly with it; the size range in the error and hint uses it; outside the range is an error naming the range |

Giving a custom material's own thickness, or the named material's (wood 0.025, glass 0.01, metal 0.05), changes nothing: renders are bit-identical to leaving the field out, and the pinned hashes in `tests/m7.rs` hold.

### Recipe

```json
[
  {"type": "impulse", "name": "hit", "at": [0.01]},
  {"type": "modal", "name": "key", "excite": "hit", "material": "metal", "size": 0.08, "thickness": 0.0015},
  {"type": "output", "name": "out", "input": "key"}
]
```

Give the human a macro on `thickness` (exp curve, 0.0008 to 0.004) as the pitch and brightness knob of the object.

### Measured

- `thickness_scales_every_mode_linearly`: wood plate 0.1 m, thickness 0.002 lowest mode 843.3 Hz, 0.004 1686.5 Hz, ratio 2.000; wood 0.3 m with thickness 0.025 equals the default wood (1171.2 Hz).
- `a_thin_metal_disc_of_8_cm_has_modes_between_3_and_9_khz`: metal 0.08 m x 1.5 mm peaks 1122, 2774, 2787, 4528, 5578, 5615, 7363, 9509 Hz (four between 3 and 9 kHz).
- The card's coin (metal 0.025 m x 1.5 mm) has one mode, at 11.5 kHz, and none between 3 and 9 kHz (the plate formula; test `a_thin_metal_disc_has_modes_between_3_and_9_khz` is `#[ignore]`d with this reason).
- `glass_shard_with_a_thickness_field_renders` (glass 0.03 m x 3 mm) and `thickness_is_stable_across_its_range_for_every_shape` (0.0003 to 0.05, five thicknesses, six shapes, two sizes): finite, peak under +3 dB, or the "no mode" error.

### Honest limits

The plate formula, not a measurement of coins or keys. Thickness is a pitch knob: a small object that must ring in the 3 to 9 kHz band needs a size and thickness whose product keeps the lowest mode near 1 kHz.
