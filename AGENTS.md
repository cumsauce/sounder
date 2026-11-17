# AGENTS.md

sounder is a standalone procedural **sound-effects** tool driven by an **MCP
server on stdio**. A sound is a JSON graph of nodes rendered offline to WAV.
Agents build sounds over MCP; the human listens and comments in a window
(phase 5; until then the server is headless). Sibling of `modeler`: the same
shape (atomic edit batches, deterministic checks, an undo tree, a catalog,
comments the agent resolves), for audio. No game code.

```sh
cargo run -- --headless                 # the MCP server (stdio; exits on stdin EOF)
cargo run --example laser /tmp/sounder  # a laser zap: laser.wav + laser.png + descriptors
cargo test --workspace                  # graph, senses, session, and the driver script
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
```

This file is the long-form reference. The critical rules are also served in
the MCP `initialize` response (`instructions`); keep the two in sync.

**The honest limit.** You cannot hear. The senses (`summary`, `listen`,
`verify`) get you to a plausible sound: right length, level, pitch, spectral
shape, no clicks. Whether it is *good* is the human's call, made with comments
and with the knobs (macros) you give them. Do not claim a sound "sounds
warm"; say what you measured.

## Getting started (agent)

The MCP config runs `python3 mcp_driver.py --proxy` (one shared daemon for
every session; each session is a client with its own active sound), or the
binary directly: `target/debug/sounder --headless`. Example `.mcp.json`:

```json
{ "mcpServers": { "sounder": {
    "command": "python3",
    "args": ["/path/to/sounder/mcp_driver.py", "--proxy"] } } }
```

The driver is also a shell client (`python3 mcp_driver.py --calls calls.json`,
`--connect`, `--save-shot`), for when the session's tool list is frozen.
`SOUNDER_BINARY`, `SOUNDER_SOCKET`, `SOUNDER_PROJECT`, `SOUNDER_STDERR` mirror
modeler's `MODELER_*`.

1. `summary` — what is open, what the human asked for (open comments).
2. `new_sound` (or `sound_ops` with `sound: "name"`), then `sound_ops` to build.
3. `listen`, `verify`, fix, repeat.
4. Add 2-4 **macros** (knobs) so the human can fit the sound by ear.
5. `catalog save`, `export`.

## The tools (16)

| Tool | What it does |
|---|---|
| `summary` | state, nodes (what reads what), params, macros + claim status, the human's knob moves since last read (reading clears them), open comments, warnings, causal yes/no, descriptors |
| `sound_ops` | atomic batch: `ops` (node upserts / deletes), `params`, `macros`, `delete_macros`, `kind`, `duration`, `seed`, `sample_rate`, `expect`; renders for real; one history step if anything changed |
| `listen` | the image + descriptor table; `node`, `seeds`, `sweep`, `zoom`, `diff` |
| `verify` | PASS / FAIL checks; any FAIL is an error |
| `describe_op` | node types, one line each; or one type's example + schema |
| `extract_modes` | strongest partials of a WAV as a ready `resonators` node |
| `play` | window only: the headless error says so |
| `new_sound` | opens a sound beside the others; an open name is refused |
| `catalog` | `list describe save load delete close` |
| `export` | WAVs + `<name>.sfx.json` manifest |
| `undo redo jump history` | the history tree; one `sound_ops` call is one step |
| `add_annotation` | comment on a node / region, or `resolve` / `reopen` one |
| `session_log` | recent calls; `feedback` notes; `report` of the friction log |

Every tool that reads a sound takes `doc` (an open sound's name). Subagents of
one Claude session share one client id and one active sound: pass `doc` on
every call.

## The document

```json
{ "name": "laser", "kind": "oneshot", "sample_rate": 48000, "duration": null,
  "seed": 1, "nodes": [ ... ], "params": {}, "macros": [ ... ] }
```

- Time is seconds, frequency Hz, `*_db` fields dB, other gains linear.
- `kind` `oneshot` with no `duration` renders until the output stays under
  -80 dBFS for 50 ms (doubling 2 s up to 30 s; then a warning that it never
  decays), then trims the trailing silence. A `loop` needs a `duration`.
- `seed` drives every random node (mixed with the node's name and its own
  `seed`). Same document + seed = bit-identical samples at any thread count.
- Mono is the default; a node fed stereo that only takes mono errors and tells
  you to add a `mix` with `"to": "mono"`.

## Nodes

`describe_op` has the full list with examples. A node is `{"type", "name",
...fields}`; unknown fields are errors that list the node's fields. Signal
fields take a number, `"$param"`, `"=expr"` (`+ - * /`, parentheses, `$name`)
or the **name of another node** (`"freq": "sweep"`: audio-rate modulation, a
port as `"verb.wet"`).

Families (phase 1): sources `osc noise impulse sample`; envelopes `env adsr
decay follower`; filters `filter onepole comb allpass resonators`; time
`delay reverb convolve`; nonlinear `shape compress limit`; control `lfo
wander sample_hold slew ramp`; maths `math remap gain`; mix `mix pan width`;
offline `reverse normalize trim fade`; `output`.

Recipes:

- **Pitch sweep (laser, zap):** `ramp` 2000 -> 200 `curve: "exp"` into an
  `osc` `freq`, times a `decay`, through a `filter` whose `freq` is a scaled
  copy of the ramp (a `math` mul).
- **Struck object (knock, bell, click):** an `impulse` (or a short noise burst
  times a fast `decay`) into `resonators` `[[hz, decay_s, gain], ...]`.
  `extract_modes` starts from a real recording.
- **Texture (wind, hiss):** `noise` pink into a `filter` bp whose `freq` is a
  `wander` range; shape the level with another `wander`.
- **Loudness:** set `output.lufs`; it normalizes, then the ceiling limiter
  (-1 dBFS, lookahead) keeps peaks in. A very peaky sound cannot reach a high
  target; the render warns when it misses by more than 1 LU.
- **Edges:** the output fades 2 ms in and 5 ms out by default (0 for loops).

## Feedback loops

A cycle in the graph is legal only if it passes through a `delay` node. The
loop is then evaluated sample by sample (a delay's output at sample i depends
only on its input at earlier samples). A cycle with no delay is an error naming
the nodes ("nodes form a cycle without a delay: a -> c -> b -> a").

```json
[{"type": "noise", "name": "n"}, {"type": "decay", "name": "e", "time": 0.004},
 {"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"},
 {"type": "mix", "name": "m", "inputs": ["burst", "g"]},
 {"type": "delay", "name": "d", "input": "m", "time": 0.00227},
 {"type": "onepole", "name": "lp", "input": "d", "mode": "lp", "freq": 9000},
 {"type": "gain", "name": "g", "input": "lp", "gain": 0.997},
 {"type": "onepole", "name": "hp", "input": "g", "mode": "hp", "freq": 30}]
```

That is a Karplus-Strong string: the pitch is 1 / delay time (a little flat: the
filter adds a fraction of a sample to the loop; the delay `time` is the period).
The loop reads `burst` (outside it) per sample, so an outside `ramp` or `lfo`
can modulate anything in the loop.

- **Loop-capable nodes** (the rest error naming the node and listing these):
  `delay filter onepole comb allpass gain math mix shape remap slew follower
  expr`.
  Loops are mono; `mix` inside one takes no `pan` / `at` (do those after it).
- A loop's gain must stay under 1 or it blows up; the output limiter then
  holds it and `summary` warns the sound never decays. A burst's DC rings in a
  loop: block it with an `onepole` hp before the output (verify's `dc` says so).
- `delay` and `comb` still have their own `feedback` field for the simple case;
  the loop form is for filters, shapers or several delays in the path.
- Loops are cached and deterministic like everything else; changing a node in
  a loop re-runs the whole loop, not the nodes that only feed it.

## Offline pitch / time / spectral

Three acausal nodes (a phase vocoder: Hann window 2048 by default, 75 %
overlap, identity phase locking). They take mono or stereo (each channel on
its own), are deterministic, and **every output is exactly the document's
length**: a longer result is cut, a shorter one zero-padded.

- `pitch_shift` {input, `semitones` or `ratio`, [window]}: stretches by the
  ratio, then reads back at that rate, so the length is unchanged. Shifting up
  drops content above Nyquist / ratio. The ratio is rounded to a whole number
  of hop samples (a few cents of error at most).
- `time_stretch` {input, `factor`, [window]}: 2 = twice as long. The result is
  cut at the document length, so a stretched sound needs a longer `duration`
  (or auto length can't see it: set `duration`). Sharp transients smear over
  about one window; use a smaller `window` for clicks.
- `spectral` {input, `mode`, [window], [seed]}: `freeze` {at, [length]} holds
  the spectrum at `at` as a texture with seeded random phases from there (the
  input plays before `at`); `blur` {amount 0..1, a signal} smooths magnitudes
  over frequency and time; `gate` {threshold_db} zeroes bins quieter than a
  sine of that level; `morph` {other, amount 0..1, a signal} interpolates
  magnitudes from `input` (0) to `other` (1).

Recipe, a tail for a hit: `pitch_shift` the hit down 7 semitones, run it
through `time_stretch` factor 3 (set `duration` to fit), `gain` it -12 dB and
`mix` it under the dry hit with a small `at` offset.

## Events and grains

An **event stream** is a node output of kind `events`: a list of `{t, value,
id}` (seconds, a number, 0..n in time order), sorted, inside the document.
It is not audio: `mix`, `filter` and the like refuse it. Event nodes make
streams; `grains` turns one into sound now, and `voices` (rendering a sub-patch
per event) will after it lands. Every generator takes `values {min, max,
[curve]}` for a seeded per-event `value` (default 1); `curve` is `linear`
(uniform), `exp` (geometric, both ends > 0) or `smooth`. All take `[seed]`.

| Node | Fields |
|---|---|
| `once` | `[at]` |
| `pattern` | `times: [..]` or `every` + `count`; `[start]` added to all; `[jitter]` seconds, each time moves within +-jitter |
| `poisson` | `rate` events/s, a number or a signal (thinning, so the rate can vary) |
| `bounce` | `first` (seconds from the first impact to the second), `restitution` (0..1 exclusive), `[min_interval 0.004]`, `[start]`; gap k is `first * restitution^k`, impact k has value `restitution^k` |
| `burst` | `count`, `spread` (first to last event), `[curve]` timing density (linear even, exp dense early, smooth dense at both ends), `[at]` |
| `on_threshold` | `input` (mono audio), `level`, `[hold 0.01]`: an event at each rising crossing (interpolated time), value = the signal there; a signal that starts above `level` fires at 0 |
| `grains` | `source` (a node or a `.wav`), `events`, `size` s, `[position 0..1 or signal]` where each grain starts reading, `[pitch]` ratio, `[window hann\|tri\|rect]`, `[pan_spread 0..1]` (stereo, random pan per grain), `[jitter]` (+- of the source length on the position) |

`grains` places one windowed grain per event at the event time, scaled by the
event value; the output has exactly the document's length. Recipes: **rain** =
`poisson` (rate 40..200, `values` 0.2..1) -> `grains` of a short filtered
`noise` burst times a fast `decay`, `pan_spread` 0.6; **a bouncing ball** =
`bounce` -> `grains` of a short decaying `osc` thump (event value is the
impact strength), or later `voices`; **a crackle** = `poisson` with a
`wander` rate. `Cx::events` (in `node.rs`) is how a node reads a stream.

## expr: per-sample maths

```json
{"type": "expr", "name": "smooth", "inputs": {"a": "src"}, "state": {"y1": 0},
 "out": "y1", "update": {"y1": "y1 + 0.05 * (a - y1)"}}
```

- `out` is the sample; `update` assigns new `state` after each sample, every
  update reading the OLD state. `out` / `update` are plain expression text:
  no leading `=` or `$` (they are not param fields; node refs are only the
  `inputs` values).
- Names: `t` (seconds), `sr`, `pi`, the `inputs` (mono, sampled at this
  sample), the `state`. Functions: `sin cos tan tanh exp log pow sqrt abs min
  max floor fract clamp mix step noise(seed)`; operators `+ - * /`, `< <=
  > >= == !=`, `&& || !`, unary `-`, `cond ? a : b` (both sides evaluate).
  `noise(k)` is deterministic -1..1 hash noise per sample (document seed,
  node name, the node's `seed`, `k`).
- Compiled once to bytecode, no parsing per sample. A non-finite result
  (`log` of a negative, `x / 0`) becomes 0, in `out` and in state. Errors
  give the field, the expression, the position and the names in scope.
- The same functions, comparisons and `?:` work in `"=..."` field
  expressions over `$params` (`"=$hz > 200 ? $hz : 100"`).
- Wavefolder: `{"inputs": {"a": "tone"}, "out": "sin(a * 3 * pi / 2)"}`.
  Bouncing ball envelope (hits every `gap` s, each gap and level 0.7x the
  last): `state {"ph": 0, "gap": 0.3, "amp": 1}`, `out "amp * exp(0 - ph *
  40)"`, `update {"ph": "ph >= gap ? 0 : ph + 1 / sr", "gap": "ph >= gap ?
  max(gap * 0.7, 0.01) : gap", "amp": "ph >= gap ? amp * 0.7 : amp"}`.

## Voices and instances

- `voices` renders a sub-patch once per event and sums the results at the event
  times: rain, gravel, shards, fire crackle, gunfire. `{"type": "voices",
  "events": "drops", "length": 0.05, "max": 700, "map": {"hz": "1500 + $rand *
  5500"}, "patch": [ ...nodes... ]}`.
  - The `patch` is its own graph: it cannot read nodes outside it. Its sound
    is its `output` node if it has one, else its last node. Inside it `$value`
    (the event's value), `$index`, `$rand` (0..1, seeded per event) and `$t`
    (the event's time) are params, plus the ones `map` makes (plain expressions,
    no leading `=`) and the sound's own params. Voices are seeded per event and
    rendered in parallel; the sum is in event order, so the result is
    bit-identical at any thread count.
  - `length` is how long each voice renders (default 1 s): make it as short as
    the voice needs, the work is per voice. `max` (default 64) is an error when
    exceeded, not a silent cut: raise it or thin the stream. A voice whose sound
    is stereo makes the result stereo.
  - Recipes: `poisson` rate 120 -> short band-passed noise ticks = rain;
    `burst` 70 in 0.22 s `curve: "exp"` -> ticks of random pitch = footstep on
    gravel; `bounce` -> a ball's impacts. `examples/rain_loop.rs` and
    `examples/gravel_step.rs` are worked, checked versions.
- `sound` instances another document, open or saved in the catalog: `{"type":
  "sound", "sound": "thud", "at": 0.3, "gain_db": -6, "params": {...},
  "macros": {...}, "seed": 1}`. The instance keeps its own `output` stage
  (loudness target, ceiling), so layer by mixing instances with `mix` gains: a
  transient from one document, a body from another, a tail from a third. Editing
  the instanced sound re-renders every sound that instances it. Cycles (a
  instances b instances a) are an error naming the chain; nesting stops at 8.
  Only the server can reach other sounds: a bare `SoundDoc::render` needs a
  `RenderOpts.sounds` (`SoundSource`) for `sound` nodes.
- **Loops** get `loop_seam` (not `silent_edges`) in the default checks. The
  seam check compares the wrap sample with the loop's own roughness (a jump or a
  slope kink) and the third-octave band levels of the last and first 0.34 s. Noise
  loops pass easily; a smooth loop (a drone, an lfo) must be periodic in the
  duration (every lfo / oscillator frequency a multiple of 1 / duration).

## Macros: knobs for the human

You cannot hear, so give the human a front panel: a few knobs that move
several fields together, named for what they do. They turn them in the window
until it fits; you read their moves in `summary`.

```json
{"name": "brightness", "value": 0.5, "doc": "how cutting the zap is",
 "claim": ["centroid", 1], "game": false,
 "map": [{"node": "cutoff", "field": "b", "from": 1.2, "to": 7, "curve": "exp"},
         {"node": "lp", "field": "q", "from": 0.8, "to": 2.4}]}
```

- `value` is where it sits; `default` (the agent's setting, kept for reset)
  defaults to `value` the first time you set it.
- `map` entries write one field each over 0..1 (`curve` linear or exp; exp
  needs both ends above 0). `col` multiplies one column of a table field
  (resonator `modes`); `index` / `key` set one element of a list field (a `mix`
  input's `gain`). Mapped fields are overridden: the node's own value for them
  is ignored. Two macros may not write the same field.
- **The name is a claim.** `claim` is `[measure, direction]`: `centroid`
  (Hz), `decay` (ring-out), `crest` (dB), `lufs`, `flatness`, `swing`
  (loudness spread); `1` rises over 0..1, `-1` falls. `verify` (macros, in
  the defaults when a sound has any) sweeps each knob over 0, .25, .5, .75, 1
  with the others where they are and requires the measure to move that way at
  every step and no position to peak over -0.3 dBFS. A FAIL says to rename the
  macro to what it really does, or fix what it moves. Pick names that match:
  brightness -> centroid up, weight -> centroid down, length -> decay up,
  punch / snap -> crest up, grit / howl -> flatness, gustiness -> swing.
- `game: true` exports the knob in the manifest `inputs` (name, default, doc,
  map) for the game to set at runtime; others are baked at their value.
- The human's moves arrive in `summary` ("brightness 0.50 -> 0.72"). Keep
  them: re-issue the macro with `value` (and so `default`) set to where they
  left it, or re-centre its range. Moves are not history steps.
- `listen sweep: {param: "brightness", values: [0, .5, 1]}` tiles the knob.

## Senses

- `listen`: one PNG (log-frequency spectrogram with octave lines, waveform with
  peak + RMS envelopes, loudness curve; seeds / sweep values tile as labelled
  columns, 8 at most, under ~1.15 MP) and the descriptor table: level, LUFS,
  true peak, attack / decay, onsets, YIN pitch, spectral centroid / rolloff /
  flatness at start / middle / end, an 8 octave-band x 5 time-segment dB
  table (your main "what does it sound like"), modes, stereo.
  `diff: true` shows your previous render of the same request, this one, and
  their residual (this - previous). `zoom` magnifies an attack or a band.
  Reading the picture: x is time, y log frequency, colour dB from -90 (black)
  to 0.
- `verify` / `expect` checks: `peak_db` `true_peak_db` `lufs` `duration`
  `silent_edges` `clicks` `dc` `finite` `loop_seam` `bands` `pitch` `onsets`
  `variations` `rebuild` `macros`. Each line is PASS / FAIL with the measured
  value; a FAIL says what to change.

Warnings in `summary`: a node nothing reads, a frequency above Nyquist, a
silent output, DC over -40 dB, an output whose input is squashed by the
ceiling limiter, a sound that never decays, one that is mostly silence.

## Sessions, history, comments

- Several agents may share one server. Each keeps its own active sound; results
  say what others changed in yours since your last call; `undo` / `jump`
  refuse to revert another client's step unless `force: true`.
- History is a tree of full snapshots with a cursor; editing at an old step
  branches. `history` lists steps newest-last with who made each.
- Comments (annotations) are the human's requests, pinned to a node and / or a
  time / frequency region. `summary` lists the open ones in full. Deal with each,
  then `add_annotation {resolve: id, text: what you did}`. Resolve, never delete.
- Autosave: 0.5 s after the last edit into `<library>/session.json` (a second
  live server in the same library writes `session.<slot>.json`); `--restore`
  reopens it (the driver always passes it). `catalog save` is what persists a
  sound for other sessions.
- The library is `<project>/.sounder/` (a project is a folder with `.sounder/`,
  or a `Cargo.toml` beside `assets/`), else
  `~/Library/Application Support/friendslop/sounder/`;
  `FRIENDSLOP_DATA_DIR` overrides both (tests).
- `export` writes into `<project>/assets/sounds/` for relative paths:
  `<name>.wav` (or `<name>_<i>.wav` for `variations: N`, seeds 1..N) and
  `<name>.sfx.json`: files with seconds / samples / LUFS / peak, kind, loop
  points, game inputs, baked macros, seeds, causal flag, source catalog entry.

## Failures explain themselves

Errors name the node and field and say what to do next: `node #5 (filter
"lp"): field `frq`: unknown field `frq`, expected one of ...`, `macro `weight`
and macro `brightness` both write lp.q: split the field or drop one`. When one
does not, tell the maintainers: `session_log(feedback: "...")`. The friction
log (`<library>/feedback.jsonl`, never sent anywhere) also records every failed
call and slow call; `session_log(report: true)` or `sounder --feedback-report`
folds it into what to fix first.

## Not now

- A **live runtime** for the causal subset (every node here has a per-sample
  state form; `summary` reports whether a sound is fully causal). Firewheel and
  `bevy_seedling` are checked out for that later.
- Compressed export (ogg).
- A project **sound style** (loudness targets per category); project-level
  macros across several sounds ("grit" on all the footsteps) belong with it.
- Physical models, `reference` compare (phase 4); the window and `play`
  (phase 5).
