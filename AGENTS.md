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
- Feedback loops through `delay`, events and `voices`, `expr`, `sound`
  instancing, offline pitch / time (phase 3); physical models, `reference`
  compare (phase 4); the window and `play` (phase 5).
