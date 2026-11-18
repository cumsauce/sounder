# Reverb models, ir

Spaces: `reverb` gains `model`, `er_mix` and `width`; `ir` is a new source that
makes an impulse response for `convolve`. Code: `nodes/time.rs` (the fdn and the
dispatch), `nodes/space.rs` (room, plate, spring, `ir`). Tests:
`crates/sfx_analysis/tests/it/space.rs`.

## `reverb`

A reverb (time family), stereo out, port `out` (dry + wet) and `wet` (tail
alone). `model` picks the algorithm; `fdn` is the node as it was before
models existed and its output is bit-identical (pinned by a hash test).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | summed to mono for the tail |
| `size` | number | 0.5 | 0 .. 1 (clamped) | `fdn`: line lengths x 0.35 .. 2.0; `room`: longest side 3 .. 20 m (sides 1 : 0.8 : 0.4); `plate`: line lengths x 0.6 .. 1.4; `spring`: loop 20 .. 80 ms |
| `decay` | number | 1.5 | s, >= 0.05 | time for the tail to fall 60 dB |
| `damp` | number | 0.3 (`plate` 0.1) | 0 .. 0.99 (clamped) | high-frequency damping in the loop |
| `mix` | number | 0.3 | 0 .. 1 (clamped) | wet share of `out` |
| `predelay` | number | 0.01 | s, >= 0 | delay before the tail starts |
| `model` | word | `fdn` | `fdn` `room` `plate` `spring` | algorithm |
| `er_mix` | number | 0.3 | 0 .. 1 | `room` only: share of early reflections in the wet signal (0 tail alone, 1 reflections alone). An error for other models: `er_mix: 0.5: only model room has early reflections: remove er_mix or set "model": "room"` |
| `width` | number | the model's own | 0 .. 2 | wet stereo width: side signal times `width` (0 mono, 2 doubled). Out of range: `width: 3: give a number from 0 (mono) to 2` |

Nothing scales with a field: line lengths, the 40-section spring chain and the
63 image sources per ear are fixed (cost is the document length times a constant).

- `fdn`: 8 delay lines (29.7 to 73.3 ms at size 1 before scaling), Hadamard feedback.
- `room`: a shoebox (source at 0.3 / 0.45 / 0.4 of the sides, listener at 0.7 / 0.5 / 0.4,
  ears 18 cm apart). Image sources up to 3 wall hits give early reflections (no
  direct sound: that is the dry path) at time (r - r_direct) / 343 m/s plus
  `predelay`, amplitude beta^hits x r_direct / r. Wall absorption is derived from
  `decay` by Sabine's formula so reflections and tail agree. The fdn tail starts
  half the reflection span after `predelay`. The wet signal is
  `tail x (1 - er_mix) + reflections x er_mix x 0.25`.
- `plate`: four Schroeder allpass diffusers (4.7, 3.6, 12.7, 9.3 ms) into 8
  shorter lines (11.3 to 31.7 ms), Hadamard feedback, `damp` default 0.1.
- `spring`: one feedback loop, delay (20 to 80 ms) -> 40 identical second-order
  allpass sections (poles r 0.8 at 4 kHz, group delay peaks there) -> one-pole
  damping -> back. Each round trip repeats the chirp. Channel 2 reads the loop
  through a chain tuned 15 % higher. Loop gain is chosen so a round trip
  loses 60 dB per `decay` (capped at 0.999).

### Recipe

A spring "boing" from a short noise snap, and the same snap in a 14 m room via
an `ir`; give the human macros on `decay` and the spring's `size`.

```json
[
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "decay", "name": "e", "time": 0.004},
  {"type": "math", "name": "snap", "op": "mul", "a": "n", "b": "e"},
  {"type": "ir", "name": "hall", "model": "room", "size": 14, "absorption": 0.15},
  {"type": "convolve", "name": "inroom", "input": "snap", "ir": "hall", "mix": 1.0},
  {"type": "reverb", "name": "boing", "input": "snap", "model": "spring", "size": 0.4, "decay": 1.2, "mix": 1.0},
  {"type": "mix", "name": "both", "inputs": ["inroom", "boing"], "to": "stereo"}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| default model output vs the code before this task | 6 hashes equal (default, tuned, extreme values; `out` and `wet`) | `default_reverb_is_bit_identical_to_before_this_task` |
| T60 (Schroeder, -5 to -35 dB fit) at decay 1.5, damp 0 | fdn 1.509, room 1.510, plate 1.502, spring 1.472 s | `t60_is_within_20_percent_of_decay_for_every_model` |
| same, default damping (0.3) | fdn 1.328, room 1.299, plate 1.325, spring 1.338 s (-10.8 to -13.4 %; damping shortens the broadband tail) | same |
| T60 over decay 0.8 / 1.5 / 3.0, both damps | worst -13.4 % (room, damp 0.3), spring damp 0 -2.0 % | same |
| spring first pass, 500 Hz vs 4 kHz band (400-600, 3500-4500 Hz) | 10 % energy at 59.52 vs 67.12 ms; centroid 61.86 vs 67.59 ms: 4 kHz is 5.73 ms later | `spring_four_khz_arrives_after_five_hundred_hz` |
| plate vs fdn, first 60 ms | 1 ms windows within 30 dB of the peak: plate 0.60, fdn 0.03; first echo 21.3 vs 44.9 ms | `plate_builds_faster_and_denser_than_fdn` |
| room, first reflection at the left ear | geometry says 3.754 ms, found 3.750 ms (within 2 samples) | `room_reverb_first_reflection_arrives_when_the_geometry_says` |
| room er_mix 1 vs fdn, rms of first 50 ms | -37.0 vs -51.6 dB | same |
| plate side rms, width 0 / own / 2 | -120 (L = R) / -35.8 / -29.8 dB (+6.0 dB) | `width_scales_the_side_signal` |

### Honest limits

- The spring's chirp direction (high band later) follows the card; whether it
  matches a real spring is not measured. Its T60 reads 2 % under `decay` at
  damp 0 because the round trip is estimated at 1.5 kHz.
- `room` early reflections use frequency-independent walls and nearest-sample
  delays; the tail is the fdn, so the two are matched in T60 and not in spectrum.
  Fixed geometry (listener and source positions, ratios); `size` only scales it.
- Damping makes the measured broadband T60 up to 13 % shorter than `decay`
  (`decay` is the low-frequency time). `fdn` clamps its fields silently, as
  before; the new fields error.
- Finite and bounded at size 0 / 1, decay 0 / 1e6, damp 0 / 1, predelay 0 / 1e9,
  width 0 / 2 for all four models (input amplitude 1e3, peak under 1e5;
  `every_model_is_finite_at_the_extremes`). Not heard.

## `ir`

A source (no input) whose output is an impulse response: the direct sound at
sample 0, zeros after `length`, to feed `convolve`'s `ir`. `room` is stereo
(two ears), the others mono. Random (reads the seed) only for `room`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `model` | word | `room` | `room` `tube` `box` `cabinet` `phone` | what to model |
| `size` | number | room 6, tube 1, box 0.5, cabinet 0.6 | m, 0.05 .. 100 | room: longest side (others 0.8x, 0.4x); tube: length; box: longest side (others 0.8x, 0.4x); cabinet: width, sets the resonance at 70 / size Hz. Error for `phone` |
| `absorption` | number | room 0.3, tube 0.1, box 0.3 | 0.005 .. 1 | room: mean wall absorption (Sabine T60 = 0.161 V / (S absorption)); tube, box: share of pressure lost per round trip. Error for `cabinet`, `phone` |
| `length` | number | see below | s, 0.01 .. 30 | seconds of response; also cut at the document length |
| `seed` | integer | 0 | any | the room's noise (mixed with the document seed) |

Default `length`: room 1.2 x T60 (0.01 to 10 s); tube and box until -60 dB (up to
5 s); cabinet 0.1; phone 0.05. The response is zero-padded to the document
length. Errors name the field: `size: 500: give a number from 0.05 to 100`.

- `room`: the same image-source set as `reverb` (with the direct sound), plus
  white noise through a low-pass that closes from 14 kHz by 0.35x per T60,
  level-normalised, times exp(-t / tau) with tau = T60 / 6.908, faded in across the
  reflections. Its level: the reverberant energy is (r / r_critical)^2 of the
  direct energy, r_critical = sqrt(A / 16 pi), A = 0.161 V / T60, minus what the
  reflections already hold (at least a quarter of it is the tail).
- `tube`: a comb loop with round trip 2 size / 343 s, gain 1 - absorption, a
  one-pole low-pass (0.25) per trip: modes at k x 343 / (2 size).
- `box`: three such combs (axes size, 0.8 size, 0.4 size), each at 1/3.
- `cabinet`: unit impulse through 80 Hz high-pass, a +6 dB Q 2.5 peak at 70 / size Hz, and a 4-pole 5 kHz low-pass.
- `phone`: unit impulse through Butterworth order 6 high-pass at 250 Hz and low-pass at 3.8 kHz.

### Recipe

See the `reverb` recipe above (`hall` -> `convolve`). Give the human a macro on
`absorption` (length of the room) and `size`.

### Measured

| What | Measured | Test |
|---|---|---|
| room T60 vs Sabine, 5 rooms (3 to 20 m, absorption 0.05 to 0.5) | +1.5, +0.1, +0.2, +3.6, +0.1 % (worst 3.6 %) | `room_ir_energy_decay_matches_sabine` |
| room, direct sound | 1.000000 at sample 0 | same |
| room left/right correlation | 0.069 | `room_ir_is_stereo_and_seeded` |
| phone, re 1 kHz | 300 Hz -0.46 dB, 3.4 kHz -0.97 dB, 100 Hz -47.8 dB, 8 kHz -42.8 dB | `phone_ir_passes_300_to_3400_and_cuts_100_and_8000` |
| tube 1 m, spectral peaks | 171.5, 343.0, 514.5, 686.0 Hz (wanted the same) | `tube_ir_has_modes_at_k_c_over_2l` |
| box 0.5 m, peaks | 343.1, 428.6, 857.5 Hz (wanted 343, 428.75, 857.5) | `box_ir_has_the_modes_of_all_three_axes` |
| cabinet 0.6 | resonance 118 Hz (wanted 117), +5.1 dB re 1 kHz; 40 Hz -12.2 dB, 10 kHz -28.7 dB | `cabinet_ir_has_a_low_resonance_and_rolls_off` |
| `convolve` of an impulse with an `ir` node | equals the response, max difference 1.5e-8 | `convolve_with_an_ir_node_returns_the_response` |

### Honest limits

- Room walls are frequency-independent; the tail's colour (a closing low-pass)
  is a choice, not a measurement of a real room. The Sabine T60 is a
  statistical formula and holds for rooms that are not very absorbent.
- Reflections are placed at the nearest sample.
- `tube` and `box` are comb filters, not a wave solution; `cabinet` and
  `phone` are filters with a short ring, not measured devices.
- The response can be at most the document's length; a `length` of 30 s needs a
  document that long (and costs time proportional to it).
- Finite and bounded for every model at size 0.05 / 100, absorption 0.005 / 1,
  length 30 (`every_model_is_finite_at_the_extremes`). Not heard.
