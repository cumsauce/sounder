# `step`

Footsteps after the foley project's contact model (`foley-synth/src/modal.rs`, `contact(Contact::Step, ..)`, MIT / Apache-2.0), called as it stands. A heel of about 8 kg strikes through a Hertz contact (a soft sole: 10 MPa, 5 cm round), and the surface's plate modes ring under it. The sole scuffs as the foot rolls, and the ball of the foot lands 60-120 ms later, softer. Gravel and sand crunch in grains, dirt crumbles, grass swishes round the shoe, snow crackles, and a wooden floor creaks now and then (15 %). Each step is normalized to foley's level, 0.4 x the material's loudness.

```json
{"type": "step", "name": "walk", "surface": "gravel", "speed": 1, "at": [0.005, 0.55], "seed": 1}
```

| Field | |
|---|---|
| `surface` | concrete (stone, pavement), gravel (sand), grass (leaves, plant), dirt (earth, mud), wood, metal, ice, snow, carpet (soft) |
| `speed` | heel speed, m/s, 0.2..3 (1 a walk): shorter, brighter contact and more crunch when faster |
| `at` | one time or a list (a walk); each step draws its own seed |
| `gain_db` | level |

Added after labels batch 11. There, the `footstep` node on concrete was heard as a snare / wooden drum, on gravel as a pitter-patter, and on grass as a blow. Measured on one step (`tests/it/step.rs`): metal centroid 229 Hz, ice 181, snow 131, carpet 48. The rest print in the test.

## After labels batch 12

The contact code is now a copy in `step.rs`, so it can change:
- `grit` (0..2, default 1) scales the grains and the scuff.
- The ball of the foot rings 3 x shorter (concrete's toe was heard "too resonant, wood-y").
- Grass is the gravel body with 12 dull grains and louder blade swishes (0.35). It measures 1.2 % of its energy above 2 kHz, against 0.1 % for the plant-plate version.
