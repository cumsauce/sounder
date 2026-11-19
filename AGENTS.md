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
`--connect`, `--save-shot`), for when the session's tool list is frozen.
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
   `describe_op` lists the 122 node types; `describe_op {kind: "filter"}` gives
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
  -80 dBFS for 50 ms (doubling 2 s up to 30 s; then a warning that it never
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
- **Names that trip people:** `impulse` takes `at`, a number or a list of
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

Families (122 types; groups marked * have a section in
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
- physical models `modal contact friction` (with shapes, `loss`, `tilt`)*,
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
- **Impact, break, step:** `footstep`, `explosion`, `gunshot`, `splash`, or
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

- `catalog {action: "search", role: "body", material: "wood"}` (also `text`,
  `approved`): the library's parts and saved sounds plus the 86 built-in
  parts in `parts/`: the five layers of the approved door slam (`wood_brush`,
  `noise_crack`, `wood_panel_body`, `low_thump`, `wood_knock`), 72 the
  listener rated good blind, and `wooden_crate_drop` (a whole crate landing
  on concrete, built from a spec, rated great), and eight from the metal
  and stone batch: metal heard as metal (`large_pipe_drop`,
  `large_metal_container_hit`, `metal_plate_gong_hit`, `metal_bell_hit`,
  `metal_into_small_glass`) and small stone taps heard as clicks
  (`small_object_taps`, `mouse_click_tap`, `keyboard_button_hit`). Hits and
  layers: bodies in wood, metal, glass, ceramic and plastic, cracks, weights, lead-ins and
  whooshes, tails and debris. Beds and textures (roles `bed`, `texture`):
  fire, storm wind, fan, sparks, crickets, underwater bubbles, liquid drops.
  Water: a splash with bubbles (5 to 30 cm), a deep bubble pop (a big
  splash without its tail).
  Whole sounds: door creak, engine rev, bird, breath, bells, siren, alarm,
  flute, plucked string, twang, laser, error beep. **Not in the library
  yet** (every attempt was heard as something else, rated lower, or not
  rated): a large rock or stone that reads as stone (a slab was heard as
  "hitting a rock", ok only; small rocks read as clicks), a footstep, rain,
  a stream, a large splash (0.6 m was rated bad), an electric motor, a
  creature voice. Build those from
  nodes and say they are unheard. Each of those is named and described by what the listener heard
  (`heard as "..."`), which is often not what it was built as: search by the
  sound you need, not by the node that might make it. Approved ones come
  first. `catalog describe` shows a part's nodes and knobs.
- Use it with a `sound` node: `{"type": "sound", "name": "body", "sound":
  "wood_panel_body", "at": 0.045, "gain_db": 0, "macros": {"size": 0.6}}`. A
  hit part strikes 5 ms after its start; `wood_brush` ends at 50 ms, so the
  hit goes at `at: 0.045`. Parts carry no loudness target, so their levels
  are the balance they were approved at: move them with `gain_db`, then
  `mix`, `finish`, `output` as for any layered sound.
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

**From the round 6 listen (testers' sounds, judged by ear):**
- **Door slam:** `new_sound` with `template: "door_slam"` (the recipe below, approved by ear, ready to tune); keep
  its weighty low layer (band-limited noise under 200 Hz) under the detail:
  detail without it reads light, weight without detail reads like a drum. A
  brief that names a latch adds a `click` (`noise` about 0.75) as the after
  layer; the slam is still the layered recipe, not a click over one `modal`.
- **Footstep:** a step is not an impact. Keep it gentle: `footstep` with
  `weight` 0.25-0.4, wood `ring` under 1, `noise` about 0.35, little or no
  crack; check with `layers` `{"preset": "footstep"}`.
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

**When one node is still right:** ambience and loops (`fire`, `wind`, `water`),
UI blips, a single texture, or a brief that names one ready-made sound
(`footstep`, `explosion`): use it as the body layer and add layers only if the
brief asks for more weight or detail. When a project has reference recordings
(`refs/`), tune the layer levels against them (`verify` `expect reference`):
the distance is a coarse gate (about 10 dB is the spread between real
recordings of one event), not a judge.

A heavy wooden door slam (`slam_b2` in Phase 0; macros: `stick` -> `scr_g`
db, `weight` -> `thd_g` db, `snap` -> `crk_g` db, `knock` -> `knock_g` db):

<!-- recipe 0.8 -->
```json
[
  {"type": "env", "name": "scr_env", "points": [[0, 0], [0.008, 0], [0.02, 1], [0.042, 0.7], [0.05, 0]], "curve": "smooth"},
  {"type": "math", "name": "scr_speed", "op": "mul", "a": "scr_env", "b": 1.4},
  {"type": "friction", "name": "scr_f", "speed": "scr_speed", "pressure": "scr_env", "roughness": 0.85, "grain": 800},
  {"type": "noise", "name": "scr_n", "color": "pink"},
  {"type": "math", "name": "scr_nb", "op": "mul", "a": "scr_n", "b": "scr_env"},
  {"type": "mix", "name": "scr_m", "inputs": [{"input": "scr_f", "gain": 0.6}, {"input": "scr_nb", "gain": 0.5}]},
  {"type": "filter", "name": "scr", "input": "scr_m", "mode": "bp", "freq": 1956, "q": 0.6},
  {"type": "gain", "name": "scr_g", "input": "scr", "db": -10},
  {"type": "noise", "name": "crk_n", "color": "white"},
  {"type": "decay", "name": "crk_e", "time": 0.008, "at": 0.05},
  {"type": "math", "name": "crk_raw", "op": "mul", "a": "crk_n", "b": "crk_e"},
  {"type": "filter", "name": "crk", "input": "crk_raw", "mode": "bp", "freq": 1928, "q": 0.5},
  {"type": "gain", "name": "crk_g", "input": "crk", "db": -3},
  {"type": "noise", "name": "hit_n", "color": "pink"},
  {"type": "decay", "name": "hit_e", "time": 0.012, "at": 0.05},
  {"type": "math", "name": "hit_burst", "op": "mul", "a": "hit_n", "b": "hit_e"},
  {"type": "modal", "name": "panel", "excite": "hit_burst", "material": "wood", "size": 0.5, "loss": 1.69, "modes": 96, "strike_position": 0.31, "gain_db": -10.7},
  {"type": "decay", "name": "body_e", "time": 0.59, "at": 0.05},
  {"type": "math", "name": "body_raw", "op": "mul", "a": "hit_n", "b": "body_e"},
  {"type": "filter", "name": "body_bp", "input": "body_raw", "mode": "bp", "freq": 344, "q": 0.5},
  {"type": "gain", "name": "body_g", "input": "body_bp", "db": 6.35},
  {"type": "noise", "name": "thd_n", "color": "brown"},
  {"type": "decay", "name": "thd_e", "time": 0.44, "at": 0.05},
  {"type": "math", "name": "thd_raw", "op": "mul", "a": "thd_n", "b": "thd_e"},
  {"type": "filter", "name": "thd", "input": "thd_raw", "mode": "lp", "freq": 192, "slope": 24},
  {"type": "gain", "name": "thd_g", "input": "thd", "db": -6.6},
  {"type": "contact", "name": "knock_c", "speed": 1, "mass": 0.3, "material": "wood", "hardness": 0.6, "at": 0.078},
  {"type": "modal", "name": "knock_m", "excite": "knock_c", "material": "wood", "size": 0.54, "loss": 3},
  {"type": "filter", "name": "knock", "input": "knock_m", "mode": "lp", "freq": 3500},
  {"type": "gain", "name": "knock_g", "input": "knock", "db": -7.4},
  {"type": "mix", "name": "dry", "inputs": ["scr_g", "crk_g", "panel", "body_g", "thd_g", "knock_g"], "to": "mono"},
  {"type": "transient", "name": "punch", "input": "dry", "attack_db": 3.1, "sustain_db": -6},
  {"type": "shape", "name": "warm", "input": "punch", "mode": "tube", "drive": 1.5, "oversample": 4, "mix": 0.25},
  {"type": "onepole", "name": "dc", "input": "warm", "mode": "hp", "freq": 30},
  {"type": "eq", "name": "tone", "input": "dc", "bands": [{"mode": "peak", "freq": 200, "gain_db": 5.6, "q": 0.8}, {"mode": "lp", "freq": 10000}]},
  {"type": "output", "name": "out", "input": "tone", "lufs": -28}
]
```

A sword clash (metal, so here the body **should** ring and be inharmonic:
two `modal` bars a non-integer ratio apart; macro: `ring` -> both bars'
`loss`, exp, 1.5 to 0.25, unclaimed: the thump and crack hold the whole-sound
`decay` at the low end):

<!-- recipe 0.6 -->
```json
[
  {"type": "env", "name": "scr_env", "points": [[0, 0], [0.02, 0], [0.04, 1], [0.25, 0.3], [0.32, 0]], "curve": "smooth"},
  {"type": "friction", "name": "scr_f", "speed": "scr_env", "pressure": "scr_env", "roughness": 0.6, "grain": 1800},
  {"type": "filter", "name": "scr", "input": "scr_f", "mode": "bp", "freq": 4000, "q": 0.8},
  {"type": "gain", "name": "scr_g", "input": "scr", "db": -8},
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "decay", "name": "crk_e", "time": 0.012, "at": 0.005},
  {"type": "math", "name": "crk_raw", "op": "mul", "a": "n", "b": "crk_e"},
  {"type": "filter", "name": "crk", "input": "crk_raw", "mode": "hp", "freq": 2500},
  {"type": "gain", "name": "crk_g", "input": "crk", "db": -6},
  {"type": "decay", "name": "hit_e", "time": 0.004, "at": 0.005},
  {"type": "math", "name": "hit_burst", "op": "mul", "a": "n", "b": "hit_e"},
  {"type": "modal", "name": "blade_a", "excite": "hit_burst", "material": "metal", "shape": "bar", "f0": 1750, "loss": 0.6, "tilt": 3, "gain_db": -4},
  {"type": "modal", "name": "blade_b", "excite": "hit_burst", "material": "metal", "shape": "bar", "f0": 2630, "loss": 0.8, "tilt": 3, "gain_db": -8},
  {"type": "noise", "name": "thd_n", "color": "brown"},
  {"type": "decay", "name": "thd_e", "time": 0.15, "at": 0.005},
  {"type": "math", "name": "thd_raw", "op": "mul", "a": "thd_n", "b": "thd_e"},
  {"type": "filter", "name": "thd", "input": "thd_raw", "mode": "lp", "freq": 250, "slope": 24},
  {"type": "gain", "name": "thd_g", "input": "thd", "db": 4},
  {"type": "mix", "name": "dry", "inputs": ["scr_g", "crk_g", "blade_a", "blade_b", "thd_g"], "to": "mono"},
  {"type": "gain", "name": "trim", "input": "dry", "db": -8},
  {"type": "transient", "name": "punch", "input": "trim", "attack_db": 4},
  {"type": "shape", "name": "warm", "input": "punch", "mode": "tube", "drive": 1.5, "oversample": 4, "mix": 0.2},
  {"type": "onepole", "name": "dc", "input": "warm", "mode": "hp", "freq": 40},
  {"type": "output", "name": "out", "input": "dc", "peak_db": -1.5}
]
```

A UI confirm (tonal on purpose: a tick, two rising notes a fifth apart, a short
low body, and an echo as the after-layer instead of a reverb; give each note a
3 ms `env` attack, since a `decay` starting at `at` clicks and the echo repeats
the click; macro: `brightness` -> both notes' `freq`, exp, claim `centroid` 1):

<!-- recipe 0.8 -->
```json
[
  {"type": "noise", "name": "n", "color": "white"},
  {"type": "decay", "name": "tick_e", "time": 0.004, "at": 0.005},
  {"type": "math", "name": "tick_raw", "op": "mul", "a": "n", "b": "tick_e"},
  {"type": "filter", "name": "tick", "input": "tick_raw", "mode": "hp", "freq": 4000},
  {"type": "gain", "name": "tick_g", "input": "tick", "db": -14},
  {"type": "osc", "name": "note1_o", "shape": "tri", "freq": 880},
  {"type": "env", "name": "note1_env", "points": [[0, 0], [0.005, 0], [0.008, 1], [0.3, 0]]},
  {"type": "math", "name": "note1", "op": "mul", "a": "note1_o", "b": "note1_env"},
  {"type": "osc", "name": "note2_o", "shape": "tri", "freq": 1320},
  {"type": "env", "name": "note2_env", "points": [[0, 0], [0.075, 0], [0.078, 1], [0.45, 0]]},
  {"type": "math", "name": "note2", "op": "mul", "a": "note2_o", "b": "note2_env"},
  {"type": "osc", "name": "body_o", "freq": 220},
  {"type": "decay", "name": "body_e", "time": 0.09, "at": 0.005},
  {"type": "math", "name": "body", "op": "mul", "a": "body_o", "b": "body_e"},
  {"type": "gain", "name": "body_g", "input": "body", "db": -12},
  {"type": "mix", "name": "notes", "inputs": [{"input": "note1", "gain": 0.5}, {"input": "note2", "gain": 0.5}]},
  {"type": "delay", "name": "echo", "input": "notes", "time": 0.11, "feedback": 0.25},
  {"type": "filter", "name": "echo_lp", "input": "echo", "mode": "lp", "freq": 3000},
  {"type": "gain", "name": "echo_g", "input": "echo_lp", "db": -12},
  {"type": "mix", "name": "dry", "inputs": ["tick_g", "notes", "body_g", "echo_g"], "to": "mono"},
  {"type": "output", "name": "out", "input": "dry", "peak_db": -3}
]
```

`examples/door_slam.rs` renders the door slam and checks it.

## Which ready-made node for which sound

Round-1 testers built every footstep from primitives and never tried `fire` or
`footstep`: the nodes below exist so that a brief that names one of these
sounds starts from one node (the body layer) plus a few knobs. **Building from primitives is still
fine** (and right for anything not listed: a laser, a UI blip, a whoosh, a
drone); `describe_op {kind}` gives each node's fields and an example, and the
sections below give the measured limits. None of these has been heard: pick by
the brief, then `listen`, `verify` and let the human's macros do the rest.

| Brief | Node | Start with | Fits |
|---|---|---|---|
| **a door slam** | `new_sound` `template: "door_slam"` | knobs `weight`, `snap`, `stick`, `knock`, `latch` (a click latch, off at 0) | the layered slam the listener approved (brush, crack, noise-struck wood body, noise weight, knock, dry bus), copied whole and editable. Two rounds of testers given only the recipe built a click over one `modal` instead, heard as weak: **start from the template**, then tune the knobs |
| a footstep, a walk | `footstep` | `surface` concrete / gravel / grass / wood, `weight`, `pace`, `shoe`, `body` (pitch apart from weight), `swish` (swish against ticks), on wood: `ring` (0.25..2, the board's ring; the default puts 84 % of the energy under 200 Hz and reads slammed, try 0.6 with `weight` 0.25), `noise` about 0.35 (the default board rings pitched, voiced 0.84), `scuff` (the roll between heel and toe) and `creak` (the board under the weight), both sparingly | one heel + toe step with the surface's texture. A walk is several steps: `pattern` + `sound` instances, or a few `footstep` nodes with different `seed` / `at` in a `mix` |
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
| a pipe, a flute, a string, a bow | `tube`, `string` | `length` / `freq`, `excite`, `pressure` | waveguides: winds, plucks, strikes, bowed notes |
| a latch, a switch, a button, a lock, a buckle | `click` | `kind` latch / switch / button / lock / clasp, `size`, `hardness`, `noise` about 0.75 for a small plastic or metal part (the listener's setting; the default rings pitched, voiced 0.33-0.78) | one or two transients through small resonators; a hit at t = 0: `fade_in` 0.0005 |
| a telephone bell, a ringing bell | `bell` | `freq` (750), `at` (strike time or list), `ring` | Farnell's telephone bell: partial groups, a strike and the bell body; 3.6 dB from the patch played by Pd |
| a bouncing ball | `ball_bounce` | `length` (to rest), `gap` (first interval), `pitch` | Farnell's ball: sine thumps that come faster, shorter and quieter |
| a cricket, night insects | `cricket` | `tick`, `pulses`, `period`, `pitch`, `ring` (0.3; lower is more cicada) | Farnell's cricket: chirps of rising pulses ringing narrow bands at 4.5 / 9 kHz |
| a ruler twang, a sprung tongue, "boing" | `twang` | `freq` (overhang pitch), `vibrato`, `ring` | Farnell's twanged ruler: a phasor-driven clamped bar exciting free modes |
| a police siren, an emergency vehicle | `siren` | `rate` (3 = yelp, 0.1 = wail), `low` / `high` Hz, `echo` (the patch's street; 0 dry) | Farnell's siren: a log-shaped oscillator swept through a clipped plastic horn |
| a creaking door, a hinge | `door_creak` | `force` (signal; creaks over 0.3) | Farnell's stick-slip hinge through wood resonances and a square panel |
| a jet engine, a turbine | `jet` | `speed` (signal 0.1..1, spools up through a 0.2 Hz low-pass) | Farnell's jet: turbine tones and a clipped, swept burner roar |
| a sci-fi alarm, red alert | `red_alert` | `at` (whoop times), `pitch` | Farnell's klaxon: a rising buzz through feedback delays and a formant horn |
| thunder, a distant storm | `thunder` | `distance`, `brightness`, `length`, `duration` | foley's N-wave and rumble; `brightness` tilts above 100 Hz, `length` stretches the rumble; a `duration` that fits |
| a struck object: panel, bell, pipe, keys, a door's thud | `modal` fed by `contact`, `impulse` or a noise burst | `shape` plate / bar / tube / membrane / string / shell, `material`, `size`, `thickness`, `loss`, `tilt`; **`noise` for real wood (0.6, `tilt` 4) and metal (0.85, `noise_width` 0.5, `tilt` 6)** (clean modes read as a tuned instrument) | normalized; `size` and `thickness` are pitch knobs (small thin objects: `thickness` 0.0015 instead of a custom material); `loss` 0.5 rings twice as long; `tilt` flattens a one-mode plate |
| a larger-than-life hit (cinematic, trailer, a game's heavy impact) | `subdrop` under it, `riser` into it | `from` / `to` / `decay`, `drive`; `at` (the hit), `length`, `shape` | tonal on purpose: add them to a layered hit only when the brief asks for exaggeration (a realistic slam with a sine sub read as a tuned drum) |
| a robot voice, speech-like texture, band dynamics | `vocoder`, `multiband`, `filterbank` | a pitched `carrier` + a noisy `modulator`; `crossovers` + `bands` | see "Bands and vocoder" in `docs/nodes/README.md` |
| a swarm, a swell, a granular wash | `cloud`, `grains` | `source`, `density` (signal), `size`, `pitch_spread` | grains from any node or WAV |
| a vowel, a creature call | `glottis` into `tract` | `f0` (signal), `tension`, `vowel`, `scale` | `docs/nodes/README.md`, "Foley models" |

Scene sounds layer these: a door slam is the layered recipe in "Layering a
designed sound" (not a `click` latch under a `modal` thud: that read as metal
and as a tuned drum), a campfire is `fire` plus a few `spark`s, rain is
`poisson` into `voices`.
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
- Loops get `loop_seam`: put `crossfade_loop` last, after any filter; `fire`, `wind`, `spark`, `water` render periodic in a `kind: loop` document.
- `modal` takes `size` or `f0` (not both), is normalized, and needs `noise` for real wood (0.6) and metal (0.85) or it reads as a tuned instrument.
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
  claim off. `decay`, `swing` and `crest` read the whole sound:
  they are unreliable on a very short or multi-hit sound (several separate
  hits), so give those a `density`, `attack`, `spacing` or `ring` claim, or
  leave the claim off, rather than rename the knob to fit. `flatness` is the
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

- `listen`: one PNG (log-frequency spectrogram with octave lines, waveform with
  peak + RMS envelopes, loudness curve; seeds / sweep values tile as labelled
  columns, 8 at most, under ~1.15 MP) and the descriptor table, one line each:
  - `length`, `level` (peak, true peak, LUFS, momentary max, RMS, crest, DC),
    `time` (attack as the 10-90 % rise, peak time, decay -20 / -60 dB, and the silence wording:
    leading / trailing silence, or "all silent"; "loop: n/a (see steadiness)"
    for a loop), `swell/attack (the headline)` (ms from the first sound to 90 %
    of the peak of the 10 ms RMS envelope: a swell reads long, a click near
    zero; the 10-90 % `attack` and `attack detail` measure other things),
    `onsets {n} (40 ms merge); fine {m} (5 ms): ...` (the fine count is the one
    `density` is built on; it resolves hits 5 ms apart; the first 40 are listed
    in ms, 20 to a line, with "... N more fine onsets not shown" after; not
    for a loop), `pitch` (YIN, voiced share, harmonicity).
  - `events: N strong (>= 0.5 of the loudest onset), M weak; last event at T s`
    (from the fine onsets by peak strength: clinks against crackle, a hit
    against its chips; it also says what follows the last event; omitted for a
    loop), and when the ticks overlap the `density` line reads "density
    estimate" (see the `density` claim above).
  - `spectrum (start/middle/end)` centroid / rolloff / flatness, then the
    **whole-sound** line (centroid, rolloff, full-band flatness, and flatness
    over the **occupied band**, the middle 80 % of the energy, "none" when that
    band is too narrow: tonal), `energy shares` (<200 Hz, 200-2k, 2k-8k, >8k),
    `centroid by tenth` and `pitch by tenth` (ten values over the sound; "-"
    where none), and when partials are clearly off a harmonic series an
    `inharmonic: partials at ...` line.
  - the 8 octave-band x 5 time-segment dB table (your main "what does it sound
    like"), `modes`, `stereo` (correlation, width).
  - `attack detail` (first sound to 90 % of peak, and to the sample peak, in
    ms), `density` (events a second, from a 1 ms envelope: the count of hits in
    a rain, a crackle or a footstep) and `density by tenth (events/s)` (ten
    values: does a roll speed up, does a crumple thin out), `steadiness` (loops: loudness over 400 ms
    windows, min / max / spread in LU), and the **onset table** (one row per
    onset: time, strength, attack ms, -20 dB ms, -60 dB ms (`>N` when the hit
    is still sounding at the next onset or the end, N ms in), centroid, pitch
    start>end; pitch reads `n/a` for an inharmonic or unvoiced hit, and a
    burst of 8-60 ms shows `glide up a>b` / `glide down a>b` instead; left out
    for loops: read `density`). Beside it: `modes` ends with `(N ignored:
    slower than the sound)` when partials too slow to fit in the sound were
    left out; `inharmonic: partials at ...` comes from partial *ratios* (the
    separate `inharmonicity:` line below still uses the older pitch-guess
    method, so the two can disagree: trust the ratios for "is it bell-like");
    `closely spaced events: N pairs within 20 ms (at ... ms)` flags double
    hits (a latch, a bounce's end) that the 40 ms onset merge hides.
  - four lines for synthesis checking: `aliasing` (energy off the harmonic grid
    above 4 kHz, relative to the total, dB; n/a without a steady pitched
    segment), `inharmonicity` (cents, mean over the strongest 8 partials; n/a
    under 3 clear partials), `amplitude modulation` and `pitch modulation`
    (the strongest rates and peak-to-peak depths: Hz, dB / cents), `roughness`
    (percent of the loudness envelope's energy at 20-150 Hz).
  `diff: true` shows your previous render of the same request, this one, and
  their dB-difference spectrogram (this minus previous; diverging colours:
  blue = quieter now, red = louder, grey = same, saturating at +-24 dB, the
  scale is drawn). `zoom {t: [from_s, to_s], hz: [low, high]}` (seconds and
  Hz; either key alone; a bad shape errors with this) magnifies an attack or
  a band; with `zoom {t: [a, b]}`
  the descriptor text describes that window too (the line "zoom window a-b s:
  the numbers below describe this window; times count from a" says so), so
  onset, fine-onset and peak times count from `a`. `descriptors: false`
  returns the image alone.
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
- `listen reference: "file.wav"` (relative to the project or a sources folder, or absolute): the
  reference is resampled to the sound's rate, moved so its first onset (30 dB
  under its peak) meets the sound's, and scaled to the sound's integrated
  loudness (the shift and the gain are reported). The image is [sound,
  reference, difference]; the text gives the multi-resolution STFT distance
  (windows 256 / 1024 / 4096, dB, 0 = identical), the 5 biggest band x time
  cells that differ by 3 dB or more in words ("0-20% of the sound: 2.8-5.7 kHz
  is 31 dB quieter than the reference"; segments are fifths of the sound), and
  length / attack / decay / centroid differences. A reference with nothing
  above its Nyquist counts the sound's top end as a difference (it says so).
  Not with seeds, sweep or diff. This is the distance, not a taste verdict.
  Reading the picture: x is time, y log frequency, colour dB from -90 (black)
  to 0.
- `listen reference_set: "door_slam"`: a **reference set** is a folder of
  recordings of one kind of sound, `<project>/refs/<set>/` (at least two
  WAVs; a path with a `/` names any folder). One recording is a poor target:
  in Phase 0 a real door slam sat 10.4 dB from two other real slams, further
  than a good synthetic one, so a distance below about 10 dB ranks nothing.
  A set gives the distance to each recording, the nearest against the set's
  own spread (the mean distance between its recordings), and eight features
  against the span the recordings cover: `weight` / `body` / `crack` energy
  (under 200 Hz / 200 Hz-2 kHz / above 2 kHz), `crest`, `tail` (to -20 dB),
  `centroid`, `voiced`, `ringing` partials. A feature passes inside [min,
  max] widened by a quarter of the span, at least a floor (3 % for shares,
  1 % crack, 3 dB crest: the crest of a noise hit moves several dB with the
  seed, 20 ms tail, 10 % centroid, 0.05 voiced). The image compares with the
  nearest. `verify expect reference_set {set, [max_distance]}` reports each
  feature (`reference_set weight` ...) and the nearest. Measured against
  three CC0 door slams (406635, 571797, 724124; they sit 10.06 dB from each
  other): the round-0 build is outside on weight (86 % against 12-39 %),
  body, tail, centroid and voiced, and 13.3 dB from the nearest; the
  layered build the listener called a tuned drum is outside on crest (28.3
  dB against 16-24), centroid and ringing (3 against 0-2); the approved one
  is inside on all eight, 6.9 dB from the nearest. Limits: a few recordings
  make a thin range; the ringing count reads noise partials in band-passed
  noise (a synthetic noise set spanned 0-4), so it catches less against
  noisy sets; sets are for comparison only (keep `refs/` out of git and out
  of shipped sounds unless the licence allows).
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
    `aliasing true` or `{max_db -40}`; `inharmonicity {min, max}` cents (a
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
    preset value). Each FAIL names the role and the layer to add. Measured on
    the Phase 0 door slams (`tests/it/layers.rs`, fixtures in
    `tests/fixtures/phase0/`): the round-0 build fails weight (86 %), body
    (5.7 %) and crest (24.9 dB); the first layered one, which the listener
    heard as a tuned drum with a reverb tail, fails ringing (75, 329, 457 Hz
    at 0.65-0.98 s), crest (28.3 dB) and dry; the approved one passes, and
    so does the real recording (38.5 / 55.9 / 5.6 %, voiced 0.15, one
    partial ringing, crest 21.4 dB). Not a default: a UI blip or a drone has
    no weight or crack. `summary` warns when 80 % of a oneshot's energy sits
    in one role band, and `listen`'s energy shares line names the roles.
  `aliasing`, `inharmonicity`, `modulation` and `roughness` cannot be named in
  `checks` / `only`: put them in `expect`. `reference {file, max_distance}` fails
  when the STFT distance to the WAV (aligned and loudness-matched as in
  `listen reference`) exceeds `max_distance` dB, and then names the biggest
  band differences. `variations` needs a node that reads the seed.
- `extract_modes` fits each partial in the time domain (candidates from a long
  spectrum, then peeling: demodulate the recording minus the other modes,
  line-fit the log envelope and the phase), so decays of fast modes, close
  pairs and the gains are good. A gain is the amplitude the mode has at the
  hit in the recording's own scale: a unit `impulse` into the node reproduces
  the recording's level. Measured (`tests/it/reference.rs`): a rebuilt five-mode
  hit with a noise tick sits 0.69 dB (STFT distance) from its original and
  within 0.1 dB in level; a hard case (30 ms and 20 ms modes, a pair 25 Hz
  apart) 1.21 dB; a rebuild with the modes shifted 10 % sits 13 dB away.
  `residual: "name.wav"` (a file name) also writes the **residual** into the
  sources folder: the recording from 5 ms before its onset minus the modes
  as the `resonators` play them from an impulse at that time. It returns the
  rebuild (`impulse` at the lead-in time, the `resonators`, a `sample` of the
  residual from 0, a `mix`), which sums back to the recording: on the
  five-mode hit 0.22 dB from the original, against 4.85 dB for the modes
  alone (2 modes: 0.22 against 10.44 dB; `the_residual_restores_what_the_modes_miss`).
  The residual holds the hit's noise, rattle and room, and also what the
  fitted partials get wrong, so retuning the modes later leaves their old
  partials in it: keep the modes' pitch and edit their decay and level, or
  re-extract. A recording with under 5 ms before its hit needs the output's
  `fade_in: 0.0005` (the tool says so). **Noisy recordings** (a slam, a
  noisy hit) are not modal: peeled one by one, their partials grow to cancel
  each other (gains in the thousands on band-passed noise peaking at 0.9),
  and the `resonators`, which start every partial at the impulse, would play
  them far louder than the recording (a real door slam: 46 dB). Two guards:
  each peeled amplitude is capped at the recording's peak, and when the
  peeled partials still play over 6 dB louder than the recording the tool
  falls back to an **envelope fit** and says so ("NOTE the recording is not
  modal"): its strongest spectral peaks at least 15 % apart, each with the
  decay of the recording's own envelope around it, levelled so the rebuild
  peaks at the recording's peak. That is a tonal sketch, not the object's
  modes: the real door slam gave 8 peaks at 89-424 Hz, modes alone 16.75 dB
  from the recording (no crack above 2.8 kHz), modes + residual 0.37 dB.
  Band-passed noise: 0.0 dB over its peak, against 85 dB before
  (`a_non_modal_recording_gets_an_envelope_fit`); a clean struck object keeps
  the modal fit (0.2 dB). The residual of an envelope fit also carries the
  sketch's negative (a peak of 1.10 against the noise's 0.89), so change the
  sketch's decays and the residual stops cancelling them.

Warnings in `summary`: a node nothing reads, a frequency above Nyquist, a
silent output, DC over -40 dB, an output whose input is squashed by the
ceiling limiter, a sound that never decays, one that is mostly silence, an
`output.lufs` the render missed (with the numbers and the cause), the default
fade-in cutting a first hit (it suggests `fade_in` 0.0005; not given for
a `fade_in` of 1 ms or less or when the target is met), a field driven by a
macro (your value is ignored).

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
  ready-made node for which sound" at most; its detail goes in `docs/nodes/`.
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
