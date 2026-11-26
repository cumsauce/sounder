# `grind`: a pile that is still moving, or one rolling object

`debris` only does impacts. A real pile also slides, grinds and tumbles for a
while after the last big hit. `grind` makes that texture, and its level
follows how much is still moving.

`{[events], [speed], [material "stone"], [roughness], [size], [f0], [settle
1.5], [top_speed 1], [micro_rate 40], [micro_db -6], [gain_db], [seed]}`,
mono. Give `events`, `speed`, or both.

- **Activity.** Each event adds its value (0..1) to a state that falls 60 dB
  over `settle` seconds, is smoothed over 10 ms and soft-saturated
  (1 - e^-x); at 1 the scrape runs at `top_speed` m/s. A `speed` signal (m/s of
  one object, or a motion energy from a `follower`) is added as it is.
- **Scrape.** The activity is the `speed` of a `rolling` force
  (roughness, grit `size`), which rings a `modal` plate of the material at
  `f0`. Silence at activity 0.
- **Micro-impacts.** Small grit hits at `micro_rate` x activity per second,
  amplitude activity^1.2, through a second body at 1.7 x `f0`, `micro_db`
  under the scrape. `micro_rate: 0` turns them off.
- **Materials** set defaults (roughness, grit size, f0, modal noise / loss):
  stone 0.7 / 4 mm / 700 Hz, wood 0.5 / 3 mm / 500 Hz, metal 0.4 / 2 mm /
  1500 Hz (noise 0.4, loss 0.5), glass 0.3 / 1.5 mm / 2500 Hz.
- Up to 200,000 events.

Measured (tests/it/surface.rs, `grind`): after six hits (`settle` 1.0) the level
is -12.6 dB just after, -43 dB 0.5 s later, -81 dB at 1.2 s; a longer `settle` holds the level
10 dB higher a second on; centroid wood 548 Hz, metal 1438 Hz; a `ramp` of
speed 0..2 m/s raises the level by over 6 dB. Not yet heard.

## Uses

```json
[
  {"type": "collapse", "name": "wall", "hero": 4},
  {"type": "debris", "name": "bricks", "events": "wall", "ground": "wall.ground"},
  {"type": "grind", "name": "slide", "events": "wall", "material": "stone", "settle": 2, "gain_db": -12},
  {"type": "mix", "name": "dry", "inputs": ["bricks", "slide"], "to": "mono"}
]
```

A ball rolling (the library has no good one): a `ramp` or `wander` for the
speed into `grind` `speed`, `material` wood or glass, a `duration`.
