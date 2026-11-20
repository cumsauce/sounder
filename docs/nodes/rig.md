# `rig`: bodies and joints stepped together

`impact` couples one moving body to one object. `rig` is the general form:
up to 16 bodies (point masses and modal objects) and 32 joints (contacts and
springs), all stepped together per sample. A hammer, a sheet and a loose
screw; a ball on a drum; a lid resting on a pan that is struck.

```json
{"type": "rig", "name": "roof",
 "bodies": [
   {"name": "hammer", "mass": 0.4, "velocity": -3.0},
   {"name": "sheet", "mass": 2.0, "fixed": true, "material": "metal", "size": 0.5,
    "thickness": 0.002, "loss": 0.4, "noise": 0.3},
   {"name": "screw", "mass": 0.005, "material": "metal", "shape": "bar", "f0": 2500,
    "loss": 0.6, "level_db": -6}],
 "joints": [
   {"kind": "contact", "a": "hammer", "b": "sheet", "at_b": 0.4, "hardness": 0.9},
   {"kind": "spring", "a": "screw", "b": "sheet", "at_b": 0.8, "gap": 0.0002,
    "stiffness": 2e5, "damping": 2}]}
```

## Bodies

`{name, mass, [fixed], [velocity], [drive], [material, shape, size | f0,
thickness, loss, tilt, noise, noise_width, modes 24], [level_db]}`.

- No `material`: a point mass (silent: a hammer, a ball, a weight).
- With `material`: a modal object, `modal`'s mode table. Each joint reads the
  modes at its own attachment point (`at_a` / `at_b`, as `strike_position`),
  so a plate struck in the middle and touched at its edge couples
  differently at the two. The table's modes are taken at the body's first
  joint; the others are matched by frequency (within 1 %, else 0).
- Modal mass is `mass` / 4 for every mode. `fixed` holds the body in place
  (its modes still ring); otherwise it translates under gravity and the
  joints.
- `velocity` m/s (up is positive), `drive` a force signal in newtons (up is
  positive).
- `level_db` sets how loud this body is in the output; only modal bodies
  radiate.

## Joints

`{kind, a, b, [at_a 0.3], [at_b 0.3], [gap 0], ...}`. `a` is the upper body,
`b` the lower, or `"ground"` (rigid, fixed, 50 GPa).

- `contact`: Hertz, k = 4/3 E* sqrt(`radius`) (E* from the two materials;
  a point mass uses `hardness` 0..1 as `contact` does), with Hunt-Crossley
  damping set at each touch so it keeps `bounce` of the incoming speed.
  `gap` is the starting distance (a ball 0.2 m above a plate is `gap` 0.2).
- `spring`: `stiffness` N/m and `damping` N s/m; with `gap` it has that much
  free play either side of its rest length and only pulls past it, so a
  loose part slaps against its fixing: a rattle.

## Rig fields

`gravity` (9.81; 0 for a hit in free space), `oversample` (4, raised so the
shortest contact or spring period spans 20 substeps, and the highest mode
stays stable; at most 64), `at`, `gain_db`, `seed`. The output peaks at
-6 dBFS before `gain_db`. Port `force`: the largest contact force each
sample, 1 at its peak.

## Measured (`crates/sfx_analysis/tests/it/rig.rs`)

- A 50 g ball dropped 0.2 m onto a fixed metal plate lands at 0.20 s
  (within 10 ms) and bounces with shrinking gaps.
- A free 2 kg tube dropped 0.1 m onto the ground rings at its 160 Hz `f0`
  (within 5 %).
- A hammer on a sheet with a screw on a spring: with 0.2 mm of free play the
  screw's band (2.5-3.5 kHz) carries over 3 dB more of the late sound,
  against the rest, than with none.
- Bit-identical across thread counts.

## Honest limits

- Heard once (batch 27, `docs/history/labels/batch27_findings.md`): the
  ball on a drum and the pan with its lid picked; the loose screw on a tin
  sheet heard as good detail but strange (a clean 2.5 kHz bar mode); with
  `noise` 0.7 and 4 dB lower it was picked (batch 28): give a rattling part
  `noise`.
- Vertical motion only: no rotation, no sliding (use `impact` for `slide`,
  `friction` and rolling).
- A contact keeps `bounce` through Hunt-Crossley alone: `impact`'s rebound
  cap (the modes giving their energy back to the bounce) is not here, so a
  body on a soft low mode can bounce more than `bounce` says.
- Each mode keeps a buffer as long as the sound.
