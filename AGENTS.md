# AGENTS.md

sounder is a standalone procedural **sound-effects** tool driven by an **MCP
server on stdio**. A sound is a JSON graph of nodes rendered offline to WAV.
Agents build sounds over MCP; the human listens and comments in a window
(`--headless` runs without it). Sibling of `modeler`: the same shape (atomic
edit batches, deterministic checks, an undo tree, a catalog, comments the
agent resolves), for audio. No game code.

```sh
cargo run -- --headless                 # the MCP server (stdio; exits on stdin EOF)
cargo run --example laser /tmp/sounder  # a laser zap: laser.wav + laser.png + descriptors
cargo test --workspace                  # graph, senses, session, and the driver script
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
```

**The honest limit.** You cannot hear. The senses (`summary`, `listen`,
`verify`) get you to a plausible sound: right length, level, pitch, spectral
shape, no clicks. Whether it is *good* is the human's call, made with comments
and with the knobs (macros) you give them. Never claim a sound "sounds warm";
say what you measured.

**How this file is laid out.** Part 1 is for agents making sounds: setup, the
workflow, the rules that catch everyone, then reference in the order you need
it. Part 2 is for changing the tool itself. Per-node detail lives in `docs/`
(see "More reference"). The critical rules are also served in the MCP
`initialize` response (`instructions`, in `src/bin/sounder/server/tools.rs`);
keep the two in sync.

# Part 1: Making sounds

## Getting started (agent)

The MCP config runs `python3 mcp_driver.py --proxy`: one shared daemon for
every session, each session a client with its own active sound, and the
daemon opens the window so the human sees every agent's sounds in one place
(`--proxy --headless` keeps it off).

```json
{ "mcpServers": { "sounder": {
    "command": "python3",
    "args": ["/path/to/sounder/mcp_driver.py", "--proxy"] } } }
```

- **One window per user.** A windowed `sounder` takes
  `$TMPDIR/sounder-window.lock`; any further windowed launch logs "the window
  is already open" and runs headless. Point every agent at `--proxy`.
- **No MCP tools in your session?** The driver is also a shell client:
  `python3 mcp_driver.py --calls calls.json` (also `--connect`,
  `--save-shot`). Consecutive `--calls` runs share one autosave, so a sound
  made in one run is open in the next.
- Env: `SOUNDER_BINARY`, `SOUNDER_SOCKET`, `SOUNDER_PROJECT`, `SOUNDER_STDERR`
  (mirror modeler's `MODELER_*`). The binary directly:
  `target/debug/sounder --headless`.

### The workflow

1. **`summary`**: what is open, and what the human asked for (open comments
   are requests).
2. **`new_sound {name, brief: "<the brief, word for word>"}`**: it lists the
   approved parts, templates and ready-made nodes that fit, best first.
   **When the top one fits, start from it** (a `sound` node, or `catalog
   load` to edit a copy) instead of rebuilding from nodes. See "Which
   ready-made node for which sound".
3. **Write the brief as a spec** (`sound_ops` `spec`, see "The brief as
   data"): one component per layer the brief asks for, the node named once it
   exists.
4. **Build with `sound_ops`.** For an impact, a hit or a mechanism, read
   "Layering a designed sound": the ready-made node is the body layer, and a
   designed sound adds pre, crack, weight and after layers on a dry bus.
   `describe_op` lists the 154 node types; `describe_op {kind: "filter"}`
   gives one's example and schema.
5. **`listen`, `verify`, fix, repeat.**
6. **Add macros (knobs)**, unasked, on every sound: usually 1-4, on the 1-2
   fields that dominate the character (size / pitch, brightness, length,
   density, intensity). The brief's values are the default and the range
   ends stay near them: a knob must not turn the sound into a different one.
   `summary` reports "no knobs yet" until there is one. See "Macros: knobs
   for the human".
7. **`catalog save`, `export`.**

### Rules that catch everyone

Each of these has cost an agent a retry or a wrong sound. The section named
after each holds the detail.

- **An upsert replaces the whole node.** A field left out of a re-issued node
  goes back to its default; send every field you keep (`summary` lists them).
- **Remove a node inside `ops`**: `{"type": "delete", "name": "x"}`, one item
  per node. A top-level `delete` is not a field and errors.
- **Pass `doc` on every call** when you share a session (subagents of one
  Claude session share one client id and one active sound).
- **A hit at t = 0 needs `fade_in: 0.0005`** on the output (or the hit
  starts at `at: 0.005`): the default 2 ms fade-in cuts it. This includes a
  `pattern` or `bounce` whose first event is at 0. ("The output stage")
- **Level: `peak_db` for short sounds, `lufs` for sustained ones.** A click
  or hit cannot reach a high `lufs` under the ceiling. ("The output stage")
- **Give a `duration`** to a `loop`, to a part used as the whole sound (its
  own `duration`, from `catalog describe`), to a sound whose level follows a
  `ramp` (it holds its last value), and to offline or long-tailed nodes
  (`time_stretch`, `pitch_shift`, `spectral`, `smear`, a long `reverb`,
  `thunder`), or they are cut. `duration: 0` returns to automatic length.
- **Dry and mono by default**; no `reverb` unless the brief names a space.
  A sound for a video has no engine to add the room: put `camera` before
  `output` (`docs/nodes/camera.md`).
- **`modal` needs `noise`** (wood 0.6; metal 0.4 with `noise_width` 0.1 and a
  low `loss`) or it reads as a tuned instrument. Impacts are atonal.
- **An event stream is not audio**: `pattern poisson bounce burst once
  on_threshold shatter collapse` become sound through `voices`, `grains` or `debris`, control
  through `trigger_env`, `event_gate` or `seq`.
- **A cycle is legal only through a `delay`**, with loop gain under 1.
- **Sounds are sample-free**: recordings are references, never layers.
- **A `seed` changes only nodes that read it**; four variations of a chain
  with no random node are four identical files. ("Variation")
- **An `output.lufs` target renormalizes**, so a loudness knob claiming
  `lufs` cannot move. ("Macros: knobs for the human")
- **Every tool rejects an argument it does not take** and lists the ones it
  does; an unknown node field errors with the node's fields listed. Check
  `describe_op` before guessing a field.

## The tools (16)

| Tool | What it does |
|---|---|
| `summary` | state, nodes (what reads what), params, macros + claim status, the human's knob moves since last read (reading clears them), open comments, warnings, causal yes/no, descriptors |
| `sound_ops` | atomic batch: `ops` (node upserts; `{"type": "delete", "name": ...}` removes), `params`, `macros`, `delete_macros`, `kind`, `duration` (0 = automatic length again; null changes nothing), `seed`, `sample_rate`, `expect`, `spec` / `spec_update` (the brief as data; `spec: {}` removes it); renders for real; one history step if anything changed |
| `listen` | the image + descriptor table; `node`, `seeds`, `sweep {param, values: [..]}`, `zoom {t: [a, b], hz: [lo, hi]}`, `diff`, `reference`, `reference_set` (a bad `zoom` / `sweep` error shows the accepted shape) |
| `verify` | PASS / FAIL checks (`checks`, `only`, `expect`, `seeds`; see "Senses"); any FAIL is an error |
| `describe_op` | node types, one line each; or one type's example + schema (`kind`, aliases `type` / `node`) and the built-in parts built on it |
| `extract_modes` | strongest partials of a WAV as a ready `resonators` node; `residual: "x.wav"` also writes what they miss |
| `play` | plays on the speakers for the human (the window, or `--headless --audio`); a plain headless server answers with an error saying so |
| `new_sound` | opens a sound beside the others; `kind`, `duration` (0..600 s), `seed` at once (a loop is one call); `template` (`door_slam`); `brief` lists the parts, templates and nodes that fit; an open name is refused |
| `catalog` | `list search sources describe save load delete close`; `search` by `text` / `role` / `material` / `approved` (`text` alone also matches templates and ready-made nodes); `sources` lists the WAVs in the sources folders; `save` with `role` / `material` makes a part; the sound is `name` or `doc` |
| `export` | Ogg Vorbis files (default; `format: "wav"` for WAV) + `<name>.sfx.json` manifest |
| `undo redo jump history` | the history tree; one `sound_ops` call is one step |
| `add_annotation` | comment on a node / region, or `resolve` / `reopen` one |
| `session_log` | recent calls; `feedback` notes; `report` of the friction log |

Every tool that reads a sound takes `doc` (an open sound's name).

## The document

```json
{ "name": "laser", "kind": "oneshot", "sample_rate": 48000, "duration": null,
  "seed": 1, "nodes": [ ... ], "params": {}, "macros": [ ... ] }
```

- Units: time in seconds, frequency in Hz, `*_db` fields in dB, other gains
  linear.
- **Length.** A `oneshot` with no `duration` renders until the output stays
  under -80 dBFS or 60 dB under its own peak (the higher) for 50 ms
  (doubling from 2 s up to 30 s, then a warning that it never decays), then
  trims the trailing silence. A `loop` needs a `duration`. In `sound_ops` a
  number fixes the length, `0` returns to automatic, leaving it out (or
  `null`) changes nothing.
- **Determinism.** `seed` drives every random node (mixed with the node's
  name and its own `seed`). Same document + seed = bit-identical samples at
  any thread count.
- **Channels.** Mono is the default; a node fed stereo that only takes mono
  errors and tells you to add a `mix` with `"to": "mono"`. Node names `mono`
  and `stereo` are reserved.
- **Aliases accepted** (names agents guess): filter / onepole `mode`
  `lowpass` `highpass` `bandpass`; osc `wave` (= `shape`); reverb `wet`
  (= `mix`); slew `rise` / `fall` (= `up` / `down`); crossfade_loop `time`
  (= `fade`); trigger_env `input` (= `events`); `gain` `db` / `gain_db`;
  `ramp` `time` / `duration`. A tool's `duration`, `seed` or `sample_rate`
  given as a string (`"2.0"`) is read as the number.

**Fields that trip people:**

| Node | Watch out |
|---|---|
| `pattern` | starts at `start`, not `at`; `accel` 0.7 = last gap 0.7 x the first (a speed-up) |
| `impulse` | `at` is a number or a list of times (`[0, 0.1, 0.2]`), default 0 |
| `wander` | `rate` plus `range: [lo, hi]` **or** `min` and `max` (not both; no range is -1..1) |
| `ramp` | `exp` curve by default (`"curve": "linear"` for straight); holds its last value |
| `decay` | `time` is seconds to fall **60 dB** (exp), not a time constant: `time` 0.3 reads ~0.1 s at -20 dB in `listen`; it is an envelope with no `input` |
| `gain` | `db` or `gain_db` (both is an error), or a linear `gain` |
| `mix` | input `gain` is linear, not dB (see "`mix` inputs") |
| any node | `at` delays the start (silent before it) |

### The output stage

Exactly one `output` node: `{input, [gain_db], [lufs] or [peak_db],
[fade_in 0.002], [fade_out 0.005], [ceiling_db -1]}` (fades are 0 in a loop).
The chain: gain, the loudness target, a lookahead ceiling limiter holding the
**true peak** (4x oversampled), then the edge fades. **The target is met
after the fades**: a hit at t = 0 with `fade_in: 0.0005` and `peak_db: -1.5`
is delivered at -1.5 dBFS (+-0.1).

- **`peak_db`** (clicks, ticks, short hits, impacts) normalizes the
  pre-limiter sample peak. A noisy hit whose inter-sample peaks run over the
  ceiling is turned down, so its delivered sample peak can sit below
  `peak_db` (its true peak is at the ceiling; that counts as met).
- **`lufs`** (sustained sounds) normalizes integrated loudness (from as low
  as -70 LUFS). A short or peaky sound cannot reach a high `lufs`. A miss of
  more than 1 LU warns with the numbers and the cause: "too short or too
  sparse: use peak_db" (crest 20 dB or more), "the limiter took N dB off the
  peak" (compress or shape before the output, lower the target, or raise
  `ceiling_db`), or "the edge fades took N LU" (shorten them).
- `lufs` with `peak_db` is an error, as is `peak_db` above `ceiling_db`;
  `gain_db` stacks on either. A sound whose true peak is under the ceiling
  passes bit-identical.
- **The default 2 ms fade-in cuts a hit that starts at t = 0.** The render
  warns ("the default 2 ms fade-in is cutting the first hit by N dB") unless
  `fade_in` is 1 ms or less or a met target already delivers the right level.
  Fix: `fade_in: 0.0005` (the fade reaches zero at sample 0, so
  `silent_edges` passes; `fade_in: 0` fails `silent_edges`), or start the hit
  at `at: 0.005` as `explosion`, `gunshot` and `splash` do.
- The limiter-squash warning does not fire when a `peak_db` target is met;
  with `gain_db` pushing a `peak_db` target over the ceiling it does.

### `mix` inputs

`inputs` items are a node name or an object `{"input": "node", "gain": 0.5,
"pan": -0.3, "at": 0.1}`: `gain` is linear (a number or a node, so a `decay`
envelope works), `pan` -1..1 makes the mix stereo, `at` delays that input (a
tail after a transient). `to: "mono" | "stereo"` forces the width. For dB, put
a `gain` node (`db`) before the mix. A macro moves one input's level with
`{"node": "scene", "field": "inputs", "index": 1, "key": "gain", "from": 0.1,
"to": 0.9}`. Inside a feedback loop a `mix` takes no `pan` / `at`. Prefer the
object form to extra `gain` nodes.

## Nodes

A node is `{"type", "name", ...fields}`. **Signal fields** take a number,
`"$param"`, `"=expr"` (`+ - * /`, parentheses, `$name`) or the **name of
another node** (`"freq": "sweep"`: audio-rate modulation; a port as
`"verb.wet"`). A string with `$` or number arithmetic (`"1.5 * $value"`,
`"2*$rand"`) is an expression even without the `=`; a string that is a valid
node or port name (`"verb.wet"`, `"a_b"`) is always a node.

Families (154 types; * = a section in `docs/nodes/README.md`):

- **sources** `osc noise impulse sample`, `fm wavetable`*; **control** `lfo
  wander sample_hold slew ramp`, `chaos fbm curve switch pitch_follow
  centroid_follow`*; **envelopes** `env adsr decay follower`, `trigger_env
  event_gate seq notes`*; **maths** `math remap gain`, `expr`.
- **filters** `filter onepole comb allpass resonators`, `ladder eq
  filterbank vocoder multiband`*; **time** `delay convolve`, `reverb`
  (models)*, `ir`*; **nonlinear** `shape compress limit`, `ringmod transient
  gate sub exciter`*; **the bus** `finish`*, `camera`* (video: distance, air, space); **exaggeration** `subdrop
  riser`*; **modulation** `chorus flanger phaser freq_shift`*; **lo-fi and
  loops** `crossfade_loop resample tape stutter`*.
- **mix** `mix pan width`, `haas decorrelate doppler`*; **offline** `reverse
  normalize trim fade`, spectral `pitch_shift time_stretch spectral`*.
- **events** `once pattern poisson bounce burst on_threshold shatter
  collapse cascade fracture_on contacts`*, with `grains cloud debris grind
  dust`* and `voices`; `sound` (an instance of another sound).
- **physical models** `modal contact friction impact rig`*, mechanism
  `click`*, foley `bubble air glottis tract syrinx thunder`*, waveguides
  `tube string`*, surfaces `rolling cloth crumple footstep`*, water `water
  splash`*, elements `fire wind spark`*, machines `engine motor clock fan`*,
  from Farnell's book `bell ball_bounce cricket twang siren door_creak jet
  red_alert`*, blasts `explosion gunshot`*; and `output`.

Graph rules:

- `voices` renders its `patch` once per event (`$value`, `$index`, `$rand`,
  `$t`; exceeding `max` is an error); `sound` instances another document.
- Loops are checked with `loop_seam`: put `crossfade_loop` last, after any
  filter. `fire`, `wind`, `spark`, `water` render periodic in a `kind: loop`
  document, but run `verify {seeds: 4}` and add `crossfade_loop` if a seed
  fails (a `fire` loop failed one of four).
- `modal` takes `size` or `f0` (not both) and is normalized.
- `pitch_shift`, `time_stretch`, `spectral` are offline: the output is
  exactly the document's length.

Recipes:

- **Pitch sweep (laser, zap):** `ramp` 2000 -> 200 `curve: "exp"` into an
  `osc` `freq`, times a `decay`, through a `filter` whose `freq` is a scaled
  copy of the ramp (a `math` mul).
- **Struck object (knock, bell):** an `impulse` (or a short noise burst times
  a fast `decay`) into `resonators` `[[hz, decay_s, gain], ...]`, or a
  `contact` into `modal` (pick a `shape`). A latch or switch is `click`.
  `extract_modes` starts from a real recording.
- **Texture (wind, hiss):** `wind`, `fire`, `water` in one node; or pink
  `noise` into a `filter` bp whose `freq` is a `wander`, the level shaped by
  another `wander`.
- **Machine / vehicle:** `ramp` or `wander` into `engine` / `motor` `rpm`.
- **Impact, break, step:** `step`, `explosion`, `gunshot`, `splash`, or
  `shatter` -> `voices` of a small `modal` per fragment.
- **Thick or moving:** `osc` `unison`; `chorus` / `phaser`; `doppler` for a
  fly-by; `reverb` `model` or `convolve` an `ir`.

### Variation

A `seed` changes only what reads it: `noise`, `wander`, `poisson`, `voices`
(`$rand`), `grains`, `cloud`, `shatter`, `pattern` with `jitter` or `values`,
`bubble` and `bounce` with `jitter`, and the seeded models (`footstep`,
`click`, `fire`, `wind`, `thunder`, ...). A chain of `osc`, `filter`, `ramp`,
`decay`, an unjittered `pattern` / `bubble` / `bounce`, or a `modal` fed by a
plain `impulse` renders the same bits for every seed: `export variations: N`
then warns "all N variations are bit-identical" and `verify expect
variations` fails. For different takes:

- jitter the timing: `pattern` `jitter` (seconds), `bounce` `jitter` (a
  fraction of each gap, 0..0.5), `poisson` for a stream;
- randomize the voice: `voices` with a `map` of `$rand` expressions (pitch,
  `modal` `size`, level), `bubble` `jitter` (0..0.9), `grains` `jitter` or
  per-grain ranges.

<!-- recipe -->
```json
[
  {"type": "pattern", "name": "taps", "every": 0.25, "count": 6, "jitter": 0.04,
   "values": {"min": 0.4, "max": 1}, "seed": 3},
  {"type": "voices", "name": "knocks", "events": "taps", "length": 0.5, "max": 16,
   "map": {"size": "0.2 + $rand * 0.3"},
   "patch": [
     {"type": "impulse", "name": "tap", "at": [0.0]},
     {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
     {"type": "modal", "name": "panel", "excite": "strike", "material": "wood", "size": "$size", "loss": 1.5}]},
  {"type": "footstep", "name": "step", "surface": "wood", "weight": 0.6, "pace": "walk"},
  {"type": "mix", "name": "scene", "inputs": [
    {"input": "knocks", "gain": 0.6},
    {"input": "step", "gain": 0.9, "pan": -0.3, "at": 1.0}]}
]
```

## Which ready-made node for which sound

**Search first, by the sound you need.** `new_sound {name, brief}` lists the
parts, templates and nodes that fit; `catalog {action: "search", text: "<the
sound>"}` (plus `role` / `material`) finds the 133 built-in parts by what the
listener heard. Agents who matched their brief against a table row instead
missed `impact_sign_wind` and `impact_bowed_bar`. Then this short table; the
full one (every ready-made node, its knobs, how it was heard, the listener's
notes on knocks, metal, stone) is `docs/routing.md`.

| Brief | Start from |
|---|---|
| a door slam | `new_sound {template: "door_slam"}` (knobs `weight snap stick knock latch`); never a click over one `modal` |
| footsteps | `step` or its parts (`all_purpose_step`, `concrete_step`, `gravel_step`, ...); not `footstep`, not an impact |
| a dropped / bouncing / rattling / scraped / bowed object | the `impact_*` parts (`impact_pipe_drop`, `impact_coin_drop`, `impact_block_drop`, `impact_lid_rattle`, `impact_sign_wind`, `impact_bowed_bar`, `impact_box_scrape`, `impact_bending_sheet`) |
| objects acting on each other | the `rig_*` parts (`rig_ball_on_drum`, `rig_pan_and_lid`, `rig_tin_roof_screw`, `rig_string_on_box`, `rig_wind_chime`, `rig_chair_fall`, `rig_hand_bell`) |
| none of those fits, but objects move each other | `impact` (one striker or the floor on one object: `height` / `speed`, `drive`, `slide` + `friction`) or `rig` (several bodies, `contact` / `spring` joints; a spring `gap` rattles); `docs/nodes/modal.md`, `docs/nodes/rig.md` |
| a knock | `pattern` -> `voices` of `contact` -> wood `modal` (`noise` 0.6, `tilt` 4, `loss` ~3) + crack + thud (`docs/routing.md`) |
| metal: pipe, bar, lid, coin | `modal` metal, `noise` 0.4, `noise_width` 0.1, `loss` 0.3-0.6, lowest mode 120-200 Hz for a pipe |
| stone, brick | noise layers + a quiet stone `modal` (`docs/routing.md`, "From the round 6 listen") |
| a latch, switch, button, lock | `click` (`noise` ~0.75; `fade_in` 0.0005) |
| fire, wind, rain, water, a splash, sparks | `fire`, `wind`, `rain`, `water`, `splash`, `spark` |
| an explosion, a gunshot | `explosion`, `gunshot` |
| an engine, a motor | `engine`, `motor` with a `ramp` / `wander` on `rpm` and a `duration` |
| a creature | `creature` or the `creature_*` parts |
| a whoosh | the `whoosh_sweep` part (wide band, q ~1) |
| breaking glass | `shatter` -> `voices` of a small `modal` |
| many things falling at once (a wall of bricks, a rockfall) | `collapse` -> `debris` (shared bodies, physical timing), a few `collapse.hero` hits through `voices` of a `rig` (`docs/nodes/collapse.md`); not heard yet |
| a scene from a physics simulation | `contacts` (a CSV / JSON contact log, any engine) -> `debris` / `voices` (`$size $pan $distance $material $kind`) |
| a building or hillside coming down | `cascade` -> `debris` with `scale` |
| hard hits breaking things, dust after | `fracture_on` -> chip events; `dust` |
| a pile still sliding, one object rolling | `grind` |
| a sound for a video (not a game) | the layers dry, then `camera` (`distance`, `space`) before `output`; `fps` + frame times (`docs/timeline.md`) |
| a laser, UI blip, drone | primitives (recipes in "Nodes") |
| a cinematic hit | add `subdrop` / `riser` only when the brief asks |

**Not in the library** (what was built was heard as something else): a ball
rolling, steps in snow or on ice, a stream, a distant large splash. Build
from nodes and say they are unheard.

**Layer levels:** ready-made nodes are not normalized (a `fire` about -30
LUFS, a `footstep` peaks near -12 dBFS, a `click` -6 dBFS). Balance with a
`gain` (`db`) per layer **before** the `mix`, then one `peak_db` / `lufs` on
the output, and one level macro per layer. `contact` and `modal` take their
own `gain_db`.

### Parts: assemble from layers the human has heard

A **part** is a small saved sound that fills one role (`pre crack body weight
after bed texture whole`), with a material, tags and one to three knobs.

- **Find one:** `catalog {action: "search", role: "body", material: "wood"}`
  (also `text`: any word of a name, tag or description, endings folded;
  `approved`). It searches the library's parts and saved sounds plus the 133
  built-in parts in `parts/` (by use: `docs/routing.md`, "Parts the listener
  picked"). Each is described by what the listener heard (`heard as
  "..."`), often not what it was built as. Approved ones come first.
  `catalog describe` shows a part's nodes, knobs and `duration`.
- **Use one** with a `sound` node: `{"type": "sound", "name": "body",
  "sound": "wood_panel_body", "at": 0.045, "gain_db": 0, "macros": {"size":
  0.6}}`. A hit part strikes 5 ms after its start (`wood_brush` ends at 50
  ms, so the hit goes at `at: 0.045`). Parts carry no loudness target: move
  them with `gain_db`, then `mix`, `finish`, `output` as for any layered
  sound.
- **A part as the whole sound** (role `whole`, most `impact` / `rig` parts):
  one `sound` node into `output` with `peak_db`, and the document gets the
  part's `duration` (with automatic length a struck string part came out
  0.87 s instead of its heard 2.5 s). A part that starts with a fall can go
  through `trim`.
- **Drive a part's knob** from the outer sound: `{"node": "body", "field":
  "macros", "key": "size", "from": 0.2, "to": 0.9}`. Leave such knobs
  unclaimed (they move unevenly under a `peak_db` output).
- `catalog load` a part to edit a copy; a library sound with a built-in's
  name is used instead of the built-in.
- **Save a layer** with `catalog {action: "save", role: "crack", material:
  "metal", description: ..., tags: [...]}`: no `lufs` / `peak_db` target,
  its event at 5 ms.
- **Approval is the human's.** Only the window sets `approved`; saving a
  changed document clears it and the result says so. Say so when you use an
  unapproved part (a draft).

The five `door_slam` layer parts were each approved alone (2026-11-18) and
pass `layers` reassembled (`tests/parts.rs`).

## Layering a designed sound

A finished game or film SFX is several layers, each owning a band and a time
slot, through one dry bus. One node gets a correct sound; layers get a
designed one: a layered door slam sat closer to real slams than they sit to
each other, where the single-chain one did not (`docs/history/phase0-results.md`).

| Role | Band | When | Made from |
|---|---|---|---|
| **pre** (anticipation) | 1.5-5 kHz | 5-50 ms before the hit | `friction` + noise under an `env`: a door brushing its frame, a blade sliding |
| **crack** (transient) | 2-6 kHz | the first 5-25 ms | white noise x a fast `decay` (`time` 0.008-0.03) through a `filter` bp or hp |
| **body** (the object) | 150 Hz-2 kHz | 50-500 ms | `modal` with the right `material`, struck by a **noise burst**; or a ready-made node |
| **weight** | 60-350 Hz | 100-500 ms | band-limited noise: brown noise x `decay` through a `filter` lp (`slope` 24), pink noise through a wide bp |
| **after** (settle, debris, ring) | any | after the body | a second small hit (`contact` -> small `modal`), `voices`, or the body's own ring |

Rules (each a fault the listener heard):

- **Dry and mono by default.** No `reverb` unless the brief names a space:
  the game puts the sound in its room. A tail is the object's own ring or an
  after-layer.
- **Impacts are atonal.** A sine sub drop plus a few clean `modal` modes read
  as "a tuned drum". Build weight from band-limited noise, excite `modal`
  with a noise burst (not a lone `impulse` or `contact`), keep `loss` at 1.2
  or more for wood. A wooden impact should read `voiced` near 0 in `listen`
  (the real slam 0.15; the tuned-drum version 0.9).
- **Keep materials apart.** `click` (resonators 1.5-8 kHz) and a hard
  `crumple` (hardness 0.9) read as "metal pieces clinking": keep them out of
  wood, cloth and flesh. A wooden second hit is a small `contact` -> wood
  `modal`, low-passed.
- **Add the detail before the hit** where the object would make it (a door
  sticking on its frame, a hand on a handle, a blade sliding): it makes a hit
  read as a real event.

**The bus:** `mix` the layers mono, then **`finish`** (`punch` 0..1,
`tail_db`, `glue` 0..1, `warmth` 0..1, `weight_db` / `air_db` shelves at 120
Hz / 6 kHz, `width` above 150 Hz for a stereo mix; no reverb), then
`output`. On the door slam `{"type": "finish", "input": "dry", "punch": 0.4,
"tail_db": -6, "glue": 0.3, "warmth": 0.6, "weight_db": 4}` matched the
hand-built bus (`transient` + `shape tube` + an `onepole` hp; details and
measurements: `docs/nodes/finish.md`).

- Balance with a `gain` (`db`) per layer before the `mix`. Knobs: one per
  layer level (pre, crack, weight, after), one on the body (`size` or
  `loss`); on `finish`, `glue` (claim `crest`, -1), `punch` (claim `crest`,
  1; leave the claim off on noisy layers), `tail_db`, `weight_db`.
- Targets for a heavy wooden impact (the real slam): crest about 16 dB,
  energy under 200 Hz / 200 Hz-2 kHz / 2-8 kHz near 38 / 56 / 5.5 %, -20 dB
  about 200 ms after the hit.
- Check with `verify {checks: ["layers"]}` (the `impact` preset: energy by
  role, voiced, ringing partials, crest, no reverb): every FAIL names the
  layer to add.

### The brief as data

A `spec` on the sound lists the components the brief asks for, so the graph
is checked against the request, not only against "plausible":

```json
{"brief": "a heavy wooden door slammed shut", "components": [
  {"role": "pre", "node": "scr_g", "at": [0, 0.05], "note": "the door brushing its frame"},
  {"role": "crack", "node": "crk_g", "at": [0.05, 0.08]},
  {"role": "body", "node": "panel", "material": "wood", "at": [0.05, 0.5]},
  {"role": "body", "node": "body_g", "at": [0.05, 0.6], "length_ms": [50, 250]},
  {"role": "weight", "node": "thd_g", "at": [0.05, 0.5], "length_ms": [40, 200]},
  {"role": "after", "material": "wood", "note": "a second small knock"}]}
```

| Field | Checked |
|---|---|
| `role` | a part role (`pre crack body weight after bed texture whole`) |
| `node` | the layer's last node before the mix; it must reach the output (left out: not built yet) |
| `material` | against every `material` field the node reads; leave it off a part's component (a `sound` node has none: "no material field to check") |
| `at` [start, end] s | holds 80 % of the layer's energy (10 ms margin) |
| `band` [lo, hi] Hz | holds half of it; default by role: pre 1-8 kHz, crack 1.5-16 kHz, body 100-3000 Hz, weight 20-400 Hz |
| `length_ms` [min, max] | the span holding the middle 80 % of its energy |

- A modal body is mostly its strike (the approved slam's noise-struck panel
  lasts 5 ms; its noise body 94 ms, its weight 73 ms): a hit's length comes
  from a noise body and the weight, so a modal-only body fails a `length_ms`
  of [40, 300].
- A sound with no nodes keeps a spec-only `sound_ops` call unrendered, so the
  spec can come first. `spec_update: [{"index": 1, "node": "boards_g"}]`
  edits one component (null clears a field; index = count appends).
- `summary` lists the spec with what is missing; `verify` runs the `spec`
  check by default once there are components, one line each (`spec crack
  crk_g`). The `door_slam` template carries its spec.
- It checks the graph against the brief, not the taste: a layer can sit in
  its slot and band and still be heard as something else.

## Macros: knobs for the human

You cannot hear, so give the human a front panel: a few knobs that move
several fields together, named for what they do. They turn them in the
window; you read their moves in `summary`.

```json
{"name": "brightness", "value": 0.5, "doc": "how cutting the zap is",
 "claim": ["centroid", 1], "game": false,
 "map": [{"node": "cutoff", "field": "b", "from": 1.2, "to": 7, "curve": "exp"},
         {"node": "lp", "field": "q", "from": 0.8, "to": 2.4},
         {"param": "size", "from": 0.2, "to": 0.8}]}
```

**Defining one**

- `value` is where it sits; `default` (for reset) is the first `value` you
  set.
- Each `map` entry writes one field over 0..1 (`curve` linear or exp; exp
  needs both ends above 0). `col` multiplies one column of a table field
  (resonator `modes`); `index` / `key` set one element of a list field.
- Deeper paths: `"node": "voices_name/patch_node"` (a `voices` patch);
  `"field": "points", "index": 1, "key": "x"` / `"y"` (a breakpoint);
  `"field": "state", "key": "y1"` (an `expr` state); `{"param": "size"}` (a
  sound param, also how a knob reaches a patch); `{"node": "<sound node>",
  "field": "macros", "key": "<part knob>"}` (a part's knob). Full forms and
  gaps: `docs/macros.md`, "Map entries that reach deeper".
- **Mapped fields are overridden**: the node's own value is ignored, and
  editing it with `sound_ops` warns ("field `time` is driven by macro
  `weight`: your value is ignored"; not when you write both in one call).
  Two macros may not write the same field.
- **Replace a macro** by naming it in `delete_macros` and `macros` in one
  call.
- `game: true` exports the knob in the manifest `inputs` for the game to set
  at runtime; others are baked at their value.
- Creating or redefining a macro renders it at 0, .25, .5, .75, 1 and errors
  with the position if one fails ("macro `m` at 0.75: ... too many events").

**Claims: the name is a promise**

`claim` is `[measure, direction]` (`1` rises over 0..1, `-1` falls). `verify`
(`macros`) sweeps each knob at 0, .25, .5, .75, 1: the measure must move that
way at every step (one step may go back by up to 15 % of the end-to-end
move), by at least 2 % of its largest value, with no position over -0.3
dBFS. A FAIL says to rename the macro or fix what it moves.

| Name | Claim |
|---|---|
| brightness / weight | `centroid` up / down |
| length | `decay` up |
| punch, snap | `crest` up |
| grit, howl | `flatness` |
| gustiness | `swing` |
| clang | `inharmonicity` up |
| rattle, buzz | `roughness` up |
| rate, busyness | `density` up (10-500 events/s) |
| softness, swell | `attack` up |
| spread, bounciness | `spacing` up |
| ring, sustain (multi-hit) | `ring` up |

- `claim` is optional. An unclaimed knob must still change the sound: from
  0 to 1 some 10 ms frame in one role band must move 1 dB or more, else "it
  does nothing audible". Claim when the name says what a measure does.
- An `output.lufs` target renormalizes: a loudness knob claiming `lufs`
  cannot move (use `peak_db`, claim another measure, or no claim). On a very
  short sound `decay`, `swing` and `crest` are unreliable: claim `density`,
  `attack`, `spacing` or `ring`. Each measure in full: `docs/macros.md`,
  "Claims".

**The human's moves** arrive in `summary` ("brightness 0.50 -> 0.72"). Keep
them: re-issue the macro with `value` (and so `default`) where they left it,
or re-centre its range. Moves are not history steps. `listen sweep: {param:
"brightness", values: [0, .5, 1]}` tiles a knob.

## Senses

**`listen`**: one PNG (spectrogram, waveform, loudness; seeds / sweep values
tile as columns, 8 at most) and the descriptor table (length, level, attack,
decay, onsets, pitch, spectrum, energy by role band, octave x segment dB,
modes, density, ...). Options: `node`, `seeds`, `sweep`, `zoom {t, hz}`,
`diff: true`, `descriptors: false`, `heard_as: false` (an audio-text model's
guess: a hint, not a check). Every line: `docs/senses.md`.

- `reference: "file.wav"` compares with one recording (aligned at the first
  onset, loudness-matched): an STFT distance in dB (about 10 dB is the spread
  between real recordings of one event: a coarse gate, not a judge) and the
  biggest band x time differences in words.
- `reference_set: "door_slam"` compares with `<project>/refs/<set>/` (two or
  more WAVs): the distance to each, the set's own spread, and eight features
  (weight, body, crack, crest, tail, centroid, voiced, ringing) against the
  span the recordings cover.

**`verify`** (and `expect` in `sound_ops`): each line PASS / FAIL with the
value; a FAIL says what to change.

| How you call it | What runs |
|---|---|
| no arguments | the defaults: `finite`, `peak_db` max -0.3, `silent_edges` (a loop: `loop_seam`), `dc`, `rebuild`, `macros` with knobs, `spec` with a spec |
| `checks: [..]` | the defaults plus these |
| `only: [..]` | just these (checks without settings: `finite peak_db true_peak_db silent_edges clicks dc loop_seam variations rebuild macros layers spec`) |
| `expect {..}` | checks with settings; **alone it runs only those** (add `checks` to keep the defaults) |
| `seeds: N` (2..8) | the checks on seeds 1..N: do it before calling a loop or a random texture finished |

- Checks with settings: `lufs` `duration` `bands` `pitch` `onsets`
  `reference` `reference_set` `aliasing` (a steady pitch only)
  `inharmonicity` `modulation` `roughness`, e.g. `{"duration": {"min": 0.4,
  "max": 1.2}, "onsets": {"min": 3, "max": 3, "gap": [0.15, 0.3]}, "pitch":
  {"hz": 440, "tol_cents": 25}}`.
- `layers` (`{"preset": "impact"}` or `"footstep"`) checks a designed hit;
  each FAIL names the layer to add. Ringing metal fails it by design:
  `{"preset": "impact", "ringing_max": 6, "crest_db": {"max": 30}}` or skip
  it.
- Every check's settings and thresholds: `docs/senses.md`, "verify checks in
  full".

**`extract_modes`** fits a WAV's strongest partials as a ready `resonators`
node. A noisy recording (a slam) is not modal: it falls back to an envelope
fit and says "NOTE the recording is not modal". `residual` writes what the
modes miss, only to study a recording (sounds are sample-free).

**Warnings in `summary`:** a node nothing reads, a frequency above Nyquist, a
silent output, DC over -40 dB, the ceiling limiter squashing the input, a
sound that never decays or is mostly silence, a missed `output.lufs` (with
the cause), the default fade-in cutting a first hit, a field driven by a
macro, a wood or metal `modal` with no `noise`.

## Source material: recordings and impulse responses

A relative file a node names (`sample` `file`, `grains` / `cloud` `source`,
`convolve` `ir`, `listen` / `verify` `reference`, `extract_modes` `file`) is
looked for in the project, then in the **sources folders**
(`<project>/assets/sources/`, then `<library>/sources/`; the path as written,
then its file name alone). `catalog {action: "sources"}` lists them.

- **Sounds are sample-free.** No recording goes into a sound: no `sample`
  layer, no `extract_modes` residual. Recordings are references, and
  `extract_modes` can read partials off one to rebuild as synthesis.
- **Only material the project may use**: your own recordings or CC0 (the
  repo ships none; reference sets live in `refs/`, gitignored).
- `convolve` resamples an impulse response at another rate (44.1 or 96 kHz
  work as is). Editing a WAV re-renders every node that reads it.

## Sessions, history, comments

- **Sharing.** Several agents may share one server; each keeps its own active
  sound. Results say what others changed in yours since your last call;
  `undo` / `jump` refuse to revert another client's step unless `force:
  true`.
- **History** is a tree of full snapshots with a cursor; editing at an old
  step branches. `history` lists steps newest-last with who made each.
- **Comments** (annotations) are the human's requests, pinned to a node and /
  or a time / frequency region; `summary` lists the open ones. Deal with
  each, then `add_annotation {resolve: id, text: what you did}`. Resolve,
  never delete.
- **Autosave** 0.5 s after the last edit into `<library>/session.json` (a
  second live server writes `session.<slot>.json`); `--restore` reopens it
  (the driver always passes it). `catalog save` is what persists a sound for
  other sessions.
- **The library** is `<project>/.sounder/` (a project is a folder with
  `.sounder/`, or a `Cargo.toml` beside `assets/`), else
  `~/Library/Application Support/friendslop/sounder/`; `FRIENDSLOP_DATA_DIR`
  overrides both.
- **Export** writes into `<project>/assets/sounds/` for a relative `path`:
  `<name>.ogg` (or `<name>_<i>.ogg` for `variations: N`, seeds 1..N) and
  `<name>.sfx.json`. A `path` ending in `.ogg` / `.wav` names the file
  itself (`renders/hit.wav` writes `hit.wav` + `hit.sfx.json` there; the
  extension sets the format).
  - **Ogg Vorbis is the default** (`quality` 0..1, default 0.5, about 160
    kbps stereo; same sound = same bytes; sample-exact, so loop points hold).
  - `format: "wav"` (`bits` 16 / 24 / 32) for a file you will feed back as a
    `reference`, `sample` or `extract_modes` input, which read WAV only.
    `bits` with ogg or `quality` with wav is an error.
  - The manifest has the format, per file `bytes`, seconds / samples / LUFS /
    peak, kind, loop points, game inputs, baked macros, seeds, the causal
    flag and the source catalog entry.
  - **For a video clip** (`docs/timeline.md`): with a sound `fps`, any time
    may be frames (`"f:123"` / `"123f"`); `export` `start_frame` / `offset`
    adds leading silence, `length_frames` / `length` pads or trims to the
    clip; the manifest records `fps`, `offset`, `start_frame`, `length_frames`.

## Failures explain themselves

Errors name the node and field and say what to do next (`node #5 (filter
"lp"): field `frq`: unknown field `frq`, expected one of ...`). When one does
not, or a call surprises you, tell the maintainers: `session_log(feedback:
"...")`. The friction log (`<library>/feedback.jsonl`, never sent anywhere)
also records every failed and slow call; `session_log(report: true)` or
`sounder --feedback-report` folds it into what to fix first. A panic inside a
call answers that call with "internal error: the call panicked (...)" and the
server keeps running: report it.

## Playback

`play {node?, seed?, loop?, stop?, doc?}` renders the sound (or one node, or
another seed) and sends it to the speakers: "playing `laser` (0.40 s) on
<device>; loop". `loop` defaults to true for `kind: loop`; `stop: true`
stops; a new `play` replaces what is playing. Speakers exist in the window or
with `sounder --headless --audio`; otherwise `play` errors and you use
`listen`. `play` succeeding means the samples were sent, nothing about how
they sound.

## More reference (not loaded here: read the file when you need it)

| Topic | File |
|---|---|
| Every node family in short: fields, a recipe, measured limits | `docs/nodes/README.md` |
| One node family in full | `docs/nodes/<family>.md` (`modal.md`, `water.md`, `machines.md`, ...) |
| Worked recipes: a sword clash, a UI confirm | `docs/nodes/README.md`, "Worked recipes" |
| The full brief -> part / template / node routing, the listener's notes per material, the door slam template's graph | `docs/routing.md` |
| Macro map paths, every claim measure with its limits | `docs/macros.md` |
| Every `listen` line, `heard_as`, every `verify` check's settings | `docs/senses.md` |
| Feedback loops, offline pitch / time / spectral, events and grains, `expr`, `voices` and `sound` instances, loop seams | `docs/graph.md` |
| Porting a patch from Farnell's *Designing Sound* | `docs/porting-farnell.md` |
| The window: data flow, the dev HTTP bridge, the webview probe | `docs/window.md` |
| How the tool was built and what listeners said | `docs/history/` |

# Part 2: Changing the tool

## Changing the tool: where things go

Name files for what they are, never for the task that made them: no work
package, round, phase or `_fix` in a file name, module name or comment. A
comment may say why a number is what it is ("a listener heard 0.1 as boiling
oil"), not which task set it.

| Adding | Goes in |
|---|---|
| a node | its family's file in `crates/sfx_graph/src/nodes/` (a new file only for a new family) |
| a node's tests | the family's module in `crates/sfx_analysis/tests/it/` (see `crates/sfx_analysis/tests/README.md`); helpers from `it/common.rs` |
| a node's reference (fields, limits, measurements) | `docs/nodes/<family>.md`, and a short entry in `docs/nodes/README.md` |
| a change made after listening | a section in that node's doc and tests in that node's module, not a new file |
| a field alias agents guess | `#[serde(alias = ...)]` on the field, "Alias `x`" in its doc comment; an alias that takes a word, not a node, also goes in the type's `Describe::WORDS` |
| a rule or recipe every agent needs | this file, kept short; the MCP `instructions` string in step |
| notes on how something was built, a plan, a round report | `docs/history/` (summaries only) |
| a part every project should have | `parts/<name>.json` and its line in `src/parts.rs`; parts for one project are `catalog save` with a `role` |
| a script you will run again | `tools/`; a one-off script stays out of git |

- **This file stays under about 70 KB, ideally near 50.** It holds rules,
  the workflow and short routing. A new node gets a row in `docs/routing.md`
  (the short table here only for a brief agents meet often); measured limits
  and examples go in `docs/nodes/<family>.md`, claim details in
  `docs/macros.md`, check details in `docs/senses.md`. Each moved detail
  leaves a pointer naming the file and section. Other files cite this file's
  section names ("Layering a designed sound", "Senses", ...): keep them, or
  grep and update the citations.
- **JSON blocks marked `<!-- recipe -->`** here and in `docs/graph.md` /
  `docs/nodes/README.md` are rendered by `crates/sfx_analysis/tests/it/recipes.rs`: keep them
  valid.
- **Not in git:** generated audio and images, exports (`assets/`), the sound
  library and session (`.sounder/`), reference recordings (`refs/`), raw
  per-sound logs, feedback or score dumps, experiment sounds (`*_old`,
  `*_new`, `ab_*`). Use the scratch directory for those.
- **Tests:** extend an existing module before adding one. Do not copy a
  helper into a module when `it/common.rs` has it or could. A measured limit
  belongs in the node's doc, not in an `#[ignore]`d test.
- **A file path written in a doc or comment must exist.** After a rename,
  grep for the old name.
- **The session thread** runs every call (`src/bin/sounder/server/bridge.rs`);
  a panic in a call is caught and answered as an error, so a render bug
  costs one call, not the server. Index into tables with a bounded index,
  not a clamped f32 (f32 cannot hold `len - 1.001` past ~16k entries).
- **Playback internals:** the window seeks and loops through the same
  `Player` (`src/player.rs`): `Engine` (pure, tested: resampling to the
  device rate, sample-accurate loop wrap, output limiter at +-1) behind
  `PlayerControl`; `Player` owns the cpal stream on its own thread and
  reports `PlayerEvent`s.
- **Before finishing:** `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt --all`, and `git status` shows
  nothing you did not mean to add.

## Not now

- A **live runtime** for the causal subset (`summary` reports whether a sound
  is fully causal; the loop-capable nodes have a per-sample form already, the
  rest are block-evaluated and would need one). Firewheel and
  `bevy_seedling` are checked out for that later.
- A project **sound style** (loudness targets per category); project-level
  macros across several sounds ("grit" on all the footsteps) belong with it.
