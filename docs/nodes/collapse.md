# `collapse` and `debris`: many objects at once

A wall of 100 bricks built as `shatter` -> `voices` of a `modal` per brick
came out as 100 separate clean hits on a statistical clock. Two nodes fix the
two halves of that:

- **`collapse`** times the impacts from the fall itself, so the order and
  spacing are physical.
- **`debris`** sends every hit through a few shared `modal` bodies (stochastic
  modal synthesis, after Cook's PhISEM), so the hits merge into one pile the
  way a real one does, and 100 hits cost about as much as 10,000.

Use `voices` with a `rig` patch for the handful of hits a listener picks out
(the `hero` port).

## `collapse`

`{[rows 10], [cols 10], [brick 0.075 m], [width 0.22 m], [mode topple |
crumble], [lean 0.15 rad], [origin [0.5, 0.2]], [speed 2 m/s], [looseness
0.3], [restitution 0.25], [bounces 2], [pile 0.4], [hero 0], [at 0], [seed]}`
gives an event stream. Each event's `value` is the impact speed divided by
sqrt(2 g H), the speed of a fall from the top of the wall, clamped to 0..1.

- **topple**: the wall turns about its base as one slab (a thin rod, theta''
  = 3g / 2H sin theta, starting from `lean`). Each brick leaves the slab at an
  angle that grows toward the bottom courses, scattered by `looseness`, then
  falls from where it is with the slab's speed at that point. The top courses
  land first and hardest. Lowering `lean` lengthens the slow tip before the
  first hit (0.05 rad gave 0.9 s of silence on a 0.75 m wall).
- **crumble**: the wall gives way at `origin` ([column, row], each 0..1 of the
  wall) and the release spreads at `speed`. Each brick falls from rest.
- After the first hit, up to `bounces` more, each at `restitution` x the speed
  (scattered by +-20 %), 2v / g later, stopping under 0.25 m/s.
- `pile`: a fraction of the later bricks land on bricks that are already
  down, a shorter fall that grows with the pile.
- Ports: `out` (every hit), `ground` (first hits on the floor), `pile`,
  `bounce`, `hero` (the `hero` fastest first hits).
- A one-brick crumble with `looseness` 0 lands at exactly at + sqrt(2h / g)
  (tested).
- At most 20,000 bricks.

## `debris`

`{events, [material "stone"], [size | f0 900 Hz], [shape plate], [banks 6],
[spread 0.3], [noise 0.6], [loss 2], [tilt 2], [contact_ms 0.6], [ground],
[ground_material "earth"], [ground_size 0.8], [ground_db -10], [bed_db],
[gain_db], [seed]}`, mono.

- Each event is a Hertz contact pulse: a half sine lasting `contact_ms` x
  v^-0.2 with a smooth random roughness, at amplitude v^1.2 (v = the event
  value). A faster hit is shorter, so it is brighter (tested), and louder.
- Each pulse goes to one of `banks` `modal` bodies picked at random, each
  `spread` apart in size or `f0` and seeded apart. The banks are ordinary
  `modal` nodes, so `noise`, `loss`, `tilt` and the material work as they do
  there.
- `ground`: an event stream (usually `collapse.ground`) that also rings one
  shared floor body, with pulses three times longer.
- `bed_db`: low noise (two one-poles at 300 Hz) whose level follows the
  smoothed energy of the hits (60 ms): the rumble that is left when the hits
  are too dense to tell apart.
- Up to 200,000 events. 2,000 Poisson hits through 4 banks render in well
  under a second.

## The wall as built (not yet heard)

```json
[
  {"type": "collapse", "name": "wall", "rows": 10, "cols": 10, "looseness": 0.4, "pile": 0.45, "hero": 6, "at": 0.05},
  {"type": "debris", "name": "bricks", "events": "wall", "material": "stone", "f0": 900,
   "banks": 6, "spread": 0.35, "ground": "wall.ground", "bed_db": -22},
  {"type": "voices", "name": "heroes", "events": "wall.hero", "length": 0.6, "max": 16,
   "map": {"v": "-(1.5 + $value * 4)", "sz": "0.3 + $rand * 0.15"},
   "patch": [{"type": "rig", "name": "r", "bodies": [
       {"name": "brick", "mass": 2.5, "velocity": "$v", "material": "stone", "size": "$sz", "loss": 2, "noise": 0.6},
       {"name": "chip", "mass": 0.05, "material": "stone", "shape": "bar", "f0": 2600, "loss": 2, "noise": 0.6, "level_db": -10}],
     "joints": [{"kind": "contact", "a": "brick", "b": "ground", "bounce": 0.2, "hardness": 0.8},
                {"kind": "spring", "a": "chip", "b": "brick", "gap": 0.0003, "stiffness": 1e5, "damping": 3}]}]},
  {"type": "gain", "name": "heroes_g", "input": "heroes", "db": -4},
  {"type": "mix", "name": "dry", "inputs": ["bricks", "heroes_g"], "to": "mono"},
  {"type": "finish", "name": "bus", "input": "dry", "punch": 0.3, "glue": 0.4, "weight_db": 3},
  {"type": "output", "name": "out", "input": "bus", "peak_db": -1}
]
```

Measured with the default `lean` 0.05 (before it became 0.15), 4 s: hits
from 0.92 to 1.44 s, energy under 200 Hz / 200 Hz-2 kHz / 2-8 kHz 53 / 42 /
5 %, voiced 0 %, `verify` passes on seeds 1-4. `rig` takes 16 bodies at most
and solves at the sample rate, so it is for the heroes, not the whole wall.
