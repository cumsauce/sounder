# AGENTS.md

sounder is a standalone procedural **sound-effects** tool driven by an **MCP
server on stdio**. A sound is a JSON graph of nodes rendered offline to WAV.
Agents build sounds over MCP; the human listens and comments in a window
(`--headless` runs without it). Sibling of `modeler`: the same
shape (atomic edit batches, deterministic checks, an undo tree, a catalog,
comments the agent resolves), for audio. No game code.

```sh
cargo run -- --headless                 # the MCP server (stdio; exits on stdin EOF)
cargo run --example laser /tmp/sounder  # a laser zap: laser.wav + laser.png + descriptors
cargo test --workspace                  # graph, senses, session, and the driver script
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
```

This file holds the rules, the workflow and the recipes. The per-node
reference is in `docs/nodes/README.md` (see "More reference" below). The
critical rules are also served in the MCP `initialize` response
(`instructions`); keep the two in sync.

**The honest limit.** You cannot hear. The senses (`summary`, `listen`,
`verify`) get you to a plausible sound: right length, level, pitch, spectral
shape, no clicks. Whether it is *good* is the human's call, made with comments
and with the knobs (macros) you give them. Do not claim a sound "sounds
warm"; say what you measured.

## Getting started (agent)

The MCP config runs `python3 mcp_driver.py --proxy` (one shared daemon for
every session; each session is a client with its own active sound; the daemon
opens the window, so the human sees every agent's sounds in one place:
`--proxy --headless` keeps it off), or the
binary directly: `target/debug/sounder --headless`. Example `.mcp.json`:

```json
{ "mcpServers": { "sounder": {
    "command": "python3",
    "args": ["/path/to/sounder/mcp_driver.py", "--proxy"] } } }
```

The driver is also a shell client (`python3 mcp_driver.py --calls calls.json`,
`--connect`, `--save-shot`), for when the session's tool list is frozen;
consecutive `--calls` runs share one autosave, so a sound made in one run is
open in the next.
`SOUNDER_BINARY`, `SOUNDER_SOCKET`, `SOUNDER_PROJECT`, `SOUNDER_STDERR` mirror
modeler's `MODELER_*`.

1. `summary` — what is open, what the human asked for (open comments).
2. `new_sound` (or `sound_ops` with `sound: "name"`), then `sound_ops` to build.
   Run `catalog search` for parts first (see "Parts"), then
   look at "Which ready-made node for which sound": a brief that fits a
   ready-made node starts from that node, not a rebuilt graph. For an impact,
   a hit or a mechanism, read "Layering a designed sound": the node is the
   body layer, and a designed sound adds the pre, crack, weight and after
   layers on a dry bus.
   `describe_op` lists the 146 node types; `describe_op {kind: "filter"}` gives
   one's example and schema (`type` and `node` are aliases of `kind`). To
   remove a node put `{"type": "delete", "name": "x"}` in `ops` (one item per
   node; a top-level `delete` is not a field and errors).
   Write the brief as a **spec** first (see "The brief as data"): one
   component per layer the brief asks for, the node named once it exists.
3. `listen`, `verify`, fix, repeat.
4. Add **macros** (knobs) so the human can fit the sound by ear: at least one
   on every sound, unasked (`summary` reports "no knobs yet" until there is
   one), usually 1-4. Choose the dimension that gives the most useful control
   while staying inside the brief: the 1-2 fields that dominate the character
   (size / pitch, brightness, length, density, intensity). The brief's values
   are the default and the range ends stay near them: a knob must not turn the
   sound into a different one.
5. `catalog save`, `export`.

## The tools (16)

| Tool | What it does |
|---|---|
| `summary` | state, nodes (what reads what), params, macros + claim status, the human's knob moves since last read (reading clears them), open comments, warnings, causal yes/no, descriptors |
| `sound_ops` | atomic batch: `ops` (node upserts; `{"type": "delete", "name": ...}` removes), `params`, `macros`, `delete_macros`, `kind`, `duration` (null or 0 = automatic length again), `seed`, `sample_rate`, `expect`, `spec` (the brief as data; null removes it); renders for real; one history step if anything changed |
| `listen` | the image + descriptor table; `node`, `seeds`, `sweep {param, values: [..]}`, `zoom {t: [a, b], hz: [lo, hi]}`, `diff`, `reference`, `reference_set` (a bad `zoom` / `sweep` error shows the accepted shape) |
| `verify` | PASS / FAIL checks (`checks`, `only`, `expect`; see Senses); any FAIL is an error |
| `describe_op` | node types, one line each; or one type's example + schema (`kind`, aliases `type` / `node`) |
| `extract_modes` | strongest partials of a WAV as a ready `resonators` node; `residual: "x.wav"` also writes what they miss and returns a rebuild |
| `play` | plays on the speakers for the human (the window, or `--headless --audio`); a plain headless server answers with an error saying so |
| `new_sound` | opens a sound beside the others (`kind`, `duration` 0..600 s, `seed` can be set at once, so a loop is one call; `template` starts from an approved sound: `door_slam`; a loop with no `duration` is an error naming the field); an open name is refused |
| `catalog` | `list search sources describe save load delete close` (`search`: parts and saved sounds by `text` / `role` / `material` / `approved`; `sources`: the WAVs in the sources folders; `save` takes `role` / `material` to make a part); the sound is `name` or `doc` (both given and different is an error) |
| `export` | Ogg Vorbis files (default; `format: "wav"` for WAV) + `<name>.sfx.json` manifest |
| `undo redo jump history` | the history tree; one `sound_ops` call is one step |
| `add_annotation` | comment on a node / region, or `resolve` / `reopen` one |
| `session_log` | recent calls; `feedback` notes; `report` of the friction log |

**Every tool rejects an argument it does not take**: the error names the tool,
the key, and lists the keys it does take (a misspelt `descriptors` or a
top-level `delete` is an error, never silently ignored).

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
  -80 dBFS or 60 dB under its own peak (the higher) for 50 ms (doubling 2 s up to 30 s; then a warning that it never
  decays), then trims the trailing silence. A `loop` needs a `duration`.
- `duration` in `sound_ops`: a number fixes the length; **`null` (or `0`)
  returns to automatic length** (leaving it out changes nothing). Nodes that
  end after the document (`time_stretch`, `smear`, a long `reverb` tail,
  `thunder`) need a `duration` long enough, or they are cut.
- `seed` drives every random node (mixed with the node's name and its own
  `seed`). Same document + seed = bit-identical samples at any thread count.
- Mono is the default; a node fed stereo that only takes mono errors and tells
  you to add a `mix` with `"to": "mono"`.
- Node names `mono` and `stereo` are reserved (they are `mix`'s `to` words);
  an `ops` item with a bad name or an unknown field errors with the node's
  fields listed.
- **An upsert replaces the whole node.** Re-issuing a node in `ops` writes
  exactly the fields you give: a field left out goes back to its default
  (it is not merged). To change one field, send the node with all of its
  fields (`summary` lists them).
- **Names that trip people:** `pattern` starts at `start`, not `at` (`accel` 0.7 = last gap 0.7 x the first, for a speed-up). `impulse` takes `at`, a number or a list of
  times (`"at": [0, 0.1, 0.2]`), default 0. `wander` takes `rate` and
  `range: [lo, hi]` or `min` and `max` together (not both forms; `range`
  omitted is -1..1). `ramp` takes `time` or `duration` (the same field) and
  defaults to an `exp` curve (`"curve": "linear"` for a straight one) and
  holds its last value, so a sound whose level follows a `ramp` never decays
  by itself: set a `duration`. `gain` takes `db` or `gain_db` (the same field;
  both is an error) or a linear `gain`; `mix` inputs are names or objects
  (see "`mix` inputs"). `decay` `time` is the seconds to fall **60 dB** (exp,
  the default; to reach 0 with `"curve": "linear"`), not a time constant and
  not the -20 dB time `listen` prints: a `time` of 0.3 reads about 0.1 s at
  -20 dB. `at` delays the start (silent before it).

### The output stage

Exactly one `output` node: `{input, [gain_db], [lufs] or [peak_db],
[fade_in 0.002], [fade_out 0.005], [ceiling_db -1]}` (fades are 0 in a loop).
Gain, then the loudness target, then a lookahead ceiling limiter that holds
the **true peak** (4x oversampled, the `true_peak_db` estimator), then the
edge fades. **The target is met after the fades**: `peak_db` and `lufs` are
measured on the faded signal when the fades move the gain (by over 0.02 dB for
a peak, 0.1 LU for loudness), so a hit at t = 0 with `fade_in: 0.0005` and
`peak_db: -1.5` is delivered at -1.5 dBFS (+-0.1), not at the lower level the
fade used to leave. A sound the fades barely touch keeps its exact gain.

- `lufs` normalizes integrated loudness (up from as low as -70 LUFS too). A
  **short or peaky sound cannot reach a high `lufs`** under the ceiling: use
  `peak_db` instead (normalizes the pre-limiter sample peak to that dBFS; for
  clicks, ticks, short hits, impacts). Because the limiter holds the true
  peak, a noisy hit whose inter-sample peaks run over the ceiling is turned
  down, so its **delivered sample peak can be lower than `peak_db`** (the
  true peak sits at the ceiling, and the fade-in warning counts that target
  as met). A sound whose true peak is already under the ceiling passes
  bit-identical. `lufs` and `peak_db` together are an error; `peak_db` above `ceiling_db` is an error; `gain_db` stacks on either.
- A **missed `lufs`** (more than 1 LU off) warns with the numbers: pre- and
  post-limiter peak, crest, and how much the limiter removed. It names the
  cause: "too short or too sparse to reach this loudness: use peak_db" (crest
  of 20 dB or more), "the limiter took N dB off the peak" (compress or shape
  before the output, lower the target, or raise `ceiling_db`), or "the edge
  fades took N LU" (shorten them).
- The **default 2 ms fade-in cuts a hit that starts at t = 0** (a click, a
  gunshot). The render warns "the default 2 ms fade-in is cutting the first hit
  by N dB (it starts at X ms)" (not for a `fade_in` of 1 ms or less, nor when a
  `peak_db` / `lufs` target is set and met: the delivered level is right
  then): set **`fade_in: 0.0005`** (0.5 ms: the fade
  reaches zero at sample 0, so `silent_edges` still passes; `fade_in: 0` keeps
  the hit whole but `silent_edges` then fails on the non-zero first sample), or
  start the hit a few ms later (`at: 0.005`), as `explosion`, `gunshot` and
  `splash` do themselves. This holds for any first event at t = 0: a
  `bounce` or `pattern` whose first hit is at 0 (a coin, a dropped ball) is
  cut the same way (give the output `fade_in: 0.0005` or start the pattern
  at 0.005).
- The limiter-squash warning ("the ceiling limiter is squashing it")
  does not fire when `peak_db` is used and the delivered peak reaches the
  target; with `gain_db` pushing a `peak_db` target over the ceiling it still
  does.

## Nodes

`describe_op` has the full list with examples. A node is `{"type", "name",
...fields}`; unknown fields are errors that list the node's fields. Signal
fields take a number, `"$param"`, `"=expr"` (`+ - * /`, parentheses, `$name`)
or the **name of another node** (`"freq": "sweep"`: audio-rate modulation, a
port as `"verb.wet"`). A string that contains `$` or number arithmetic
(`"1.5 * $value"`, `"0.1 + $value * 0.9"`, `"2*$rand"`) is read as an
expression exactly as if it began with `=`, so a `voices` patch needs no leading
`=`; a string that is a valid node or port name (`"verb.wet"`, `"a_b"`) is always
a node. The error for an unknown node name shows the hint (`write expressions
with a leading `=`, e.g. "=1.5 * $value"`).

Families (146 types; groups marked * have a section in
`docs/nodes/README.md`):

- sources: `osc noise impulse sample` and `fm wavetable`*; control `lfo wander
  sample_hold slew ramp` and `chaos fbm curve switch pitch_follow
  centroid_follow`*; envelopes `env adsr decay follower` and `trigger_env
  event_gate seq notes`*; maths `math remap gain`, `expr`.
- filters `filter onepole comb allpass resonators` and `ladder eq filterbank
  vocoder multiband`*; time `delay convolve`, `reverb` (models)* and `ir`*;
  nonlinear `shape compress limit` and `ringmod transient gate sub exciter`*; the bus `finish`*;
  exaggeration `subdrop riser`*;
  modulation effects `chorus flanger phaser freq_shift`*; lo-fi and loops
  `crossfade_loop resample tape stutter`*.
- mix `mix pan width` and `haas decorrelate doppler`*; offline `reverse
  normalize trim fade` and the spectral nodes `pitch_shift time_stretch
  spectral` (with new modes)*.
- events `once pattern poisson bounce burst on_threshold shatter`* with
  `grains cloud`* and `voices`; `sound` (an instance of another sound).
- physical models `modal contact friction impact rig` (with shapes, `loss`, `tilt`)*,
  mechanism clicks `click`*, the foley ones
  `bubble air glottis tract syrinx thunder` (voice upgrades)*, waveguides
  `tube string`*, surfaces `rolling cloth crumple footstep`*, water `water splash`*,
  elements `fire wind spark`*, machines `engine motor clock fan`*, from the book `bell ball_bounce cricket twang siren door_creak jet red_alert`*, blasts `explosion
  gunshot`*; `output`.

Recipes (the JSON in the sections below renders as written):

- **Pitch sweep (laser, zap):** `ramp` 2000 -> 200 `curve: "exp"` into an
  `osc` `freq`, times a `decay`, through a `filter` whose `freq` is a scaled
  copy of the ramp (a `math` mul).
- **Struck object (knock, bell; a latch or switch click is the `click` node):** an `impulse` (or a short noise burst
  times a fast `decay`) into `resonators` `[[hz, decay_s, gain], ...]`, or a
  `contact` into `modal` (pick a `shape`). `extract_modes` starts from a real
  recording.
- **Texture (wind, hiss):** `wind`, `fire`, `water` do it in one node; or
  `noise` pink into a `filter` bp whose `freq` is a `wander` range, the level
  shaped by another `wander`.
- **Machine / vehicle:** `ramp` or `wander` into `engine` / `motor` `rpm`.
- **Impact, break, step:** `step`, `explosion`, `gunshot`, `splash`, or
  `shatter` -> `voices` of a small `modal` per fragment.
- **Thick or moving:** `osc` `unison`; `chorus` / `phaser`; `doppler` for a
  fly-by; `reverb` `model` or `convolve` an `ir`.
- **Loudness:** set `output.lufs` (sustained sounds) or `output.peak_db`
  (clicks, short hits); see "The output stage".
- **Edges:** the output fades 2 ms in and 5 ms out by default (0 for loops);
  set `fade_in: 0.0005` for a hit at t = 0.

## Layering a designed sound

A finished game or film SFX is several layers, each owning a band and a time
slot, through one dry bus. One node (or one chain) gets you a correct sound;
layers get you a designed one. In the Phase 0 test (`docs/history/phase0-results.md`)
the single-chain door slam an agent built in round 0 sat 14.6 dB (STFT
distance) from real door slams. The layered rebuild sat 9.2 dB, closer than
one real slam is to the others (10.4), and the listener called it "basically
perfect". The listener's ears, not the distance, found the four faults the
first layered version still had: they are the rules below.

| Role | Band | When | Made from |
|---|---|---|---|
| **pre** (anticipation) | 1.5-5 kHz | 5-50 ms before the hit | `friction` + noise under an `env`: a door brushing its frame, a blade sliding |
| **crack** (transient) | 2-6 kHz | the first 5-25 ms | white noise x a fast `decay` (`time` 0.008-0.03) through a `filter` bp or hp |
| **body** (the object) | 150 Hz-2 kHz | 50-500 ms | `modal` with the right `material`, struck by a **noise burst**; or a ready-made node |
| **weight** | 60-350 Hz | 100-500 ms | band-limited noise: brown noise x `decay` through a `filter` lp (`slope` 24), pink noise through a wide bp |
| **after** (settle, debris, ring) | any | after the body | a second small hit (`contact` -> small `modal`), `voices`, or the body's own ring |

Rules (each one a fault the listener heard):

- **Dry and mono by default.** No `reverb` unless the brief names a space: the
  game puts the sound in its room. A tail is the object's own ring or an
  after-layer, not a reverb.
- **Impacts are atonal.** A sine sub drop plus a few clean `modal` modes read
  as "a tuned drum". Build weight from band-limited noise, excite `modal` with
  a noise burst (not a lone `impulse` or `contact`), keep its `loss` at 1.2 or
  more for wood. Check `listen`: a wooden impact should read `voiced` near 0
  (the real slam 0.15; the tuned-drum version 0.9).
- **Keep materials apart.** `click` (its resonators are 1.5-8 kHz) and a hard
  `crumple` (hardness 0.9) read as "a chime, rusted metal pieces clinking":
  keep them out of wood, cloth and flesh sounds. A wooden second hit is a small
  `contact` -> `modal` wood, low-passed.
- **Add the detail before the hit** where the object would make it (a door
  sticking on its frame, a hand on a handle, a blade sliding): it is what makes
  a hit read as a real event.

The bus: `mix` the layers mono, then **`finish`** (one node: `punch` 0..1,
`tail_db`, `glue` 0..1, `warmth` 0..1, `weight_db` / `air_db` shelves at 120 Hz
/ 6 kHz, `width` above 150 Hz for a stereo mix; no reverb), then `output`.
`{"type": "finish", "input": "dry", "punch": 0.4, "tail_db": -6, "glue": 0.3,
"warmth": 0.6, "weight_db": 4}` on the door slam's layers measured a mean
distance of 9.32 dB to the real slams, against 9.23 for the hand-built bus in
the recipe below (`transient` `attack_db` 3-5, `shape` `tube` `oversample` 4
`mix` 0.2-0.25, an `onepole` hp at 30-40 Hz for the tube's DC, an `eq` peak).
Macros on `finish`: `glue` (claim `crest`, -1), `punch` (claim `crest`, 1, but
leave the claim off on noisy layers: it held on 3 of 4 seeds of the slam),
`tail_db`, `weight_db`. Details: `docs/nodes/finish.md`. Balance with a
`gain` (`db`) per layer before the `mix`; give the human one macro per layer
level (pre, crack, weight, after) and one on the body (`size` or `loss`).
Measured targets for a heavy wooden impact (the real slam): crest about 16 dB
(21.6 for the rebuild), energy under 200 Hz / 200 Hz-2 kHz / 2-8 kHz near
38 / 56 / 5.5 %, -20 dB about 200 ms after the hit.

Check a hit with `verify {checks: ["layers"]}` (the `impact` preset: energy
by role, voiced, ringing partials, crest, no reverb; see "Senses"): every
FAIL names the layer to add.

### The brief as data

A `spec` on the sound (`sound_ops` `spec`) lists the components the brief
asks for, so the graph can be checked against the request, not only against
"plausible":

```json
{"brief": "a heavy wooden door slammed shut", "components": [
  {"role": "pre", "node": "scr_g", "at": [0, 0.05], "note": "the door brushing its frame"},
  {"role": "crack", "node": "crk_g", "at": [0.05, 0.08]},
  {"role": "body", "node": "panel", "material": "wood", "at": [0.05, 0.5]},
  {"role": "body", "node": "body_g", "at": [0.05, 0.6], "length_ms": [50, 250]},
  {"role": "weight", "node": "thd_g", "at": [0.05, 0.5], "length_ms": [40, 200]},
  {"role": "after", "material": "wood", "note": "a second small knock"}]}
```

- `role` is a part role (`pre crack body weight after bed texture whole`);
  `node` the layer's last node before the mix (left out: not built yet).
- `material` is checked against every `material` field the node reads; `at`
  [start, end] s must hold 80 % of the layer's energy (10 ms margin), `band`
  [lo, hi] Hz half of it. With no `band` the role's applies: pre 1-8 kHz,
  crack 1.5-16 kHz, body 100-3000 Hz, weight 20-400 Hz (widened from the
  table above so the approved slam's layers pass).
- `length_ms` [min, max]: how long the layer lasts (the span holding the
  middle 80 % of its energy). `at` only bounds where the energy may sit.
  In the approved slam the noise-struck modal panel lasts 5 ms, the
  band-passed noise body 94 ms, the weight 73 ms: a modal body is mostly
  its strike, and a hit's length comes from a noise body and the weight.
  A body made of a modal alone fails a `length_ms` of [40, 300].
- A `sound` node (a part) has no `material` field, so a component naming
  one reports "no material field to check": leave `material` off a part's
  component (the part's own material is in `catalog describe`).
- A sound with no nodes yet keeps a spec-only `sound_ops` call unrendered,
  so the spec can come first.
- `spec_update: [{"index": 1, "node": "boards_g"}]` edits one component
  (null clears a field; an index equal to the count appends one), so a
  layer's node is named as it is built without resending the spec.
- `summary` lists the spec with what is missing (no node yet, no such node,
  not reaching the output, another material); `verify` runs the `spec`
  check by default once a spec has components, one line per component
  (`spec crack crk_g`). The `door_slam` template carries its spec.
- It checks the graph against the brief, not the taste: a layer can sit in
  its slot and band and still be heard as something else.

### Parts: assemble from layers the human has heard

A **part** is a small saved sound that fills one role (`pre crack body weight
after bed texture whole`), with a material, tags and one to three knobs.
Before building a layer from nodes, look for one:

- `catalog {action: "search", role: "body", material: "wood"}` (also `text`:
  any word of a name, tag or description, plurals and endings folded, most
  hits first; `approved`): the library's parts and saved sounds plus the 130 built-in
  parts in `parts/` (table below). Each is named and described by what the
  listener heard (`heard as "..."`), which is often not what it was built
  as: search by the sound you need, not by the node. Approved ones come
  first. `catalog describe` shows a part's nodes and knobs.
- Use it with a `sound` node: `{"type": "sound", "name": "body", "sound":
  "wood_panel_body", "at": 0.045, "gain_db": 0, "macros": {"size": 0.6}}`. A
  hit part strikes 5 ms after its start; `wood_brush` ends at 50 ms, so the
  hit goes at `at: 0.045`. Parts carry no loudness target, so their levels
  are the balance they were approved at: move them with `gain_db`, then
  `mix`, `finish`, `output` as for any layered sound.
- A part can be the whole sound (role `whole`, most `impact` / `rig` parts):
  one `sound` node into `output` with `peak_db`; it needs no extra layers.
  Give the document the part's `duration` (`catalog describe` shows it):
  with automatic length the instance is cut at 60 dB under its peak (a
  struck string part came out 0.87 s instead of its heard 2.5 s). A part
  that starts with a fall (`rig_ball_on_drum`: 0.27 s before the first
  hit) can go through `trim` to lose the silence.
- A knob on the outer sound can drive a part's own knob: `{"node": "body",
  "field": "macros", "key": "size", "from": 0.2, "to": 0.9}` (`node` is the
  `sound` node, `key` the part's knob). Knobs on parts are usually
  unclaimed: an event part's spacing and a strike knob's centroid move
  unevenly under a `peak_db` output.
- `catalog load` a part to edit a copy; a library sound with a built-in's
  name is used instead of the built-in.
- Save a layer worth reusing with `catalog {action: "save", role: "crack",
  material: "metal", description: ..., tags: [...]}`. Give it no `lufs` /
  `peak_db` target and put its event at 5 ms.
- **Approval is the human's.** Only the window sets `approved`; saving a
  changed document clears it and the result says so. A part that is not
  approved is a draft: say so when you use one.

The five built-in parts are the layers of the approved door slam, and the
listener approved each one alone as well (2026-11-18). Reassembled with the
bus above they pass `layers` (`tests/parts.rs`).

### Parts the listener picked, by use

Start from these. Each was picked best of its box, or rated good blind.

| Need | Parts | Stack |
|---|---|---|
| a door slam | the `door_slam` template; its layers `wood_brush`, `noise_crack`, `wood_panel_body`, `low_thump`, `wood_knock` | the template is the approved slam |
| footsteps | `all_purpose_step`, `concrete_step`, `gravel_step`, `grass_step`, `dirt_step`, `wood_step`, `carpet_step`, `metal_step` (all `step`) | `gravel_crunch` or `grass_swish` (step textures) over `all_purpose_step`; a walk is `step` with an `at` list |
| a pipe, coin or wooden block dropped, bouncing | `impact_pipe_drop`, `impact_coin_drop`, `impact_block_drop` (`impact`) | a crack from the part's `force` over it |
| a loose lid or part rattling, shaken | `impact_lid_rattle` | |
| a sign, panel or sheet hit while the wind pushes it | `impact_sign_wind` | |
| a metal bar (or glass, a saw) bowed: a sustained eerie tone | `impact_bowed_bar` | |
| a box or crate dragged, scraped | `impact_box_scrape` | |
| a struck sheet that bends or wobbles as it rings | `impact_bending_sheet` | |
| objects acting on each other | `rig_ball_on_drum`, `rig_pan_and_lid`, `rig_tin_roof_screw`, `rig_string_on_box` (`rig`) | |
| stone and metal hits | `brick_or_rock_drop`, `small_brick_drop`, `heavy_stone_hit` (a wall); `large_pipe_drop`, `large_metal_container_hit`, `metal_plate_gong_hit`, `metal_bell_hit`, `metal_into_small_glass` | `wooden_crate_drop` is a whole crate on concrete |
| clicks and taps | `small_object_taps`, `mouse_click_tap`, `keyboard_button_hit` | |
| creatures | `creature_roar`, `creature_growl`, `creature_snarl`, `creature_whine`, `small_frog` | `creature_snarl_breath` under any voice, `creature_roar_voice` for the voice alone |
| rain and water | `rain_on_stone`; `splash_close`, the 5-30 cm splashes, a deep bubble pop | `rain_drops_on_grass` over `rain_hiss` |
| beds and loops | `surf_shingle`, `lava_bubbling`, `wind_in_trees`, `underwater_bed`, `cave_drips`; fire, storm wind, fan, sparks, crickets | |
| machines and alarms | `small_motor_spin`, engine rev, siren, alarm, error beep, laser | |
| others | rattlesnake rattle and scissors snip (cicada patches heard so), door creak, bird, breath, bells, flute, plucked string, twang | |

**Not in the library** (every attempt was heard as something else, or set aside): a ball or marble rolling (`impact` `slide` over `roughness`: four rounds heard as crackle or crunch, set aside 2026-11-19), footsteps in snow or on ice (`step` has both; two rebuilds each were heard bad), a stream (heard as a gurgling tube), and a large splash heard from a distance. Build those from nodes and say they are unheard.

**From the round 6 listen (testers' sounds, judged by ear):**
- **Door slam:** `new_sound` with `template: "door_slam"` (the recipe below, approved by ear, ready to tune); keep
  its weighty low layer (band-limited noise under 200 Hz) under the detail:
  detail without it reads light, weight without detail reads like a drum. A
  brief that names a latch adds a `click` (`noise` about 0.75) as the after
  layer; the slam is still the layered recipe, not a click over one `modal`.
- **Footstep:** a step is not an impact. Use `step` or a step part (table
  above), not `footstep` (heard as a snare on concrete, a pitter-patter on
  gravel); check with `layers` `{"preset": "footstep"}`.
- **Knock:** the approved pattern: a `pattern` into `voices` of a `contact`
  into a wood `modal` (`noise` 0.6, `noise_width` about 0.15, `tilt` 4,
  `loss` about 3), plus a short crack and a noise thud per knock, through
  `finish`. Pitch it slightly low (lowest mode near 200 Hz).
- **Metal (pipe, bar, lid, bin, tool, coin):** `modal` metal struck by a
  short noise burst with `noise` 0.4, `noise_width` 0.1 and a **low `loss`
  (0.3-0.6) so it rings for seconds**; plus a crack and a thud per bounce.
  All five metal objects built this way were heard as metal (a pipe as
  "large pipe drop"). The same objects as clean sine modes, even with
  split, beating pairs, were heard as "digital bell" / "fm bell"; the
  earlier short rings (0.1-0.4 s) as glass and ceramic. Start a dropped
  pipe low: lowest mode 120-200 Hz (`f0`), not 300+ (a very small pipe).
- **Stone (a rock, a brick):** brown-noise weight low-passed at 200-230 Hz
  (0.15-0.22 s), crackle-noise grit band-passed at 2-2.5 kHz, band-passed
  noise at 700-800 Hz for the body, a white-noise crack, and a quiet
  `modal` stone body (f0 700-900, loss 2-2.5, 6-8 dB under the noise),
  then a few chips or a small bounce. Heard as "brick dropped or medium
  rock hit". A louder, longer-ringing stone body (loss 1.2) reads as a
  woodblock; small bright dead hits read as mouse clicks; a boulder with
  long low weight reads as a wall.

**When one node is still right:** ambience and loops (`fire`, `wind`, `water`),
UI blips, a single texture, or a brief that names one ready-made sound
(`step`, `explosion`): use it as the body layer and add layers only if the
brief asks for more weight or detail. When a project has reference recordings
(`refs/`), tune the layer levels against them (`verify` `expect reference`):
the distance is a coarse gate (about 10 dB is the spread between real
recordings of one event), not a judge.

The heavy wooden door slam (`slam_b2` in Phase 0) is the `door_slam` template:
`new_sound {template: "door_slam"}` or `catalog describe` it for the graph
(brush `scr_g`, crack `crk_g`, noise-struck wood `panel`, band-passed noise
body `body_g`, brown-noise weight `thd_g`, knock `knock_g`, then `transient`
3.1 dB, `shape` tube mix 0.25, `onepole` hp 30, `eq` peak 200 Hz +5.6 dB,
`output` lufs -28).

A sword clash (two inharmonic `modal` bars) and a UI confirm (tonal on purpose,
an echo instead of a reverb) are worked recipes in `docs/nodes/README.md`,
"Worked recipes".

`examples/door_slam.rs` renders the door slam and checks it.

## Which ready-made node for which sound

Round-1 testers built every footstep from primitives and never tried `fire` or
`footstep`: the nodes below exist so that a brief that names one of these
sounds starts from one node (the body layer) plus a few knobs. **Building from primitives is still
fine** (and right for anything not listed: a laser, a UI blip, a whoosh, a
drone); `describe_op {kind}` gives each node's fields and an example, and the
sections below give the measured limits. None of these has been heard (the ports from `stream` to `two_tone` were rated once, blind: their rows say how they were heard): pick by
the brief, then `listen`, `verify` and let the human's macros do the rest.

| Brief | Node | Start with | Fits |
|---|---|---|---|
| **a door slam** | `new_sound` `template: "door_slam"` | knobs `weight`, `snap`, `stick`, `knock`, `latch` (a click latch, off at 0) | the layered slam the listener approved (brush, crack, noise-struck wood body, noise weight, knock, dry bus), copied whole and editable. Two rounds of testers given only the recipe built a click over one `modal` instead, heard as weak: **start from the template**, then tune the knobs |
| a footstep, a walk (older model) | `footstep` | `surface`, `weight`, `pace`, `shoe` | heard as a snare on concrete, a pitter-patter on gravel, a blow on grass: use `step` (next row); kept for its wood steps (`ring` 0.6, `weight` 0.25, `noise` 0.35) |
| footsteps on concrete, gravel, grass, dirt, snow, carpet (foley's model) | `step` | `surface`, `speed` m/s (1 a walk), `grit` 0..2, `layer` all / body / texture (they add back exactly), `at` a list for a walk | a Hertz heel strike on the surface's modes, scuff, ball of the foot; grains, swish, snow crackle. Heard: gravel good ("really good footsteps"; `grit` 0 is the all-purpose step), concrete picked (`concrete_step`, `all_purpose_step` are parts); grass on soil with the crunch at the heel picked ("a bit heavy"; `grass_step` lightens it with a high-pass) |
| fire, a campfire, crackle | `fire` | `size`, `intensity`, `crackle`, `loop` | roar + hiss + Poisson pops; loops by itself in a `kind: loop` document |
| wind, a gale, howling | `wind` | `speed` (signal), `gust`, `whistle`, `loop` | five moving bands (+ an Aeolian tone); a `ramp` on `speed` is a rising gale |
| electric arcs, sparks | `spark` | `rate` (snaps/s), `buzz`, `body` | a beating transformer hum, gated arcing chirps and formant snaps (Farnell) |
| a stream, a brook, rain on water | `water` | `flow` (signal), `density`, `splash`, `depth` | a Minnaert bubble population plus hiss; a loop in a loop document |
| a splash, something dropped in water | `splash` | `size` (m) | cavity bubbles and a pour with no noise burst, droplets falling back, then bubbles whose pitch rises (after Farnell's poured liquid and rain drops; rated good by ear at 5-20 cm, bad at 60 cm); `impact: "noise"` is the earlier opening, heard as a snare; a transient: use `peak_db` |
| a single drip, a blip of water | `bubble` | `radius_mm`, `jitter`; in `voices` `map` a `$rand` radius | one Minnaert bubble: drips, water_drips example |
| cloth, foliage rustle | `cloth` | `motion` (signal), `weight` | power-law rustle bursts in four bands; shape `motion` with a `wander` or `ramp` |
| crumpled paper, foil, plastic, dry leaves | `crumple` | `density` (signal, bursts/s up to 400), `size`, `hardness` | band-passed noise bursts whose count follows `density` (cloth's saturates); hardness 0.2 paper, 0.85 foil; `peak_db` for a sparse one |
| rolling, scraping, sliding | `rolling`, `friction` | `speed` (signal), `roughness` | an excitation: put it through `modal` (a scale first: `rolling` is loud at high speed) |
| breaking glass, shards, debris | `shatter` into `voices` | `pieces`, `spread`, `exponent`; a small `modal` per fragment | a crack then power-law fragments; `modal` glass with `loss` and `tilt` per shard |
| explosion, gunshot, blast | `explosion`, `gunshot` | `size`, `distance`; `caliber`, `barrel`, `suppressed` | finished one-shots, start 2.5 ms in (the default fade-in does not cut them); distance delays and darkens |
| engine, motor, vehicle | `engine`, `motor` | `rpm` (a `ramp` or `wander`), `cylinders` / `poles`, `load` | rpm-driven pitch; set a `duration` (ramps hold) |
| a whoosh, a swing, a sword or stick through the air | the part `whoosh_sweep` (`catalog search` `text: "whoosh"`, heard as "whoosh") | `at`, `gain_db`, its knobs | noise band-passed wide (q about 1) and swept up then down over 0.3-0.5 s. Narrow bands (q 2.5-6) were heard as "an artificial sweep, filtered noise": keep the band wide |
| a pipe, a flute, a string, a bow | `tube`, `string` | `length` / `freq`, `excite`, `pressure` | waveguides: winds, plucks, strikes, bowed notes |
| a latch, a switch, a button, a lock, a buckle | `click` | `kind` latch / switch / button / lock / clasp, `size`, `hardness`, `noise` about 0.75 for a small plastic or metal part (the listener's setting; the default rings pitched, voiced 0.33-0.78) | one or two transients through small resonators; a hit at t = 0: `fade_in` 0.0005 |
| a bell, a bouncing ball, crickets, a ruler twang, a siren, a creaking door, a jet, a sci-fi klaxon, a stream or a pour, a helicopter or a rocket, a fly or cicadas, surf, lava or a cave, frogs, leaves or underwater, a droid / transporter / torpedo / beam, phone tones, thunder | one node each: `docs/nodes/README.md`, "More ready-made nodes by brief" | its row | Farnell ports and foley beds, each heard once |
| rain, a shower, a downpour, rain on a roof | `rain` | `surface` water / stone / leaf / wood / soft / glass / metal, `rate` mm/h (2 light, 10, 50 a downpour), `roof` + `roof_height`, `loop`, `layer` all / drops / hiss (they add back) | physically sized drops over a surface-coloured hiss (after the foley project); levels differ by 20 dB per surface: set `lufs`. Heard: soft and wood as rain (good), leaf and stone as a stream (good), glass and metal as chimes, water as white noise |
| an animal roar, growl, snarl, whine | `creature` | `call`, `size` 0..1 (tract length and pitch), `aggression`, `roughness`, `length`, `layer` all / voice / breath (they add back) | foley's LF voice through a waveguide tract; every call picked (the creature parts) |
| a struck object: panel, bell, pipe, keys, a door's thud | `modal` fed by `contact`, `impulse` or a noise burst | `shape` plate / bar / tube / membrane / string / shell, `material`, `size`, `thickness`, `loss`, `tilt`; **`noise` for real wood (0.6, `tilt` 4) and metal (pipe, bar, lid, tool, coin: 0.4, `noise_width` 0.1, `loss` 0.3-0.6; 0.85 with `noise_width` 0.5 reads as a thin sheet)** (clean modes read as a tuned instrument) | normalized; `size` and `thickness` are pitch knobs (small thin objects: `thickness` 0.0015 instead of a custom material); `loss` 0.5 rings twice as long; `tilt` flattens a one-mode plate |
| how a hit plays out over time: a dropped pipe / coin / block, a loose lid rattling, a sign in the wind, a bowed bar, a box dragged, a struck sheet that bends | `impact`; **start from its part** (`sound` node or `catalog load`): `impact_pipe_drop`, `impact_coin_drop`, `impact_block_drop`, `impact_lid_rattle`, `impact_sign_wind`, `impact_bowed_bar`, `impact_box_scrape`, `impact_bending_sheet` | `height` or `speed`, `object_mass`, `striker_mass` (left out: a drop onto `floor`), `bounce`, `scatter`, and `modal`'s shape / material / `f0` / `loss` / `noise` | a striker (or the floor) and the modes stepped together (Hunt-Crossley). Picked blind over the approved builds (2026-11-19). The body layer: a crack from its `force` port (white noise x `hit.force`, hp 2 kHz). Lost by ear: rolling, creaks (use `door_creak`), a pitch bend on a low sheet. Single hits stay on the parts / template / `contact` or noise -> `modal`; hand-timed rhythms on `pattern` / `voices`. Peaks at -6 dBFS |
| several objects acting on each other: a ball on a drum, a pan with its lid, a tin roof with a loose screw, a plucked or struck string on a wooden body | `rig`; **start from its part**: `rig_ball_on_drum`, `rig_pan_and_lid`, `rig_tin_roof_screw`, `rig_string_on_box` (picked blind 2026-11-19/20) | `bodies` (point masses, modal objects, `string` / `tube` waveguides) and `joints` (`contact`, `spring` with a `gap` for a rattle); a rattling part needs `noise` about 0.7; leave the `air` joint out (it lost both listens) | `impact` generalised, vertical only; see `docs/nodes/rig.md` |
| a larger-than-life hit (cinematic, trailer, a game's heavy impact) | `subdrop` under it, `riser` into it | `from` / `to` / `decay`, `drive`; `at` (the hit), `length`, `shape` | tonal on purpose: add them to a layered hit only when the brief asks for exaggeration (a realistic slam with a sine sub read as a tuned drum) |
| a robot voice, speech-like texture, band dynamics | `vocoder`, `multiband`, `filterbank` | a pitched `carrier` + a noisy `modulator`; `crossovers` + `bands` | see "Bands and vocoder" in `docs/nodes/README.md` |
| a swarm, a swell, a granular wash | `cloud`, `grains` | `source`, `density` (signal), `size`, `pitch_spread` | grains from any node or WAV |
| a vowel, a creature call | `glottis` into `tract` | `f0` (signal), `tension`, `vowel`, `scale` | `docs/nodes/README.md`, "Foley models" |

Scene sounds layer these: a door slam is the layered recipe in "Layering a
designed sound" (not a `click` latch under a `modal` thud: that read as metal
and as a tuned drum), a campfire is `fire` plus a few `spark`s, rain is the `rain` node.
One `sound` per layer and a `mix` with `gain` / `at` keeps each one editable.

Layer levels: the ready-made nodes are not normalized and sit at different
levels (a default `fire` about -30 LUFS, a `footstep` peaks near -12 dBFS, a
`click` press at -6 dBFS), so balance a scene with a `gain` node (`db`) per
layer **before** the `mix`, then one `peak_db` / `lufs` on the output. For
example `fire` -> `gain` db 0 and `click` -> `gain` db -8 and `footstep` ->
`gain` db -4, each a `sound` or a node, summed by one `mix`. A macro maps the
`gain` node's `db` (or a `mix` input's `gain`), so "fire level" is one knob
per layer. `contact` and `modal` take their own `gain_db` (-60..60, 0 =
unchanged), so a struck layer's level needs no extra node.

### Variation

`seed` (the sound's, `new_sound` / `sound_ops` `seed`) is mixed into every
random node, so **a seed changes only what reads it**: `noise`, `wander`,
`poisson`, `voices` (`$rand`), `grains`, `cloud`, `shatter`, `pattern` with
`jitter` or `values`, `bubble` and `bounce` with `jitter`, and the seeded model nodes
(`footstep`, `click`, `fire`, `wind`, `thunder`, ...). A chain of `osc`,
`filter`, `ramp`, `decay`, a `pattern` without jitter, a `bubble` or
`bounce` without jitter, `modal` fed by a plain `impulse` renders the same bits for every seed,
so `export` with `variations: N` writes N identical files and warns ("all N
variations are bit-identical"), and `verify expect variations` fails. To get
four different takes:

- jitter the timing: `pattern` `jitter` (seconds), `bounce` `jitter` (a
  fraction of each gap, 0..0.5), `poisson` for a stream;
- randomize the voice: `voices` with a `map` of `$rand` expressions (pitch,
  `modal` `size`, level), `bubble` `jitter` (0..0.9) or `grains` `jitter` /
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

### `mix` inputs

`mix` `inputs` is a list whose items are a node name or an object
`{"input": "node", "gain": 0.5, "pan": -0.3, "at": 0.1}`: `input` (required),
`gain` (linear; a number or a node, so a `decay` envelope works), `pan` (-1..1,
makes the mix stereo), `at` (seconds of delay: a tail after a transient). Names
and objects mix freely, `to: "mono" | "stereo"` forces the width. The gain is
`gain`, not `gain_db`: use a `gain` node with `db` (or `gain_db`) before the mix
for decibels. A macro can move one input's level: `{"node": "scene", "field":
"inputs", "index": 1, "key": "gain", "from": 0.1, "to": 0.9}`. Inside a feedback
loop a `mix` takes no `pan` / `at`. Layer with the object form (a tail with
`at`, a balance with `gain`) rather than extra `gain` nodes.

## More reference (not loaded here: read the file when you need it)

| Topic | File |
|---|---|
| Every node family in short: fields, a recipe, measured limits (`modal` / `contact` / `friction`, the foley models, sources, filters, dynamics, `finish`, exaggeration, modulation, space, motion, control, events to control, lo-fi and loops, the physical models, spectral, bands, grains) | `docs/nodes/README.md` |
| One node family in full | `docs/nodes/<family>.md` (`modal.md`, `water.md`, `machines.md`, ...) |
| Feedback loops, offline pitch / time / spectral, events and grains, `expr`, `voices` and `sound` instances, loop seams | `docs/graph.md` |
| Porting a patch from Farnell's *Designing Sound* | `docs/porting-farnell.md` |
| The window: data flow, the dev HTTP bridge, the webview probe | `docs/window.md` |
| How the tool was built and what listeners said | `docs/history/` |

Rules that matter before you open those files:

- A cycle in the graph is legal only through a `delay` node; a loop's gain stays under 1.
- An event stream (`pattern poisson bounce burst once on_threshold shatter`) is not audio: turn it into sound with `voices` or `grains`, into control with `trigger_env`, `event_gate` or `seq`.
- `voices` renders its `patch` once per event (`$value`, `$index`, `$rand`, `$t`; `max` is an error when exceeded); `sound` instances another document.
- Loops get `loop_seam`: put `crossfade_loop` last, after any filter; `fire`, `wind`, `spark`, `water` render periodic in a `kind: loop` document, but a `fire` loop failed the seam on one of four seeds: run `verify {seeds: 4}` and add `crossfade_loop` if one fails.
- `modal` takes `size` or `f0` (not both), is normalized, and needs `noise` for real wood (0.6) and metal (0.4 with `noise_width` 0.1 and a low `loss`; 0.85 reads as a thin sheet) or it reads as a tuned instrument.
- `pitch_shift`, `time_stretch`, `spectral` are offline: the output is exactly the document's length, so a stretched sound needs a `duration`.

## Macros: knobs for the human

You cannot hear, so give the human a front panel: a few knobs that move
several fields together, named for what they do. They turn them in the window
until it fits; you read their moves in `summary`.

```json
{"name": "brightness", "value": 0.5, "doc": "how cutting the zap is",
 "claim": ["centroid", 1], "game": false,
 "map": [{"node": "cutoff", "field": "b", "from": 1.2, "to": 7, "curve": "exp"},
         {"node": "lp", "field": "q", "from": 0.8, "to": 2.4},
         {"param": "size", "from": 0.2, "to": 0.8}]}
```

- `value` is where it sits; `default` (the agent's setting, kept for reset)
  defaults to `value` the first time you set it.
- `map` entries write one field each over 0..1 (`curve` linear or exp; exp
  needs both ends above 0). `col` multiplies one column of a table field
  (resonator `modes`); `index` / `key` set one element of a list field (a `mix`
  input's `gain`). Mapped fields are overridden: the node's own value for them
  is ignored. Two macros may not write the same field.
- **Map entries reach deeper** (R4-C). A node inside a `voices` patch is
  `"node": "voices_name/patch_node"` (deeper for a `voices` in a patch): the
  write goes to the patch template, so every voice sees it, with no param to
  declare (a typo errors with the patch's node names). A breakpoint list
  (`env` `points`, `curve` `points`) takes `"field": "points", "index": 1,
  "key": "x"` (also `time`, `t`; the first number of the pair) or `"key": "y"`
  (also `value`, `v`). An `expr`'s initial state is `"field": "state", "key":
  "y1"` (the entry must exist). Known gap: the "driven by a macro" warning and
  the window's effective-value display do not look inside patch paths, so an
  edit to a patch field a macro writes is silently ignored.
- **A map entry may write a sound param** instead of a node field:
  `{"param": "size", "from": 0.2, "to": 0.8}` (no `node`, `field`, `col`,
  `index`). A `voices` patch (and its `map`) reads the sound's params, so this
  is one way a knob reaches the inside of a patch (the other is the path form
  above). A map entry naming a patch node without the path is an error that
  says so and shows both: `"voices_name/node"`, or declare a param with
  `sound_ops` `params`, use it in the patch as `"=$name"` (or `$name` in the
  `voices` `map`), map `{"param": ...}`.
- **`claim` is optional.** A macro without one is a plain knob (summary says
  "unclaimed"). `verify` holds it to no measure, but it must still change
  the sound: from 0 to 1, some 10 ms frame in one of the four role bands
  must move by 1 dB or more, else "it does nothing audible" (a crate's
  bounce knob, masked by the boards' tail, moved 0.66 dB). Give a claim
  when the name says what a measure does.
- **Replace a macro** by listing its name in `delete_macros` and in `macros` in
  the same call (the new definition stays; the old one is deleted first).
- **The sweep runs when you create a macro.** `sound_ops` renders each macro you
  made or redefined at 0, .25, .5, .75, 1 and errors with the position and the
  render error if one fails ("macro `m` at 0.75: ... too many events"), so a
  knob that breaks the render at one end is found at once.
- **The name is a claim.** `claim` is `[measure, direction]`: `centroid`
  (Hz), `decay` (ring-out), `crest` (dB), `lufs`, `flatness`, `swing`
  (loudness spread), `inharmonicity` (cents off a harmonic series; clean to
  bell-like), `roughness` (percent of the loudness envelope's energy at
  20-150 Hz; smooth to beating), `density` (events per second at the fine 1 ms
  hop of `listen`'s `density` line: rain, crackle, ticks), `attack` (ms from
  the first sample over -60 dBFS to the first 0.25 ms frame at 90 % of the
  peak: a swell or one hit), `spacing` (s, the median gap between the
  strongest onsets: a bounce, a roll, a footfall pair; 0 under two onsets),
  `ring` (ms, the median over the strong onsets, at least 0.25 of the loudest,
  of each hit's own fall to -20 dB under its peak, stopping at the next onset;
  a hit cut off by the next onset is skipped, none usable falls back to
  `decay`: it falls as a modal `loss` rises and rises with a `decay` node's
  `time`);
  `1` rises over 0..1, `-1` falls. `verify`
  (macros, in the defaults when a sound has any) sweeps each knob over 0, .25, .5, .75, 1
  with the others where they are and requires the measure to move that way at
  every step and no position to peak over -0.3 dBFS, **and that the knob moves
  the measure by at least 2 % of its largest value across 0..1** (one step
  may go against the claim by up to 15 % of the end-to-end move, so a knob
  over a noisy layer, `modal` `noise` or a noise crack, still holds its
  claim: keep the noise, do not lower it to pass) ("the knob
  barely moves X: widen its map, or rename the macro"). A FAIL says to rename
  the macro to what it really does, or fix what it moves. Pick names that
  match: brightness -> centroid up, weight -> centroid down, length -> decay
  up, punch / snap -> crest up, grit / howl -> flatness, gustiness -> swing,
  clang / stiffness -> inharmonicity up, rattle / buzz -> roughness up,
  rate / busyness -> density up, softness / swell -> attack up, spread /
  bounciness -> spacing up, ring / sustain on a multi-hit sound -> ring up.
  Pick the claim that fits the knob: a layer's level (a `gain` before the
  output) -> `lufs` only with a `peak_db` output (else `flatness`, or leave
  the claim off); a filter or brightness knob -> `centroid`; an envelope time
  -> `attack` (a rise) or `decay` (a single hit's fall); event spacing ->
  `spacing` (a few hits) or `density` (any event-rate knob: rain, crackle,
  crumple, bubbles); the ring-out of each hit in a knock, typewriter or
  footfall train -> `ring`. The `density` claim holds for tick trains
  from 10 to about 500 events/s (calibrated on 3 ms ticks; the counter's
  ceiling is near 590 events/s, so a knob that sweeps past it flattens; it
  is an estimate when the ticks overlap, which `listen` says: "density
  estimate"). A `pattern` with `jitter` gives a steadier count than a
  `poisson` of the same rate (its chance clumps move the counter between
  seeds), so use it under a density-claim knob when the human did not ask
  for a random stream.
  **An `output.lufs` target renormalizes the level, so a "loudness" /
  "volume" macro with claim `lufs` cannot move**: use a `peak_db` target (then
  map a `gain` before the output) or claim another measure, or leave the
  claim off. When the whole output fails a claim (a loud layer masks the
  knob, or a multi-hit sound's `decay` jumps between hits), `verify` also
  measures it on the knob's own layer (the node its `map` writes, followed
  down to the `mix`) and passes it if it holds there, saying "measured on
  `node`"; `decay`, `swing` and `crest` stay unreliable on a very short
  sound, so give those a `density`, `attack`, `spacing` or `ring` claim. `flatness` is the
  full-band measure; to check a "high flatness" (grit, hiss) knob by hand read
  `listen`'s whole-sound flatness lines (full-band, and the occupied band, the
  middle 80 % of the energy: a band-limited texture reads low full-band and
  high occupied-band) at the knob's ends with `sweep`; `verify` has no
  occupied-band check yet.
- `game: true` exports the knob in the manifest `inputs` (name, default, doc,
  map) for the game to set at runtime; others are baked at their value.
- A macro **overrides** the fields it maps: editing such a field with `sound_ops`
  changes nothing audible, and the result says so ("WARNING node `amp` field
  `time` is driven by macro `weight`: your value is ignored"). Edit the macro's
  map or value instead. No warning when the node and the macro are made in the
  same call (you wrote both).
- The human's moves arrive in `summary` ("brightness 0.50 -> 0.72"). Keep
  them: re-issue the macro with `value` (and so `default`) set to where they
  left it, or re-centre its range. Moves are not history steps.
- `listen sweep: {param: "brightness", values: [0, .5, 1]}` tiles the knob.

## Senses

- `listen`: one PNG (log-frequency spectrogram, waveform with peak and RMS
  envelopes, loudness curve; seeds / sweep values tile as labelled columns, 8
  at most) and the descriptor table, one line each: length, level, time
  (attack, decay -20 / -60 dB), the headline swell/attack, onsets (40 ms
  merge and a fine 5 ms count), pitch, events strong / weak, spectrum
  start / middle / end and whole-sound (occupied-band flatness), energy
  shares by role band, centroid and pitch by tenth, the 8 octave x 5 segment
  dB table (the main "what does it sound like"), modes, stereo, density, the
  onset table, and aliasing / inharmonicity / modulation / roughness. Every
  line is explained in `docs/senses.md`, "The descriptor table". `diff:
  true` shows the previous render and a dB-difference spectrogram; `zoom
  {t: [a, b], hz: [lo, hi]}` magnifies a window, and the text then
  describes that window (times count from `a`); `descriptors: false` is the
  image alone.
- `heard_as` (a line in `listen`, on when a model is set up; `heard_as:
  false` skips it): the five closest of about 100 sound descriptions
  (`tools/hearing/vocabulary.txt`) an audio-text model
  (`laion/larger_clap_general`, run by `tools/hearing/heard_as.py`) finds
  for each column, and where the spec's brief ranks among them. **A hint,
  not a check:** it called the crate drop the listener rated great "a snare
  drum" and ranked its brief 83 of 102, while the approved door slam's brief
  ranks 1. Read it as "what a stranger might mistake this for". The worker
  starts on the first `listen` (about 4 s) and its Python is
  `SOUNDER_HEARING_PYTHON`, else `~/.local/share/sounder-hearing/venv`;
  without one the line says the hint is off. `SOUNDER_HEARING=off` turns it
  off.
- `listen reference: "file.wav"` compares with one recording (aligned at the first onset, loudness-matched): an STFT distance in dB (about 10 dB is the spread between real recordings of one event: a coarse gate, not a judge) and the biggest band x time differences in words. `listen reference_set: "door_slam"` compares with a folder `<project>/refs/<set>/` of at least two WAVs: the distance to each, the set's own spread, and eight features (weight, body, crack, crest, tail, centroid, voiced, ringing) against the span the recordings cover; `verify expect reference_set {set, [max_distance]}` checks them. Details and measurements: `docs/senses.md`.
- `verify` / `expect` checks: `peak_db` `true_peak_db` `lufs` `duration`
  `silent_edges` `clicks` `dc` `finite` `loop_seam` `bands` `pitch` `onsets`
  `variations` `rebuild` `reference` `reference_set` `macros` `layers` `spec`, and the synthesis checks
  `aliasing` `inharmonicity` `modulation` `roughness`. Each line is PASS /
  FAIL with the measured value; a FAIL says what to change. Four ways to ask:
  - no arguments: the defaults (`finite`, `peak_db` max -0.3, `silent_edges`
    (a loop: `loop_seam`), `dc`, `rebuild`, and `macros` when the sound has
    knobs).
  - `checks: ["clicks"]` adds checks to the defaults; `only: ["clicks", "dc"]`
    runs just those. Only checks that **need no settings** can be named here:
    `finite peak_db true_peak_db silent_edges clicks dc loop_seam variations
    rebuild macros layers spec` (a bare `variations` runs 4 seeds, a bare `layers`
    the `impact` preset). A check that needs settings errors and shows the `expect`
    form to use.
  - `expect` (verify's `expect`, `sound_ops`'s `expect`, or the same fields at
    verify's top level) takes the checks with settings; given alone, only
    those run: `{"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1,
    "max": 3}, "lufs": {"min": -20, "max": -14}, "peak_db": {"max": -3},
    "clicks": true, "pitch": {"hz": 440, "tol_cents": 25}}`. `onsets` takes
    `spacing_ms` (default 40, the descriptors' merge distance, so a fast
    double like a `switch` click or a 200/s tick train reads as one): under
    30 counts with a fine detector above 1 kHz (`spacing_ms: 5` counted a 200/s
    click train within 10 %), over 40 merges more. `onsets` also takes
    `gap: [lo, hi]` (seconds): every gap between consecutive onsets, at the
    `spacing_ms` resolution, must lie in the range, and a FAIL lists the
    offending gap and its times. "Exactly 3 hits 0.15-0.3 s apart" is
    `{"min": 3, "max": 3, "gap": [0.15, 0.3]}`; leave 10 ms of margin on the
    gap bounds (onset times are measured, not exact).
  - **`verify {seeds: N}`** (2..8) re-renders the sound with seeds 1..N (the
    document's seed is replaced for those renders only; nothing is saved) and
    runs the same checks on each; the report groups results by seed ("seed 3:
    loop_seam FAIL ..."). A plain `verify` renders only the seed in the
    document, so a loop or a random texture that passes at seed 1 can still
    fail at another: run `seeds: 4` before calling it finished. `variations`
    (bare, or with settings) is a different check: it compares the seeds with
    each other.
  - Settings of the rest: `bands [{band: "1k" (or Hz), segment 0..4, min_db,
    max_db}]`; `variations {seeds 6, lufs_spread 1, peak_spread_db 1, min_distance 1}`
    (it compares the quantity the output normalizes: the sample-peak spread,
    `peak_spread_db`, when the output uses `peak_db`; the loudness spread,
    `lufs_spread`, otherwise; the other is only reported; the closest pair of
    seeds must also differ by `min_distance` dB of STFT distance);
    a wrong shape (`"variations": 5`) errors with the check's name and its
    fields, as `dc` and `aliasing` do;
    `aliasing true` or `{max_db -40}` (it needs a steady pitch: on a sweep
    it fails "no steady pitched segment", so leave it off a laser or a
    riser); `inharmonicity {min, max}` cents (a
    harmonic tone reads under 2, a bell tens); `modulation {rate_hz, tol_hz
    0.5, min_depth (1 dB or 10 cents), mode "amplitude" | "pitch"}`;
    `roughness {min, max}` percent; `dc {max}` dB; `reference {file,
    max_distance}`.
  - **`layers`**: is it a designed sound? `{"preset": "impact"}` (or bare in
    `checks`) for a hit, slam, knock or blow, `{"preset": "footstep"}` for a
    step (weight 20-70 %, body 10-60 %: a floor's mode under 200 Hz counts
    as weight; crest at most 22), runs seven measures: `weight`
    15-60 % of the energy under 200 Hz, `body` 30-80 % at 200 Hz-2 kHz,
    `crack` at least 1.5 % above 2 kHz, `voiced_max` 0.3 (pitch tracker),
    `ringing_max` 1 (partials within 6 dB of the strongest that take over
    0.6 s to fall 60 dB, and not longer than 1.5 x the sound: a longer estimate
    is a misread), `crest_db` at most 21 over the active frames (10 ms frames
    within 40 dB of the loudest: silence between hits is not counted), and `dry` (no `reverb` or
    `convolve` node, patches included). A field given beside the preset
    replaces the preset's (`tail_ms {min, max}`, the time to -20 dB, has no
    preset value). Each FAIL names the role and the layer to add. The
    approved slam and the real recording pass; the round-0 and tuned-drum
    builds fail (`tests/it/layers.rs`). Ringing metal (a pipe, a sign, a bell)
    fails the preset's `ringing_max` and crest by design: give
    `{"preset": "impact", "ringing_max": 6, "crest_db": {"max": 30}}` or skip
    `layers` for it. Not a default: a UI blip or a drone has
    no weight or crack. `summary` warns when 80 % of a oneshot's energy sits
    in one role band, and `listen`'s energy shares line names the roles.
  `aliasing`, `inharmonicity`, `modulation` and `roughness` cannot be named in
  `checks` / `only`: put them in `expect`. `reference {file, max_distance}` fails
  when the STFT distance to the WAV (aligned and loudness-matched as in
  `listen reference`) exceeds `max_distance` dB, and then names the biggest
  band differences. `variations` needs a node that reads the seed.
- `extract_modes` fits a WAV's strongest partials as a ready `resonators` node (time-domain peeling; a rebuilt five-mode hit sits 0.69 dB from its original). A noisy recording (a slam) is not modal: the tool falls back to an envelope fit and says so ("NOTE the recording is not modal"), a tonal sketch rather than the object's modes. `residual` writes what the modes miss, but sounds are sample-free: use it only to study a recording. Details: `docs/senses.md`.
Warnings in `summary`: a node nothing reads, a frequency above Nyquist, a
silent output, DC over -40 dB, an output whose input is squashed by the
ceiling limiter, a sound that never decays, one that is mostly silence, an
`output.lufs` the render missed (with the numbers and the cause), the default
fade-in cutting a first hit (it suggests `fade_in` 0.0005; not given for
a `fade_in` of 1 ms or less or when the target is met), a field driven by a
macro (your value is ignored). A wood or metal `modal` with no `noise` (clean modes: a tuned instrument).

## Source material: recordings and impulse responses

Synthesis alone gets to a plausible sound; recorded material carries the
irregular detail it lacks. A relative file a node names (`sample` `file`,
`grains` / `cloud` `source`, `convolve` `ir`, `listen` / `verify`
`reference`, `extract_modes` `file`) is looked for in the project first,
then in the **sources folders**: `<project>/assets/sources/`, then
`<library>/sources/` (the path as written, then its file name alone).
`catalog {action: "sources"}` lists every WAV there with its length, rate,
channels, peak and centroid. Rules:

- **Only material the project may use**: your own recordings or CC0 (the
  repo ships none; reference sets for comparison live in `refs/`, which is
  gitignored). Nothing licensed for research only goes into a shipped sound.
- `convolve` resamples an impulse response recorded at another rate to the
  sound's (windowed sinc), so a 44.1 or 96 kHz IR works as is (`convolve`
  of a 44.1 kHz click 10 ms in lands on sample 720 at 48 kHz:
  `tests/sources.rs`).
- Editing a WAV re-renders every node that reads it (`file`, `ir` and
  `source` are keyed on the resolved path and its modification time).
- **Sounds are sample-free.** No recording goes into a sound: no `sample`
  layer, no `extract_modes` residual. Recordings serve as references
  (`listen` / `verify` `reference`, `reference_set`), and `extract_modes`
  without `residual` can read partials off one to rebuild them as synthesis.

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
  `<name>.ogg` (or `<name>_<i>.ogg` for `variations: N`, seeds 1..N) and
  `<name>.sfx.json`. **Ogg Vorbis is the default** (`quality` 0..1, default
  0.5, about 160 kbps stereo; a fixed stream serial, so the same sound gives
  the same bytes; sample-exact, so the loop points hold). `format: "wav"`
  writes WAV instead (`bits` 16 / 24 / 32; `bits` with ogg or `quality` with
  wav is an error): use it for a file you will feed back as a `reference`,
  `sample` or `extract_modes` input, which read WAV only. The manifest has
  `format`, `bits` or `quality`, and per file `bytes`,
  seconds / samples / LUFS / peak, kind, loop
  points, game inputs, baked macros, seeds, causal flag, source catalog entry.
  `variations: N` warns when the files are bit-identical (no node reads the
  seed: add a seeded node, a `pattern` with `jitter`, a `wander`, `poisson`).

## Failures explain themselves

Errors name the node and field and say what to do next: `node #5 (filter
"lp"): field `frq`: unknown field `frq`, expected one of ...`, `macro `weight`
and macro `brightness` both write lp.q: split the field or drop one`. When one
does not, tell the maintainers: `session_log(feedback: "...")`. The friction
log (`<library>/feedback.jsonl`, never sent anywhere) also records every failed
call and slow call; `session_log(report: true)` or `sounder --feedback-report`
folds it into what to fix first.

## Playback

- `play {node?, seed?, loop?, stop?, doc?}` renders the sound (or one node, or
  a re-render with `seed`) and sends it to the speakers; it answers with one
  line ("playing `laser` (0.40 s) on <device>; loop"). `loop` defaults to
  true for `kind: loop` sounds. `stop: true` stops. A new `play` replaces
  what is playing.
- Speakers exist in the window (`sounder`) or with `sounder --headless
  --audio`. Without them `play` errors ("play needs a speaker ...") and you
  use `listen`. A build without the `audio` feature rejects `--audio`.
- The window seeks and loops spans through the same `Player`
  (`src/player.rs`): `Engine` (pure, tested: resampling to the device rate,
  sample-accurate loop wrap, output limiter at +-1) behind `PlayerControl`
  (`play stop seek position playing set_event_handler`); `Player` owns the
  cpal stream on its own thread and reports `PlayerEvent`s.
- Honest limit: the human hears it, you do not. `play` succeeding means the
  samples were sent, nothing about how they sound.

## Changing the tool: where things go

Name files for what they are, never for the task that made them: no work
package, round, phase or `_fix` in a file name, a module name or a comment. A
comment may say why a number is what it is ("a listener heard 0.1 as boiling
oil"), not which task set it.

| Adding | Goes in |
|---|---|
| a node | its family's file in `crates/sfx_graph/src/nodes/` (a new file only for a new family) |
| a node's tests | the family's module in `crates/sfx_analysis/tests/it/` (see `tests/README.md` there); helpers from `it/common.rs` |
| a node's reference (fields, limits, measurements) | `docs/nodes/<family>.md`, and a short entry in `docs/nodes/README.md` |
| a change made after listening | a section in that node's doc and tests in that node's module, not a new file |
| a rule or recipe every agent needs | this file, kept short; the MCP `instructions` string in step |
| notes on how something was built, a plan, a round report | `docs/history/` (summaries only) |
| a part every project should have | `parts/<name>.json` and its line in `src/parts.rs`; parts for one project are `catalog save` with a `role` |
| a script you will run again | `tools/`; a one-off script stays out of git |

- **Not in git:** generated audio and images, exports (`assets/`), the sound
  library and session (`.sounder/`), reference recordings (`refs/`), raw
  per-sound logs, feedback or score dumps, experiment sounds (`*_old`,
  `*_new`, `ab_*`). Use the scratch directory for those.
- **This file stays under about 70 KB.** A new node gets a row in "Which
  ready-made node for which sound" at most (a niche one goes in
  `docs/nodes/README.md` "More ready-made nodes by brief" instead); its detail
  goes in `docs/nodes/`.
- **Tests:** extend an existing module before adding one. Do not copy a helper
  into a module when `it/common.rs` has it or could. A measured limit belongs
  in the node's doc, not in an `#[ignore]`d test.
- **A file path written in a doc or comment must exist.** After a rename,
  grep for the old name.
- Before finishing: `cargo test --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo fmt --all`, and `git status` shows
  nothing you did not mean to add.

## Not now

- A **live runtime** for the causal subset (`summary` reports whether a sound is
  fully causal; the loop-capable nodes have a per-sample form already, the rest
  are block-evaluated and would need one). Firewheel and `bevy_seedling` are
  checked out for that later.
- A project **sound style** (loudness targets per category); project-level
  macros across several sounds ("grit" on all the footsteps) belong with it.
