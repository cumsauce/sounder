# `trigger_env`, event_gate, seq, notes

Event streams (`pattern`, `poisson`, `bounce`, `burst`, `once`,
`on_threshold`) used to feed only `grains` and `voices`. These four nodes turn
a stream into control signals the rest of the graph can read: an envelope, a
gate, stepped values and a melody. All four are causal and pre-computed from
the whole stream; only `seq` in `random` mode reads the seed. Code:
`crates/sfx_graph/src/nodes/sequence.rs`, tests:
`crates/sfx_analysis/tests/it/sequence.rs`.

An event at time `t` first affects sample `ceil(t * sr)`, with 0.001 of a
sample of slack, so a `pattern every 0.1` event (0.1 stored as f32) lands on
sample 4800 at 48 kHz, not 4801.

## `trigger_env`

An envelope fired at each event of a stream (envelope family).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `events` | events | required | an event node | The stream that fires the envelope. |
| `attack` | number | `0.002` | seconds, >= 0 | Linear rise to the peak. 0 puts the peak on the event's first sample. |
| `decay` | number | `0.2` | seconds, > 0 | Fall from the peak to 0; exactly 0 after `attack + decay`. |
| `curve` | word | `exp` | `exp` `linear` `smooth` | Shape of the fall. `exp` is 60 dB down at the end, shifted and scaled so it ends at 0; `smooth` is a raised cosine. The attack is always linear. |
| `scale_by_value` | bool | `true` | | Peak = the event's `value` (false: 1). |
| `retrigger` | word | `restart` | `restart` `add` | `restart`: a new event replaces the running envelope, and its attack starts from the current level (no downward step). `add`: every event keeps its own envelope and they sum. |

Caps and errors (a number out of range is an error, nothing is clamped):

- `decay: 0: give a number > 0`, `attack: -1: give a number >= 0`
- `retrigger` `add` does events x min(attack + decay, document) samples of
  work; past 2e9: `150000 events of 10.002 s envelopes is about 14400000000
  samples of work (limit 2e9): shorten attack and decay, thin the events, or
  use retrigger "restart"`. `restart` is one pass over the document.
- Audio instead of events: `events: \`o\` is audio, not events: use on_threshold to make events from a signal`

### Recipe

A tom pattern: hits with random strength, each pitched by `seq` and shaped by
`trigger_env`. Give the human macros on the envelope `decay` and on the `seq`
`values` (the pitch).

```json
[
  {"type": "pattern", "name": "hits", "every": 0.125, "count": 8, "jitter": 0.004,
   "values": {"min": 0.5, "max": 1.0}, "seed": 2},
  {"type": "trigger_env", "name": "amp", "events": "hits", "attack": 0.001,
   "decay": 0.12, "curve": "exp"},
  {"type": "seq", "name": "pitch", "events": "hits", "values": [180, 150, 120, 150], "glide": 0.02},
  {"type": "osc", "name": "body", "freq": "pitch", "amp": 0.5},
  {"type": "math", "name": "tom", "op": "mul", "a": "body", "b": "amp"}
]
```

### Measured

Tests in `crates/sfx_analysis/tests/it/sequence.rs`, 48 kHz.

| What | Measured | Test |
|---|---|---|
| `pattern every 0.1` with random values 0.3..1.7, attack 0: peak position after each event, both retriggers | 0 samples (limit 1) | `trigger_env_peaks_at_each_event_with_its_value` |
| same: peak height minus event value | 0.0 (limit 1e-4) | same |
| linear, attack 0.01, decay 0.2: peak | 1.0000 at sample 5280 (event at 4800 + 480) | `trigger_env_attack_decay_and_curves` |
| same: halfway up, halfway down | 0.5000, 0.5000 | same |
| exp, decay 0.2: level halfway | -30.27 dB (expected -30.3) | same |
| exp: level after attack + decay | exactly 0 | same |
| smooth: halfway | 0.5000 | same |
| `scale_by_value` false, event values 3 | peak 1.0000 | same |
| `add`, linear, events 10 ms apart: level at the second event | 1.9500 (1 + 0.95) | `trigger_env_restart_continues_from_the_level_and_add_sums` |
| `restart`, attack 0.02: largest sample step near the second event | 0.00104 (no downward jump) | same |
| `restart`, attack 0: level at the second event | 1.0000 | same |

### Honest limits

- `restart` with attack 0 jumps to the new peak at once (a step if the old
  envelope was still high); give an attack to avoid it.
- Event values that are negative make a negative envelope.
- The shape in `exp` is a 60 dB fall, not a time constant: `decay` is the
  time to 0, not the time to -60 dB of a true exponential tail.
- Extreme values (attack and decay from 1e-9 to 1e9, values +-1e6, all three
  curves, both retriggers) stay finite (`sequence_nodes_are_finite_at_the_extremes`).
  None of this has been heard.

## `event_gate`

A 0 / 1 gate: 1 for `length` seconds after each event, else 0 (envelope
family).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `events` | events | required | an event node | The stream. |
| `length` | number | `0.05` | seconds, > 0 | How long the gate stays 1; at least one sample. Overlapping gates merge into one stretch. |

Error: `length: 0: give a number > 0`. The work is one pass over the document
whatever the overlap.

### Recipe

Open a filter for 30 ms after each hit: `event_gate` into the filter's
`freq` through a `math` (gate times 3000 plus 200). A macro on `length` gives
the human the open time.

```json
[
  {"type": "pattern", "name": "hits", "every": 0.25, "count": 4},
  {"type": "event_gate", "name": "open", "events": "hits", "length": 0.03}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| every 0.1 s, length 0.03 s, 3 events: high samples | 4320 (3 x 1440) | `event_gate_is_one_for_length_after_each_event` |
| same: first fall, second rise | sample 1440, sample 4800 | same |
| length 0.15 on a 0.1 s grid: high samples | 16800 (0.35 s) | same |
| length 1e-9: samples high | 1 per event | same |

### Honest limits

Only the values 0 and 1. A gate that starts at an event placed on a sample
boundary is high on that sample; there is no sub-sample edge.

## `seq`

Steps through a list of values on each event and holds between (control
family).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `events` | events | required | an event node | Each event is one step. The event's own value is ignored. |
| `values` | list of numbers | required | 1 .. 100000 finite numbers | What to step through. |
| `mode` | word | `cycle` | `cycle` `random` `once` | `cycle`: values[0], [1], ... round again. `random`: a seeded uniform pick per event (a value may repeat). `once`: through the list, then stay on the last. |
| `glide` | number | `0` | seconds, >= 0 | Linear move from the old value to the new one after an event. A new event mid-glide starts from the current value. |
| `seed` | integer | `0` | | Extra seed for `random`. Only `random` mode reads the seed (`random()` is false in the others). |

The first event plays values[0] in `cycle` and `once`; before the first event
the output is values[0]. Errors: `values: give at least one number`,
`values: <n> numbers (limit 100000): give fewer`.

### Recipe

See the tom recipe above (`pitch`). For a random arpeggio: `pattern every
0.12` into `seq` mode `random` values `[220, 277.18, 329.63, 440]`, into an
`osc` `freq`; a macro on `glide`.

### Measured

| What | Measured | Test |
|---|---|---|
| `[100, 200, 300]` over 7 events, cycle | 100 200 300 100 200 300 100 | `seq_cycles_runs_once_and_draws_seeded_randoms` |
| once | 100 200 300 300 300 300 300 | same |
| random, seed 1 twice / seed 2 | `[200, 100, 100, 100, 200, 300, 100]` twice; seed 2 `[100, 200, 100, 200, 300, 300, 200]` | same |
| glide 0.05, 100 to 200 at 0.1 s: halfway, after | 150.00, 200.00 | `seq_glides_from_the_current_value` |
| glide interrupted 20 ms in (0 to 100 over 50 ms, back to 0): level at the new event, 25 ms later | 40.00, 20.00 | same |

### Honest limits

`random` can repeat the same value on consecutive events. The glide is linear
in the value (for a pitch in Hz that is not linear in semitones; use `notes`
for that). Values +-1e30 with a 1e9 s glide stay finite.

## `notes`

A melody: pitches and lengths in beats, as a held pitch, a gate and a
note-on event stream (control family).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `notes` | list of `[pitch, beats]` | required | 1 .. 100000 entries | `pitch`: a note name (`C4 D#5 Bb3`, letter A-G, `#` or `b`, octave -1..9, A4 = `root`), or a number of semitones from `root`, or `"rest"` (also `"r"`). `beats` > 0. The melody starts at 0 s and plays once. |
| `bpm` | number | `120` | 1 .. 2000 | Beats per minute. |
| `root` | number | `440` | Hz, > 0 | Hz of A4 and of semitone 0. |
| `glide` | number | `0` | seconds, >= 0 | Glide from the previous pitch to the next, exponential in Hz (linear in semitones). |
| `gate` | number | `0.9` | > 0 .. 1 | Fraction of each note's length the gate stays high. 1 keeps it high across consecutive notes. |

Ports (read as `"name"` or `"name.port"`):

- `out` (the default): pitch in Hz, held after each note's start, and after
  the last note. A rest holds the previous pitch; a melody that starts with
  a rest holds the first pitched note's pitch from sample 0.
- `gate`: 0 / 1.
- `events`: one event per pitched note at its start time, `value` = its Hz,
  ids 0..n. Rests make no event; notes starting past the document are
  dropped. **Decision:** the card allowed dropping this port if `Outs` could
  not carry events; it can (`Outs` is `(port, Buf)` and `Buf` has an
  `Events` variant; the evaluator skips the length check for event buffers),
  so all three ports are emitted. Feed `events` to `trigger_env`, `seq`,
  `event_gate` or `grains`.

A note name is a word, not a node reference: the node declares `notes` as a
word field. Errors name the index: ``notes[0]: `H4`: the letter must be A to G;
give a note name like C4, D#5, Bb3, a number of semitones, or "rest"``,
`notes[0]: beats: 0: give a number > 0`, `bpm: 0: give a number from 1 to
2000`, `gate: 0: give a number above 0, up to 1`, `notes: only rests: give at
least one pitched note`, and a pitch at or above half the sample rate:
`notes[0]: gives 45774042.750 Hz, outside 0 to 24000 Hz (half the sample
rate): lower the pitch or \`root\``.

### Recipe

A plucked line: a saw `osc` on the pitch, a `trigger_env` on the `events`
port with the peak fixed at 1. Macros on `bpm`-independent fields: the
`glide` and the envelope `decay`.

```json
[
  {"type": "notes", "name": "tune", "bpm": 150, "glide": 0.03, "gate": 0.8,
   "notes": [["A3", 1], ["C4", 1], ["E4", 1], ["rest", 1], ["A4", 2]]},
  {"type": "osc", "name": "voice", "freq": "tune", "shape": "saw", "amp": 0.4},
  {"type": "event_gate", "name": "pluck_gate", "events": "tune.events", "length": 0.03},
  {"type": "trigger_env", "name": "pluck", "events": "tune.events", "attack": 0.003,
   "decay": 0.25, "scale_by_value": false},
  {"type": "math", "name": "line", "op": "mul", "a": "voice", "b": "pluck"}
]
```

### Measured

| What | Measured | Test |
|---|---|---|
| `[["A4", 1], ["C5", 1]]` at 120 bpm: pitch at 0.25 s, at 0.75 s | 440.0000 Hz, 523.2512 Hz (C5 = 523.2511; limit 0.1) | `notes_hold_pitches_and_gate_and_emit_events` |
| same: switch sample | 24000 (0.5 s) | same |
| same: gate 0.9 | 43200 high samples = 2 x 21600; first fall at 21600 | same |
| same: events port | (0.0 s, 440.0 Hz, id 0), (0.5 s, 523.25 Hz, id 1) | same |
| an `osc` driven by the pitch, YIN median over 0.05..0.45 s and 0.55..0.95 s | 440.00 Hz and 523.25 Hz | same |
| C4, D#5, Bb3, +12 semitones at 60 bpm | 261.6256, 622.2540, 233.0819, 880.0000 Hz | `notes_names_semitones_rests_root_and_glide` |
| root 432: A4, held through a rest, E5 | 432.000, 432.000, 647.269 Hz | same |
| gate over A4, rest, E5 at gate 0.9 | 86400 high samples = 2 x 43200 | same |
| glide 0.4 s, A4 to A5: halfway | 622.254 Hz (A4 x sqrt 2: 622.254), then 880.000 | same |
| events port read by `trigger_env` (`scale_by_value`, `add`) | 440.0 at 0 s, 880.0 at 0.5 s | `notes_events_drive_a_trigger_env` |
| melody recipe, 3 s | rms -28.77 dB; 4 gate pulses x 1440 samples | `sequence_recipes_render` |

### Honest limits

- One pass: no repeat or loop field; use a `pattern` and `sound` instances,
  or a longer list.
- Pitch changes happen at the note's start sample (rounded), so a tempo that
  puts a note between samples is off by at most half a sample.
- Gate 1 with consecutive notes gives no edge between them; use the `events`
  port to re-articulate an envelope.
- An extreme list (2000 bpm, 1e-6 beat notes, a 1e9 s glide, octaves -1 and
  9) stays finite.
- Tuning is equal temperament from `root` only; no scales, no microtones
  except by fractional semitones. None of this has been heard.
