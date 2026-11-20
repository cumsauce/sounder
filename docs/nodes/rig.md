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
- A contact's `damp` (default `"body"`) puts the Hunt-Crossley damping on
  the bodies' own motion, so a bounce loses energy while the modes ring on,
  and holds the rebound of a free body off a held one (or the ground) to
  `bounce` x the speed it came in at; touches within 3 ms are chatter of the
  same landing. `"point"` damps the full speed at the touch with no cap (the
  batch 27 / 28 rigs and their parts use it).
- A contact's `slide` (m/s, a signal), `friction` `[static, dynamic]` and
  `stribeck` (0.1 m/s) drag `a` along `b`: a Stribeck stick-slip force on a
  sliding copy of both sides' modes, as `impact`'s `friction`. A 50 g bow
  pressed with 3 N at 0.01-0.06 m/s (`stribeck` 0.05) sings a metal bar on
  its second mode (1655 Hz for f0 600) with its harmonics; at 0.15 m/s it is
  quiet.
- `spring`: `stiffness` N/m and `damping` N s/m; with `gap` it has that much
  free play either side of its rest length and only pulls past it, so a
  loose part slaps against its fixing: a rattle.

## Rig fields

`gravity` (9.81; 0 for a hit in free space), `oversample` (4, raised so the
shortest contact or spring period spans 20 substeps, and the highest mode
stays stable; at most 64), `at`, `gain_db`, `seed`. The output peaks at
-6 dBFS before `gain_db`. Port `force`: the largest contact force each
sample above the upper body's resting load (its weight and a steady
downward `drive`), 1 at its peak, so it reads 0 once a body rests and a
layer multiplied by it stops (the raw force kept a crack hissing under a
chair at rest). Port `contact`: the raw contact force, 1 at its peak (it
reads the weight at rest: use it to detect touches).

## Measured (`crates/sfx_analysis/tests/it/rig.rs`)

- A 50 g ball dropped 0.2 m onto a fixed metal plate lands at 0.20 s
  (within 10 ms) and bounces with shrinking gaps.
- A free 2 kg tube dropped 0.1 m onto the ground rings at its 160 Hz `f0`
  (within 5 %).
- A hammer on a sheet with a screw on a spring: with 0.2 mm of free play the
  screw's band (2.5-3.5 kHz) carries over 3 dB more of the late sound,
  against the rest, than with none.
- A free tube with `bounce` 0 dropped 0.3 m never flies again.
- Bit-identical across thread counts.

## Honest limits

- Heard once (batch 27, `docs/history/labels/batch27_findings.md`): the
  ball on a drum and the pan with its lid picked; the loose screw on a tin
  sheet heard as good detail but strange (a clean 2.5 kHz bar mode); with
  `noise` 0.7 and 4 dB lower it was picked (batch 28): give a rattling part
  `noise`.
- Vertical motion and sliding along a contact; no rotation, no rolling.
  One axis means a clapper meets only what is above or below it, never
  several objects around it in turn: a wind chime is one clapper per tube,
  each driven by its own `wander` (built that way in batch 30 and picked).
- With `damp` "body", a real rebound off a held body leaves at exactly
  `bounce` x its landing speed (a touch that leaves slower than 30 % of that
  is chatter and is only capped): a 5 g marble in a held glass bowl at
  `bounce` 0.75 keeps landing with flights 0.75 of the one before; before
  this the bowl's modes took its bounce and it stopped after 3 landings.
- A free body resting on the ground has its lowest mode pinned by the
  contact: a 2 kg tube rings at its 160 Hz `f0` while it lands, then the
  resting contact passes the vibration into its rigid motion and it quietens
  (about 35 dB by 0.6 s). Without the cap and with `"point"` damping, it
  bounced like rubber instead (0.26 s gaps for `bounce` 0.3).
- Each mode keeps a buffer as long as the sound.

## Waveguide bodies and the air joint

A body with `waveguide` is a string or a tube as a digital waveguide: two
delay lines of velocity waves, reflected at the ends, stepped once per rig
substep (`crates/sfx_graph/src/nodes/rig_waveguide.rs`). It does not
translate and takes no `material`.

- `kind: "string"` (default): fixed at both ends; `f0`, or `length` with
  `tension` (N; f0 = sqrt(T L / m) / 2L with the body's `mass` as the
  string's). Wave impedance 2 m f0.
- `kind: "tube"`: `length` (or `f0`), `ends` `open` (c / 2L), `closed`
  (closed at 0, open at 1: c / 4L), `stopped` (both closed, c / 2L),
  `radius` (0.02 m; impedance rho c A). `mass` is ignored.
- `decay` (s for the lowest frequency to fall 60 dB; string 3, tube 0.8),
  `damping` 0..1 (0.3; a one-pole low-pass in the loop, 16 kHz at 0 down to
  320 Hz at 1; its delay is taken off the line so the tuning holds).
- A joint's `at_a` / `at_b` is a point 0..1 along it: the joint reads the
  velocity there (and its integral, leaking at about 5 Hz, as the
  displacement) and pushes F / 4Z into both lines. A contact cannot push it
  further than the overlap in one substep (a stiff striker meets the string
  at its impedance, not at its Hertz stiffness).
- It radiates from end 1: the velocity at an open end, the force at a fixed
  one, through a DC blocker near 15 Hz.
- `kind: "air"` joint (`a` any body, `b` a tube; `strength` 0..20, default
  1): `a`'s vibration velocity at `at_a` (its rigid motion left out: a
  falling body does not blow) drives end 0 of the tube, and the air pushes
  back on `a` with -strength x Z x (the pressure wave at end 0). A drum skin
  over its shell, a struck pipe's air column, a body radiating into a
  resonator.

Measured (`tests/it/rig.rs`): a struck string rings at its f0 within 2 % (110
and 330 Hz, and from length and tension); a 0.5 m tube's lowest partial is
343 Hz open and 171.5 Hz closed within 3 %; a stick on a 900 Hz skin with an
air joint (strength 2) into a closed 0.8 m tube raises the 90-125 Hz band
against the skin's band by over 10 dB against strength 0. A spring of up to
2e5 N/m on a 2 g string stays finite. Not heard yet: batch 29
(`tools/labels/gen29.py`).

A contact that touches a waveguide damps the full speed at the touch (as
`damp: "point"`): a waveguide has no rigid motion for `body` damping to act
on, and counts as held for the rebound cap.

Listen page for batch 29: https://claude.ai/artifact/VTX9qodU1K7Fbbx8eBCLSE
(`picks` collection).

## Heard (batch 29)

The waveguide string on a wooden body was picked over a plain `string`
node (part `rig_string_on_box`). The `air` joint lost both boxes it was in:
a drum with a shell tube and a pipe with its air column were each beaten by
the build without it. `docs/history/labels/batch29_findings.md`.
