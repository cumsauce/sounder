# Click

## `click`

A mechanical click (physical-model family, source): one or two transients (a
press, then a release or second snick 24 to 72 ms later by `kind`) through
three or four small body resonances at 1.5 to 8 kHz at `size` 0.5. It is the
single-node route for a door latch, a light switch, a button, a lock or a
buckle. Files: `crates/sfx_graph/src/nodes/mechanism.rs`,
`crates/sfx_analysis/tests/mechanism.rs`.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `kind` | word | required | `latch` `switch` `button` `lock` `clasp` | Sets the gap to the second transient (45, 24, 70, 72, 35 ms), the body modes, the ring time and how the second transient differs (level 0.55..0.8 of the press, 0.7..0.92 of its frequencies). |
| `size` | number | `0.5` | 0 .. 1 | Body modes scale by 4^(0.5 - size) (x2 at 0, x0.5 at 1); ring time by 2^(size - 0.5). Bigger is lower and longer. |
| `hardness` | number | `0.7` | 0 .. 1 | Contact pulse 0.4 ms (0) to 0.06 ms (1), upper modes tilted down by (f/f0)^(-1.5 (1 - hardness)), noise tick scaled by hardness. Harder is shorter and brighter. |
| `at` | number | `0` | seconds, >= 0 | Time of the press. A value past the end of the document gives silence. |
| `double` | bool | `true` | | `false` renders the press alone; the press is the same samples either way. |
| `seed` | integer | `0` | | Mixed with the document seed and the node name. Moves every mode frequency by +-3 % (independently for the two transients), the gap by +-3 % and each transient's level by +-1.5 dB. |

All fields are plain numbers or words (no signals). Errors name the field and
say what to give:

- `size: 1.5: give a number from 0 to 1` (same for `hardness`)
- `at: -1: give a number >= 0`
- `kind`: `unknown variant \`squeak\`, expected one of \`latch\`, \`switch\`, \`button\`, \`lock\`, \`clasp\``
- a sample rate so low that every mode is above 0.9 x Nyquist: an error naming `size` and the sample rate.

Modes above 0.9 of the usable top are dropped, not folded. The node is random
(reads the seed) and not causal (the press is scaled by its own peak, found after rendering it). No field scales an allocation: the longest local buffer is 1 s.

Output level: the press peaks at 0.5 (-6 dBFS) whatever the kind, size or
hardness, and the second transient is relative to it. If the second ever comes
out louder (a soft, small lock) the whole click is scaled to peak at 0.5.
Scale it with a `gain`.

**Fade-in.** A hit at `at` 0 starts at sample 0 and its peak is inside the
first millisecond. The output's default 2 ms fade-in cuts it (measured: peak
0.50 becomes 0.30 for a `button`). Give the `output` `fade_in: 0.0005`
(0.50 becomes 0.38) or `fade_in: 0` (kept whole; `silent_edges` then
complains about a non-zero first sample), or put the click a few ms in with
`at: 0.005`.

### Recipe

A latch at the end of a door slam (alone here): the press and the catch 45 ms
later, a little larger and harder than default. Give the human macros on
`size` (how big the mechanism is) and `hardness` (how sharp the contact is);
both are plain number fields, so map them with `macros`. A light switch is
`{"kind": "switch", "size": 0.35, "hardness": 0.85}`; layer a `lock` under a
`modal` thud for a door.

```json
[
  {"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.0},
  {"type": "gain", "name": "hit", "input": "latch", "gain": 0.8},
  {"type": "output", "name": "out", "input": "hit", "fade_in": 0.0005, "peak_db": -3}
]
```

### Measured

Tests in `crates/sfx_analysis/tests/mechanism.rs`, 48 kHz, 0.4 s renders of the
node. "Centroid" is the power-weighted mean frequency of one FFT over the whole
click (the descriptors' frame centroid reads a 30 ms click through windows
longer than the click). "Length" is the time to the last sample within 50 dB of
the peak.

| What | Measured | Test |
|---|---|---|
| latch descriptor onsets, seeds 0..5 | 2 each, at 0.000 and 0.032 or 0.043 s (limit 25..60 ms apart; the onset picker places the second a frame before the real 45 ms) | `latch_has_two_onsets_25_to_60_ms_apart` |
| second transient time (where `double` true and false part) | latch 45.27, switch 24.15, button 70.44, lock 72.46, clasp 35.21 ms (limit 20..80) | `every_kind_has_its_second_transient_20_to_80_ms_after_the_first` |
| default centroid / length | latch 1987 Hz / 71.7 ms, switch 3198 Hz / 34.4 ms, button 1558 Hz / 90.0 ms, lock 1235 Hz / 115.9 ms, clasp 2578 Hz / 51.3 ms | same |
| switch | centroid 3198 Hz (limit 2000..6000), length 34.4 ms (limit 120) | `switch_is_bright_and_short` |
| centroid at size .1 .3 .5 .7 .9 (falls at every step, all 5 kinds) | latch 3205 2542 1987 1539 1182; switch 4707 3874 3198 2623 2108; button 2585 2018 1558 1195 912; lock 2051 1599 1235 948 724; clasp 3977 3206 2578 2045 1607 Hz | `bigger_size_lowers_the_centroid` |
| centroid at hardness 0 .35 .7 1 (latch) | 1717 1811 1987 2286 Hz | `harder_is_brighter` |
| seed 1 vs 2 (latch) | summed abs difference is 0.85 of the summed abs signal; spectral peak 1733 vs 1820 Hz | `seeds_differ_by_a_few_percent` |
| hit at `at` 0 (button) | first sample 0.0102, peak of the first ms 0.5000; through `output`: 0.3018 (default fade), 0.3786 (`fade_in` 0.0005), 0.5000 (`fade_in` 0) | `a_hit_at_zero_is_not_cut` |
| `double` false (lock) | 1 onset, peak 0.500 | `double_false_is_the_press_alone` |
| 5 kinds, size 0 and 1, hardness 0 and 1, at 0 / 0.29 / 5 s | all finite, length 0.4 s, peak at most 0.5 | `finite_at_the_extremes` |
| 8 kHz document, lock, size 0 | renders, finite | same |
| two renders, 5 kinds, one thread vs the pool; same seed twice | same bits | `bit_identical_per_seed` |
| recipe above, 0.25 s | 12000 samples, peak -3.58 dB, 2 onsets, centroid 1830 Hz | `recipe_renders` |

### Honest limits

- None of this has been heard. The kinds are five hand-set mode tables, not
  recordings; whether a latch reads as a latch is the human's call. Use `size`,
  `hardness` and the per-kind choice as the knobs.
- The descriptor onset picker keeps onsets at least 30 ms apart, so the 24 ms
  `switch` double (and any `clasp` near the gap) reads as one onset there;
  `expect onsets` with `spacing_ms` (R1-C) resolves it.
- The excitation is a short half-sine pulse plus a decaying noise tick (at most
  2 % of the pulse area per sample), so the highest modes are weak at low
  `hardness`; that is the intended tilt, not a bug.
- `size` is a pitch and ring-time knob, not a physical length.
- The peak scale is applied after the render, so the level of the second
  transient relative to the press depends on `kind` and the seed's +-1.5 dB, not
  on `size` or `hardness`, except in the soft small lock case above.
