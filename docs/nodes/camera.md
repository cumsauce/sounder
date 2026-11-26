# camera

Where the microphone stands, for a **video or film render**. A game sound
stays dry (AGENTS.md, "Layering a designed sound"): the engine places it and
adds the room. A video has no engine after the render, so `camera` does that
job offline. It is not a change of default: with no `distance` and no
`space` it passes its input through bit for bit.

Place it on the finished, dry mono sound, before `output`:

```json
{"type": "camera", "name": "cam", "input": "dry", "distance": 25,
 "source_height": 0.3, "camera_height": 1.6, "ground": 0.6, "space": "outdoor"}
```

| Field | Default | What it does |
|---|---|---|
| `input` | | mono source (a stereo input errors: fold with `mix` `to: mono`) |
| `distance` | off | metres to the source; a number or a signal (a `ramp` for a source moving away) |
| `ref_distance` | 1 | the distance the sound was designed at: the level is unchanged there |
| `inverse_law` | true | 1/r pressure: -6.02 dB per doubling past `ref_distance` |
| `air` | true | ISO 9613-1 air absorption (20 C, 1 atm) |
| `humidity` | 50 | percent, 1..100 |
| `delay` | false | distance / 343 m/s of delay; a moving distance gives Doppler |
| `source_height` | off | m above the ground; turns on the ground reflection |
| `camera_height` | 1.6 | m above the ground |
| `ground` | 0.8 | pressure reflection of the ground, 0..1 (asphalt ~0.8, grass ~0.5) |
| `space` | none | none, outdoor, street, room, hall |
| `space_db` | 0 | the space's level against the direct sound at `ref_distance` |
| `stereo` | true | with a space: stereo out; false folds to mono |

Fields that need another (`air` without `distance`, `space_db` without a
`space`, `camera_height` without `source_height`) are errors, so a knob never
silently does nothing.

## How each part works

- **Air.** The ISO 9613-1 pure-tone absorption (`air_db_per_m`) is matched
  by two one-pole low-passes whose 6 dB point is where the air takes 6 dB
  over the distance; the cutoff is updated every 32 samples, so a moving
  source darkens as it leaves. Below about 20 m nothing under 20 kHz loses
  6 dB and the filter is bypassed. A two-pole slope falls slower than the
  air's f^2 far above the cutoff: the loss there is under-stated.
- **Ground.** One reflected path of length sqrt(d^2 + (hs + hl)^2) against
  the direct sqrt(d^2 + (hs - hl)^2), at 1/r of its own path times `ground`,
  with its own air filter: a comb with its first notch at c / (2 x path
  difference). The reflection is frequency-flat (no impedance model of the
  ground).
- **Space.** Built on the existing models in `crates/sfx_graph/src/nodes/space.rs`
  and `time.rs`, not a copy: `room` and `hall` are `reverb` model `room`
  (shoebox early reflections + fdn) at size 0.12 / 0.88 and T60 0.6 / 2.2 s;
  `street` the room model at size 0.75, T60 0.9 s with more early
  reflections (flutter between facades); `outdoor` two low-passed slaps at
  233 and 410 ms (a facade 40 m off, trees 70 m off) over a faint 0.5 s fdn
  tail. The space is driven by the dry source at the reference level and
  does not fall with distance (a diffuse field), so the direct-to-room ratio
  drops 6 dB per doubling: measured 18.1 dB lower at 8 m than at 1 m
  (`farther_camera_hears_more_room`).

## The output target undoes the distance

`output` `peak_db` and `lufs` renormalize the whole sound, so a far camera
comes out as loud as a near one: only the tone (air, room share) moves.
To hear distance as level, give the output `gain_db` only (no target), or
render every shot of a scene with the same `gain_db`. With `inverse_law:
false` the direct sound keeps its level and the room rises instead (by
distance / `ref_distance`), which keeps the same direct-to-room ratio and
works with a target.

## Measured (tests in `crates/sfx_analysis/tests/it/space.rs`, `mod camera`)

- off: bit-identical to the input;
- 2 m -> 4 m: -6.02 dB (+-0.1); `inverse_law: false` at 40 m: the input's level;
- 200 m: 8-12 kHz down more than 15 dB, 200 Hz-1 kHz under 1 dB;
- `delay` at 34.3 m: the impulse 100 ms later (+-2 samples);
- ground at 10 m, both heights 1 m, `ground` 1: the reflection 0.58 ms after
  the direct sound at 0.98 of its level;
- `hall` 1 s into the render is more than 10 dB above `room`.

Not heard yet: whether the presets read as the spaces they are named for is
the listener's call.

## Later

Position (azimuth, a moving pan, binaural) for spatial audio; a ground
impedance model; per-preset early reflections tied to the camera's place.
