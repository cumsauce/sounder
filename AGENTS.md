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
   Look at "Which ready-made node for which sound" first: a brief that fits a
   ready-made node is one node and a few knobs, not a rebuilt graph.
   `describe_op` lists the 109 node types; `describe_op {kind: "filter"}` gives
   one's example and schema (`type` and `node` are aliases of `kind`). To
   remove a node put `{"type": "delete", "name": "x"}` in `ops` (one item per
   node; a top-level `delete` is not a field and errors).
3. `listen`, `verify`, fix, repeat.
4. Add **macros** (knobs) so the human can fit the sound by ear: at least one
   on every sound, unasked (`summary` reports "macros: none yet" until there is
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
| `sound_ops` | atomic batch: `ops` (node upserts; `{"type": "delete", "name": ...}` removes), `params`, `macros`, `delete_macros`, `kind`, `duration` (null or 0 = automatic length again), `seed`, `sample_rate`, `expect`; renders for real; one history step if anything changed |
| `listen` | the image + descriptor table; `node`, `seeds`, `sweep {param, values: [..]}`, `zoom {t: [a, b], hz: [lo, hi]}`, `diff`, `reference` (a bad `zoom` / `sweep` error shows the accepted shape) |
| `verify` | PASS / FAIL checks (`checks`, `only`, `expect`; see Senses); any FAIL is an error |
| `describe_op` | node types, one line each; or one type's example + schema (`kind`, aliases `type` / `node`) |
| `extract_modes` | strongest partials of a WAV as a ready `resonators` node |
| `play` | plays on the speakers for the human (the window, or `--headless --audio`); a plain headless server answers with an error saying so |
| `new_sound` | opens a sound beside the others (`kind`, `duration` 0..600 s, `seed` can be set at once, so a loop is one call; a loop with no `duration` is an error naming the field); an open name is refused |
| `catalog` | `list describe save load delete close`; the sound is `name` or `doc` (both given and different is an error) |
| `export` | WAVs + `<name>.sfx.json` manifest |
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

Families (109 types; groups marked * have a section under "More nodes, by
group" below):

- sources: `osc noise impulse sample` and `fm wavetable`*; control `lfo wander
  sample_hold slew ramp` and `chaos fbm curve switch pitch_follow
  centroid_follow`*; envelopes `env adsr decay follower` and `trigger_env
  event_gate seq notes`*; maths `math remap gain`, `expr`.
- filters `filter onepole comb allpass resonators` and `ladder eq filterbank
  vocoder multiband`*; time `delay convolve`, `reverb` (models)* and `ir`*;
  nonlinear `shape compress limit` and `ringmod transient gate sub exciter`*;
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
  elements `fire wind spark`*, machines `engine motor`*, blasts `explosion
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

## Which ready-made node for which sound

Round-1 testers built every footstep from primitives and never tried `fire` or
`footstep`: the nodes below exist so that a brief that names one of these
sounds is one node plus a few knobs. **Building from primitives is still
fine** (and right for anything not listed: a laser, a UI blip, a whoosh, a
drone); `describe_op {kind}` gives each node's fields and an example, and the
sections below give the measured limits. None of these has been heard: pick by
the brief, then `listen`, `verify` and let the human's macros do the rest.

| Brief | Node | Start with | Fits |
|---|---|---|---|
| a footstep, a walk | `footstep` | `surface` concrete / gravel / grass / wood, `weight`, `pace`, `shoe`, `body` (pitch apart from weight), `swish` (swish against ticks) | one heel + toe step with the surface's texture. A walk is several steps: `pattern` + `sound` instances, or a few `footstep` nodes with different `seed` / `at` in a `mix` |
| fire, a campfire, crackle | `fire` | `size`, `intensity`, `crackle`, `loop` | roar + hiss + Poisson pops; loops by itself in a `kind: loop` document |
| wind, a gale, howling | `wind` | `speed` (signal), `gust`, `whistle`, `loop` | five moving bands (+ an Aeolian tone); a `ramp` on `speed` is a rising gale |
| electric arcs, sparks | `spark` | `rate`, `buzz`, `body` | mains-gated crackle into a short resonator |
| a stream, a brook, rain on water | `water` | `flow` (signal), `density`, `splash`, `depth` | a Minnaert bubble population plus hiss; a loop in a loop document |
| a splash, something dropped in water | `splash` | `size` (m) | bright, brief impact burst, then a bubble cloud whose pitch falls (the centroid falls through the sound); a transient: use `peak_db` |
| a single drip, a blip of water | `bubble` | `radius_mm`, `jitter`; in `voices` `map` a `$rand` radius | one Minnaert bubble: drips, water_drips example |
| cloth, foliage rustle | `cloth` | `motion` (signal), `weight` | power-law rustle bursts in four bands; shape `motion` with a `wander` or `ramp` |
| crumpled paper, foil, plastic, dry leaves | `crumple` | `density` (signal, bursts/s up to 400), `size`, `hardness` | band-passed noise bursts whose count follows `density` (cloth's saturates); hardness 0.2 paper, 0.85 foil; `peak_db` for a sparse one |
| rolling, scraping, sliding | `rolling`, `friction` | `speed` (signal), `roughness` | an excitation: put it through `modal` (a scale first: `rolling` is loud at high speed) |
| breaking glass, shards, debris | `shatter` into `voices` | `pieces`, `spread`, `exponent`; a small `modal` per fragment | a crack then power-law fragments; `modal` glass with `loss` and `tilt` per shard |
| explosion, gunshot, blast | `explosion`, `gunshot` | `size`, `distance`; `caliber`, `barrel`, `suppressed` | finished one-shots, start 2.5 ms in (the default fade-in does not cut them); distance delays and darkens |
| engine, motor, vehicle | `engine`, `motor` | `rpm` (a `ramp` or `wander`), `cylinders` / `poles`, `load` | rpm-driven pitch; set a `duration` (ramps hold) |
| a pipe, a flute, a string, a bow | `tube`, `string` | `length` / `freq`, `excite`, `pressure` | waveguides: winds, plucks, strikes, bowed notes |
| a latch, a switch, a button, a lock, a buckle | `click` | `kind` latch / switch / button / lock / clasp, `size`, `hardness` | one or two transients through small resonators; a hit at t = 0: `fade_in` 0.0005 |
| thunder, a distant storm | `thunder` | `distance`, `brightness`, `length`, `duration` | foley's N-wave and rumble; `brightness` tilts above 100 Hz, `length` stretches the rumble; a `duration` that fits |
| a struck object: panel, bell, pipe, keys, a door's thud | `modal` fed by `contact`, `impulse` or a noise burst | `shape` plate / bar / tube / membrane / string / shell, `material`, `size`, `thickness`, `loss`, `tilt` | normalized; `size` and `thickness` are pitch knobs (small thin objects: `thickness` 0.0015 instead of a custom material); `loss` 0.5 rings twice as long; `tilt` flattens a one-mode plate |
| a robot voice, speech-like texture, band dynamics | `vocoder`, `multiband`, `filterbank` | a pitched `carrier` + a noisy `modulator`; `crossovers` + `bands` | see "Bands and vocoder" |
| a swarm, a swell, a granular wash | `cloud`, `grains` | `source`, `density` (signal), `size`, `pitch_spread` | grains from any node or WAV |
| a vowel, a creature call | `glottis` into `tract` | `f0` (signal), `tension`, `vowel`, `scale` | "Foley models" |

Scene sounds layer these: a door slam is a `click` (the latch) under a `modal`
thud, a campfire is `fire` plus a few `spark`s, rain is `poisson` into `voices`.
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

`spectral` also has `shift stretch smear hps` and `pitch_shift` has `formant`
(see "Spectral additions" under "More nodes, by group").

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
| `bounce` | `first` (seconds from the first impact to the second), `restitution` (0..1 exclusive), `[min_interval 0.004]`, `[start]`, `[jitter 0..0.5]` (each hit after the first moves by up to jitter times the gap before it, seeded; 0 draws nothing, so old renders hold); gap k is `first * restitution^k`, impact k has value `restitution^k` |
| `burst` | `count`, `spread` (first to last event), `[curve]` timing density (linear even, exp dense early, smooth dense at both ends), `[at]` |
| `on_threshold` | `input` (mono audio), `level`, `[hold 0.01]`: an event at each rising crossing (interpolated time), value = the signal there; a signal that starts above `level` fires at 0 |
| `grains` | `source` (a node or a `.wav`), `events`, `size` s, `[position 0..1 or signal]` where each grain starts reading, `[pitch]` ratio, `[window hann\|tri\|rect]`, `[pan_spread 0..1]` (stereo, random pan per grain), `[jitter]` (+- of the source length on the position) |

`size` and `pitch` also take a signal (read at each grain's start) or a range
`{min, max, [curve]}` (a value drawn per grain); `shatter` is another event
generator and `cloud` makes grains from a density signal with no stream (see
"Grains and clouds" under "More nodes, by group").

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
  seam check has two halves: the wrap sample against the loop's own roughness
  (a jump or a slope kink; at most 2x the 99th-percentile step) and the
  third-octave band levels of the last and first 0.34 s (at most 6 dB apart).
  **A noise loop does not always pass** (round 0: the spectral half fails when
  the colour, a `wander`ed filter or a sparse crackle differs between the
  two ends), and a smooth loop (a drone, an lfo) must be periodic in the
  duration (every lfo / oscillator frequency a multiple of 1 / duration).
  What fixes it: `crossfade_loop` at the end of the chain (fixes the wrap jump
  always; the spectral half unless the spectrum changes faster than about
  every 2 s: a longer `fade` helps); `fire`, `wind`, `spark`, `water` take
  `loop: true` (the default in a `kind: loop` document) and render periodic in
  the duration; for hand-built textures make every `wander` / `lfo` slow or
  periodic. Keep control signals (`intensity`, `speed`, `flow`) periodic too.
  **A filter (or any recursive node: `onepole`, `comb`, `allpass`, `delay`, `reverb`,
  `ladder`, `eq`, `chorus`, `flanger`, `phaser`) after a looped source starts from zero state, so its output is not
  periodic at the wrap and can break the seam**, though the source alone passes
  (`water` -> `filter` hp 250 -> `filter` lp 6 kHz failed `loop_seam` in 35 % of
  480 cases at 3 s; with `crossfade_loop` `fade` 0.25 after the filters, 1 of
  480). The render now WARNS ("filters the looped source ... add a
  `crossfade_loop` after it"): put `crossfade_loop` last, after the filter. A
  `splash` in a short loop is sparse and random, so one end window may hold a
  splash and the other not and the spectral half of `loop_seam` fails by chance
  (about 17 % at `splash` 0.5; raise `flow`, or crossfade). `water` alone is
  periodic: 0 of 480 failures at 1 s and 3 s over seeds, flow and density
  (`docs/nodes/R3-E.md`).

## Physical models: modal, contact, friction

```json
{"type": "contact", "name": "blow", "speed": 2, "mass": 0.1, "material": "wood", "hardness": 0.7}
{"type": "modal", "name": "panel", "excite": "blow", "material": "wood", "size": 0.5, "strike_position": 0.3}
{"type": "friction", "name": "hinge", "speed": "swing", "pressure": 0.8, "roughness": 0.3}
```

- `modal` {excite, material, **`size` or `f0`** (one of the two), [strike_position
  0.3], [modes 48], [loss 1], [tilt 0], [thickness], [f_min], [f_max], [seed]}:
  a simply supported square plate `size` metres a side. `material` is a foley
  name (`stone wood metal glass ice earth granular plant soft snow`) or
  `{young, density, poisson, loss, thickness}`; the `thickness` field
  (metres, 0.0001..0.5, read once) overrides the material's own, so a small
  thin object needs no custom material (every mode frequency goes linearly
  with it; giving the material's own value changes nothing). Mode Hz = pi/2 * thickness *
  c_L / sqrt(12) * (m^2 + n^2) / size^2, so pitch goes as 1 / size^2 and a
  plate's thickness sets its register (wood 2.5 cm, size 0.5: lowest mode
  ~425 Hz; glass 1 cm, size 0.5: ~200 Hz). T60 per mode = ln(1000) / (pi f
  loss): high modes die first, metal (loss 0.004) rings several times longer
  than wood (0.025). `strike_position` weights each mode by
  |sin(m pi x0) sin(n pi y0)| (0.5 is the centre: modes with a node there
  vanish); a seeded 1.5 % mistuning breaks the symmetry. `size`,
  `strike_position` are read once (at the start), so `"$size"` / `"=expr"`
  work (a `voices` patch can randomize them) but a ramp will not glide them.
  The `excite` signal is the strike: modal is linear, so a `contact`
  pulse, an `impulse`, a noise burst or a `friction` stream all work.
  Stereo excite rings each channel alone.
  **`f0`** (Hz, 20..18000, a number, `"$param"` or `"=expr"`, read once) sets
  `size` so the lowest mode of the shape's table sits at `f0` ("lowest mode 2
  kHz" is one field; measured within 0.22 % for a plate, 0.04-0.10 % for the
  other shapes, in `tests/m7.rs`). It replaces `size`: giving both is an error,
  giving neither is an error. The seeded mistuning can move the lowest mode by
  up to 1.5 % (plate). A macro that maps `f0` must not also map `size`.
  **`f_min` / `f_max`** (Hz, default none) keep only the modes inside the band,
  applied after the mode table and before the `modes` cap (so `modes` counts
  kept modes); the strongest kept mode is the level reference. An empty band is
  an error naming the table's range ("widen the band, or change `size` / `f0` /
  `thickness`"), and so is a band whose modes the strike does not excite (a
  node at `strike_position`). With neither field the output is bit-identical to
  before. Use them for glass, ice, coins and keys: `f0` where the object rings,
  `f_max` to keep the top of the table out. Detail: `docs/nodes/R3-C.md`.
  **`modal` normalizes its output**: a unit impulse (1.0 at one sample, or a
  `contact` of mass x speed = 0.1) peaks near 0 dBFS (measured within 0.01 dB
  over 10 materials x 6 shapes x 3 sizes); a stronger strike is louder. It has
  `shape` (`plate bar tube membrane string shell`) and `stiffness`; the lowest
  mode falls with size (the `modes` bank always keeps the lowest half).
  See "Physical models and recipes" below for the other shapes.
  **`loss`** (0.01..100, default 1) multiplies the material's loss, named or
  `{young, ...}`: `"material": "wood", "loss": 0.5` rings twice as long (the
  20 dB decay time measured 1.95x), 2 half as long; a mode's T60 is capped at
  20 s. **`tilt`** (dB per octave, -12..12, default 0) multiplies each kept
  mode's gain by `10^(tilt log2(f / f_lowest) / 20)`: positive lifts the upper
  modes (a glass plate at `strike_position` 0.31 puts 98.6 % of its energy in
  the lowest mode; `tilt` 6 brings that to 91 %, 12 to 57 %), negative
  darkens; the unit-impulse level stays near 0 dBFS. **`size` range**: the
  lowest mode must fall between 24 Hz and 90 % of the top of the band (18 kHz
  at 48 kHz), which makes 3 mm glass usable from 0.029 m to 0.78 m and a metal
  plate from 0.12 m; outside it the error gives the range, and a size that
  leaves no mode in band says which thickness would fix it (pass `{young,
  density, poisson, loss, thickness}`). Modes above the band are dropped, not
  folded. **Honest limit of `thickness`**: it is the plate formula, a pitch
  knob, not a measured coin or key. A 25 mm metal coin of 1.5 mm has one mode,
  at 11.5 kHz, and none at 3-9 kHz; an 8 cm disc of the same thickness has four
  between 3 and 9 kHz. An object that must ring in that band needs a `size` and
  `thickness` whose lowest mode sits near 1 kHz (give the human a macro on
  `thickness`, exp curve).
- `contact` {speed, mass, material, [hardness 0.7], [at]}: a half-sine force
  pulse as long as the Hertz contact time (foley's `hertz_time`) of a
  2000 kg/m3 ball of that mass whose stiffness runs from rubber (hardness 0)
  to steel (1) against the struck material. Heavier, slower, softer = longer
  and duller. Area is mass * speed / 0.1 (0.1 kg at 1 m/s = a unit impulse).
  `speed` and `mass` are read at `at`. `gain_db` (-60..60, default 0 =
  unchanged) scales the pulse; `modal` takes the same `gain_db` on its
  (normalized) output, so a macro can balance a struck layer with one field.
- `friction` {speed, pressure, [roughness 0], [grain 200], [seed]}: a
  stick-slip sawtooth (90 % climb, a smooth 10 % slip) at `speed * grain` Hz,
  times `pressure`; speed 0 is silence, negative speeds count as their
  magnitude. `roughness` 0..1 jitters each cycle's length and size and adds
  noise. Both fields are signals: a `ramp` on speed is a creak that rises, a
  `wander` on pressure makes it breathe.
- Recipes: door hinge = `ramp` -> `friction` -> `modal` wood size ~0.45
  (`examples/door_creak.rs`); glass = `contact` -> `modal` glass cut by a fast
  `decay`, then `poisson` (a ramped rate) -> `voices` of a thin-glass `modal`
  with `$rand` size (`examples/glass_break.rs`); a bell = `impulse` -> `modal`
  metal with a small `size`.
- Honest limits: I cannot hear these. Plates are idealized (square, simply
  supported); real doors, bells and glass differ, so treat `size` as a
  pitch knob (give the human a macro on it) rather than a physical dimension.
  Foley's mode table is private, so this is a reimplementation of its
  formulas (plus a gentle high-frequency tilt), not a call into it; the
  material table and `hertz_time` are foley's.

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
  "unclaimed") and is not swept by `verify`. Give a claim when the name says
  what a measure does.
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
  the measure by at least 2 % of its largest value across 0..1** ("the knob
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
- `listen reference: "file.wav"` (relative to the project, or absolute): the
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
- `verify` / `expect` checks: `peak_db` `true_peak_db` `lufs` `duration`
  `silent_edges` `clicks` `dc` `finite` `loop_seam` `bands` `pitch` `onsets`
  `variations` `rebuild` `reference` `macros`, and the synthesis checks
  `aliasing` `inharmonicity` `modulation` `roughness`. Each line is PASS /
  FAIL with the measured value; a FAIL says what to change. Four ways to ask:
  - no arguments: the defaults (`finite`, `peak_db` max -0.3, `silent_edges`
    (a loop: `loop_seam`), `dc`, `rebuild`, and `macros` when the sound has
    knobs).
  - `checks: ["clicks"]` adds checks to the defaults; `only: ["clicks", "dc"]`
    runs just those. Only checks that **need no settings** can be named here:
    `finite peak_db true_peak_db silent_edges clicks dc loop_seam variations
    rebuild macros` (a bare `variations` runs 4 seeds). A check that needs settings errors and shows the `expect`
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
  the recording's level. Measured (`tests/reference.rs`): a rebuilt five-mode
  hit with a noise tick sits 0.69 dB (STFT distance) from its original and
  within 0.1 dB in level; a hard case (30 ms and 20 ms modes, a pair 25 Hz
  apart) 1.21 dB; a rebuild with the modes shifted 10 % sits 13 dB away.

Warnings in `summary`: a node nothing reads, a frequency above Nyquist, a
silent output, DC over -40 dB, an output whose input is squashed by the
ceiling limiter, a sound that never decays, one that is mostly silence, an
`output.lufs` the render missed (with the numbers and the cause), the default
fade-in cutting a first hit (it suggests `fade_in` 0.0005; not given for
a `fade_in` of 1 ms or less or when the target is met), a field driven by a
macro (your value is ignored).

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

## Foley models

Six nodes in `nodes/foley.rs` wrap the user's `foley` crates (pinned rev in
`Cargo.toml`). Foley renders at the sample rate it is given (`*_at`), so they
follow the document's rate. `describe_op <type>` has the schema.

| Node | Fields (`[]` optional) | Notes |
|---|---|---|
| `bubble` | `radius_mm`, `[loudness 0.3]`, `[rise 0.1]`, `[at]`, `[jitter]`, `[seed]` | Minnaert resonance `f = 3.29 kHz / radius_mm` (2 mm 1.64 kHz, 4 mm 0.82 kHz), decays at pi f 0.045, rises in pitch; level ~ r^1.5. The radius is a number (foley cannot retune a ringing bubble): in `voices` use `map {"r": "1 + $rand * 4"}` and `"radius_mm": "$r"`. Under ~0.7 mm it rings above Nyquist and aliases. |
| `air` | `input`, `distance` (signal), `[humidity 50]`, `[celsius 15]`, `[level true]` | A 12 dB/oct low-pass at foley's `absorption_cutoff` (where `distance` m cost 6 dB; none while 20 kHz loses under 6 dB, i.e. below ~10 m) and a 1/distance level drop (0 dB inside 1 m). Both only fall as distance grows. Animate `distance` with a `ramp` for a fly-by. |
| `glottis` | `f0` (signal), `[tension 0.65]`, `[breath 0.05]`, `[vibrato_cents]`, `[vibrato_hz 5.5]`, `[amp]`, `[seed]` | Foley's Liljencrants-Fant flow derivative (`lf_pulse`; tension 1 = pressed Rd 0.3, 0 = breathy Rd 2.7) with seeded breath noise. Pitch reads exactly (YIN). Aliases above ~1.5 kHz. |
| `tract` | `input`, `[vowel a]`, `[vowel_to]`, `[morph]`, `[formants [[hz, bw]..]]`, `[scale]` | **Not foley's waveguide** (its `Tract` is private): a 4-formant Klatt cascade at Peterson-Barney vowels `a e i o u ae uh schwa`. `vowel_to` + `morph` (signal) glide; `scale` > 1 shortens the tract (child, small creature), < 1 lengthens. Feed it a `glottis`. |
| `syrinx` | `species` or `pitch`; `[harmonic] [waver]` (whistle); `[rate] [at]` (species); `[amp] [seed]` | `species`: foley's `birdsong2`, any of 57 (`robin`, `cardinal`, `dove`, `crow`, `wood_thrush`, `boreal_owl`...; a bad name lists all), a phrase of its own length placed at `at`, cut/padded to the document; each `seed` is another individual; `rate` resamples (pitch and speed). `pitch`: a whistle from a contour signal (sine + 2nd harmonic + waver, as foley's songbirds). |
| `thunder` | `[distance 1500]`, `[celsius]`, `[humidity 90]`, `[exposure 1]`, `[brightness 0.5]`, `[length 1]`, `[rumble 0]`, `[tail 0]`, `[at]`, `[amp]`, `[seed]` | Foley's `strike` (Ribner-Roy N-wave over a jagged channel). Silent until `distance / 343` s, then clap and a rumble of several seconds with centroid ~30-50 Hz: set `duration` to fit (a 1.5 km flash: ~14 s). Foley levels every flash to a similar peak, so distance changes timing, brightness and length, not loudness. `exposure` 0 = shut in (low-pass to 200 Hz). `brightness` 0..1 (0.5 = foley's spectrum, bit-identical) is a +-24 dB high shelf at 100 Hz, peak kept: above 0.5 the centroid rises. `length` 0.5..3 (1 = as foley makes it, bit-identical) stretches the rumble after the first 0.1 s clap by overlap-add of 120 ms grains, so the spectrum stays and the rumble lasts `length` times as long (a longer `duration`). `rumble` 0..1 (0 = off, bit-identical) adds a continuous bed of seeded noise band-limited to 25-90 Hz from the first sound to the end of the audible tail, so the tail is not isolated spikes; its rms is `sqrt(rumble)` times the rms of the flash's loudest 0.4 s (0.6 is 2.2 dB under) and falls 3 dB over the tail; it is added before `exposure`, `brightness` and `amp` and the level is not re-normalised, so the flash gets louder with `rumble`. `tail` 0..1 (0 = off, bit-identical) is an exponential fade of the whole output: the gain falls in dB, linearly from the first sound, reaching -40 x `tail` dB at the end of the audible tail (last sample over -60 dB of the peak) and carrying on at that slope; it runs after `length` / `rumble` and before `exposure`, `brightness`, `amp`, and lowers the peak (re-level with `amp` or the output target). Measured at 1500 m, 16 s, audible length (10 ms rms over -45 dBFS) 7.19 / 6.54 / 6.53 / 4.80 / 4.80 s at `tail` 0 / .25 / .5 / .75 / 1: non-increasing, not strict (foley's tail is spikes). `listen`'s `decay -20 dB` is NOT monotone from `tail` 0 to 0.25 (1.84 s to 4.93 s): it counts from the loudest window, which sits in the rumble with no fade and moves to the clap with one; so read the audible length or the loudness curve when tuning a tail (a `decay` claim on a `tail` knob can fail verify's sweep at the 0 to 0.25 step: map `tail` from 0.25 up, or leave the claim off). Measured at 1500 m: the 400 ms rms across the mid-tail spans 18-50 dB at 0, 2.2-5.9 dB at 0.6; centroid stays under 100 Hz (`docs/nodes/r3f.md`). |

`species` / `thunder` are finished one-shots, so they are not causal for a
live runtime (`summary` says so); the rest are. All seeded nodes are
bit-identical at any thread count.

Recipes: **drips** = `poisson` -> `voices` of a `bubble` with a `$rand` radius,
then `air` (`examples/water_drips.rs`); **a vowel** = `glottis` (f0 from a
`wander` or `ramp` for intonation) -> `tract`; **a creature call** = a
`glottis` with a falling `ramp` f0, `tract` `scale` 0.6, `vowel_to` + a
`morph` envelope; **a distant storm** = `thunder` `distance` 3000 through
`air` (more highs gone) and `reverb`.

`glottis` and `tract` have more fields (roughness, subharmonic, fry; nasal,
frication, growl): see "Voice upgrades" under "More nodes, by group".

Honest limits: none of this has been heard. Measured: bubble peak within
0.2 % of Minnaert, glottis f0 within 1 cent, `tract` /a/ peaks at 768 Hz and
/i/ at 223 Hz (f0 110: harmonic spacing), `air` 100 m takes the white-noise
centroid from 12 kHz to 3.3 kHz and the level by 46 dB, thunder at 1.5 km
arrives at 4.4 s with centroid 32 Hz. Whether they sound like water, a voice,
a bird or a storm is the human's call.

## More nodes, by group

Each group below: the fields (`[x]` is optional, a default after it), one
recipe, and the limits that were measured. The long form (tables, every error
message, every measured number) is the fragment in `docs/nodes/`; `describe_op
<type>` has the schema and an example. A signal field takes a number,
`"$param"`, `"=expr"` or a node; a **number** outside a field's range is an
error that names the range, a **signal** is clamped per sample. Every number
quoted is a measurement of a test signal; none of it has been heard, and a
name like "bell" or "engine" says what the node computes, not how it sounds.

Blocks marked `<!-- recipe -->` are rendered by
`crates/sfx_analysis/tests/agents_recipes.rs` (an `output` is added when the
block has none), so they stay valid.

### Sources and oscillators (`docs/nodes/S1.md`, `S2.md`)

- `osc` new fields: `sync` (a mono signal; each rising zero crossing resets
  the phase, polyBLEP, crossing placed to a fraction of a sample), `unison`
  1..16 (voices are **summed**: 7 voices measure 5.45 dB over one, so set `amp`
  near 1/sqrt(unison)), `detune` cents 0..1200 (voices spread evenly from -d
  to +d), `spread` 0..1 (above 0 the output is stereo), `pm` (phase
  modulation in cycles; an `osc` here is FM), `seed` (unison start phases;
  more than one voice makes the node random).
- `noise` `color`: `white pink brown blue violet velvet crackle`. `blue` is
  +3 dB/oct, `violet` +6 dB/oct (measured 2.98 and 5.99 dB/oct). `density`
  (velvet default 2000, crackle 50 impulses a second) is a signal; crackle is
  Poisson with random sign and height.
- `fm` {freq, [ratio 1], [index 0 radians, signal], [feedback 0], [mod_ratio
  1], [mod_shape sine|saw|square|tri], [amp]}: two-operator; sideband k is
  J_k(index) (measured to 0.01 dB). Negative `feedback` errors.
- `wavetable` {freq, harmonics [up to 256 amplitudes], [harmonics_to],
  [morph 0..1, signal], [phases], [amp]}: additive, harmonics fade out toward
  Nyquist so a sweep never aliases (5 kHz, 32 harmonics: alias -63.6 dB).
- `impulse` `at` is a number or a list of times (seconds).

<!-- recipe -->
```json
[
  {"type": "ramp", "name": "pitch", "from": 400, "to": 180, "time": 0.4, "curve": "exp"},
  {"type": "osc", "name": "stack", "shape": "saw", "freq": "pitch", "unison": 7, "detune": 18, "spread": 0.6, "amp": 0.3},
  {"type": "decay", "name": "fall", "time": 0.25},
  {"type": "math", "name": "zap", "op": "mul", "a": "stack", "b": "fall"}
]
```

Macros: `detune` (width), `spread`, the ramp's `to`. Limits: a synced saw adds no
aliasing beyond a plain polyBLEP saw, but that saw already reads -41.6 dB off
the grid at 110 Hz and -29.8 dB at 1999 Hz; `fm` and large `pm` are not
oversampled, so a big index at a high `freq` aliases (not measured); YIN reads
the slave's rate on a sync sweep far above the master.

### Filters, EQ, ladder (`docs/nodes/s4.md`)

- `filter` new `slope` 12 | 24 | 48 dB/oct (`lp` / `hp` only; not in a
  feedback loop). `q` retunes the last section, the default is Butterworth.
  Measured one octave above the cutoff: -12.4 / -24.2 / -48.4 dB.
- `ladder` {input, freq, [resonance 0], [drive 1], [mode lp|bp|hp]}: four-pole
  ladder with a `tanh` per stage. `resonance` runs 0..1.2 and about 1
  self-oscillates at `freq` (measured within 0.14 %) but only if something
  excites it. Passband gain falls as 1 / (1 + 4 resonance) (no make-up).
- `eq` {input, bands: [{mode lp|hp|bp|notch|peak|lowshelf|highshelf, freq,
  [gain_db 0], [q 0.707]}, ...]} up to 16 bands; plain numbers, so a macro
  cannot reach inside a band (use `filter` nodes for a knob).

<!-- recipe -->
```json
[
  {"type": "noise", "name": "n", "color": "white", "amp": 0.3},
  {"type": "ramp", "name": "sweep", "from": 6000, "to": 300, "time": 0.6, "curve": "exp"},
  {"type": "ladder", "name": "acid", "input": "n", "freq": "sweep", "resonance": 0.85, "drive": 2},
  {"type": "eq", "name": "voiced", "input": "acid", "bands": [{"mode": "hp", "freq": 80}, {"mode": "peak", "freq": 1000, "gain_db": 6, "q": 2}]}
]
```

Limits: the ladder's `tanh` is not oversampled (high `drive` folds back); at
the default `q` an `eq` peak is wide (+0.78 dB two octaves away), narrow it
with `q` 4 or more; a 48 dB filter at a few Hz has an f32 noise floor.

### Dynamics (`docs/nodes/S5.md`, `S3.md`)

- `transient` {input, [attack_db 0], [sustain_db 0], [speed 0.02]}: gain on
  rising and on falling level (+-24 dB); a steady plateau is untouched.
- `gate` {input, threshold_db, [ratio] (omit: full gate), [attack 0.001],
  [release 0.05], [hold 0.01], [key]}: downward expander with sidechain.
- `sub` {input (mono), [octaves 1|2], [mix 0.5], [lowpass 120]}: adds a sine
  one or two octaves below the input's low band (monophonic divider).
- `exciter` {input, [freq 3000], [drive 2], [mix 0.3]}: saturates the band
  above `freq` and adds it back (odd harmonics, no oversampling).
- `compress` new `key`: a node that drives the detector (ducking, pumping);
  the gain is applied to `input`.

<!-- recipe -->
```json
[
  {"type": "pattern", "name": "hits", "every": 0.5, "count": 4},
  {"type": "trigger_env", "name": "thump", "events": "hits", "attack": 0.001, "decay": 0.2},
  {"type": "osc", "name": "tone", "freq": 60},
  {"type": "math", "name": "kick", "op": "mul", "a": "tone", "b": "thump"},
  {"type": "osc", "name": "pad_osc", "shape": "saw", "freq": 110, "amp": 0.3},
  {"type": "compress", "name": "duck", "input": "pad_osc", "key": "kick", "threshold_db": -24, "ratio": 6, "attack": 0.002, "release": 0.15},
  {"type": "mix", "name": "both", "inputs": ["duck", "kick"]}
]
```

Measured: `compress` keyed by a pulse at 4:1 gave -15.00 dB in the pulses (20 dB
over) and -0.09 dB between; `gate` took a -50 dB bed to -95 dBFS and moved the
bursts 0.000 dB; `transient` +6 dB attack raised a noise burst's crest 9.89 to
14.45 dB. Limits: `transient` ripples on steady tones under ~100 Hz; `gate` has
no lookahead; `sub` needs the note under ~`lowpass`.

### Shape, oversampling, ring modulation (`docs/nodes/S3.md`, `ringmod.md`)

- `shape` {input, [mode tanh|clip|fold|bitcrush|rectify|asym|diode|tube|table],
  [drive 1, signal], [bits 8] (bitcrush), [bias 0.5] (asym), [points [[x, y],
  ...]] (table, x strictly increasing), [oversample 1|2|4|8], [mix 1, signal]}.
  `oversample` is an error inside a feedback loop. `asym`, `diode`, `tube` leave
  DC: follow with an `onepole` hp.
- `ringmod` {input, freq (signal, +-sr/2), [shape sine|tri|square|saw], [mix 1]}:
  partials at f - freq and f + freq; the input frequency is removed.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "tone", "shape": "saw", "freq": 110},
  {"type": "shape", "name": "grit", "input": "tone", "mode": "asym", "drive": 4, "oversample": 4, "mix": 0.6},
  {"type": "onepole", "name": "clean", "input": "grit", "mode": "hp", "freq": 30},
  {"type": "ringmod", "name": "clang", "input": "clean", "freq": 90, "mix": 0.5}
]
```

Measured: a 5 kHz sine hard-clipped at drive 8 has 24.8 dB less off-grid
energy at `oversample` 4 and 45 dB less at 8; `asym` at drive 2 puts the 2nd
harmonic at -15.5 dBc (`tanh`: -115 dBc). Limits: oversampling removes content
above 0.45 x the sample rate and leaves up to half a sample of offset at `mix`
1; `bitcrush` and `fold` are meant to alias; a square or saw `ringmod` carrier
aliases (not measured).

### Modulation effects (`docs/nodes/s6.md`)

- `chorus` {input (mono if `spread` > 0), [rate 0.8 Hz], [depth 0.003 s],
  [delay 0.02 s], [voices 3, 1..8], [spread 1], [mix 0.5]}.
- `flanger` {input, [rate 0.25], [depth 0.002], [delay 0.003], [feedback 0,
  -0.95..0.95], [mix 0.5], [manual] (seconds; replaces the LFO)}.
- `phaser` {input, [rate 0.5], [stages 4|6|8|12], [center 800 Hz], [depth 1
  octave], [feedback 0], [mix 0.5]}.
- `freq_shift` {input, shift (Hz, signal, +-sr/2), [mix 1]}: moves every
  partial by the same Hz, so harmonic sounds turn inharmonic (single sideband).

`mix` 0 returns the input bit for bit.

<!-- recipe -->
```json
[
  {"type": "noise", "name": "wash", "color": "pink", "seed": 3, "amp": 0.4},
  {"type": "phaser", "name": "swirl", "input": "wash", "rate": 0.3, "stages": 8, "center": 600, "depth": 1.5, "feedback": 0.5, "mix": 0.6},
  {"type": "mix", "name": "summed", "inputs": ["swirl"], "to": "mono"},
  {"type": "chorus", "name": "wide", "input": "summed", "spread": 1, "voices": 4, "mix": 0.5}
]
```

Limits: `freq_shift` image rejection was measured at 1 kHz only (48.5 dB); its
dry path is not delay-matched for `mix` between 0 and 1; the phaser's notches
are up to 2 % below the maths at high frequency; with the LFO running a 3 ms
echo reads 0.880 instead of 1.0 (cause not isolated).

### Space: reverb models and impulse responses (`docs/nodes/S11.md`)

- `reverb` new `model` `fdn` (default, bit-identical to before) | `room` |
  `plate` | `spring`; `er_mix` 0..1 (`room` only: early reflections vs tail;
  an error for other models); `width` 0..2 (wet stereo width). Ports `out`
  (dry + wet) and `wet`. Fixed internal geometry: `size` only scales it.
- `ir` {[model room|tube|box|cabinet|phone], [size m], [absorption], [length
  s], [seed]}: a source whose output is an impulse response for `convolve`
  (`room` is stereo). `size` and `absorption` are errors for models that have
  no such thing (`phone`; `cabinet` has no `absorption`).

<!-- recipe -->
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

Limits: damping makes the broadband T60 up to 13 % shorter than `decay`
(it is the low-frequency time); `room` reflections are nearest-sample with
frequency-independent walls and the tail is the fdn (matched in T60, not in
spectrum); `tube` and `box` are comb filters; whether the spring's chirp
matches a real spring was not measured. An `ir` is at most the document's length.

### Motion and stereo (`docs/nodes/motion.md`)

- `doppler` {input (mono), [speed 30 m/s], [closest 5 m], [at s], [pan true],
  [air true]}, or {input, distance (signal, 0..10290 m)}: a source passing on a
  straight line; pitch bends by the radial speed, level 1/r, optional air
  low-pass and bearing pan. The start is silent for the starting distance / 343 s.
- `haas` {input, [delay 0.012 s, 0..0.05], [side right|left]}: mono to stereo
  by delaying one channel (the other is the input bit for bit).
- `decorrelate` {input, [amount 1], [seed]}: two channels with the same
  spectrum and no zero-lag correlation (all-pass FIRs, 3.4 to ~14 ms delay).

<!-- recipe 4 -->
```json
[
  {"type": "noise", "name": "engine", "color": "brown", "amp": 0.5},
  {"type": "filter", "name": "growl", "input": "engine", "mode": "bp", "freq": 300, "q": 2},
  {"type": "doppler", "name": "pass", "input": "growl", "speed": 25, "closest": 4, "at": 2.0}
]
```

Limits: a point listener in still air at 343 m/s; no reflections, wind or
directivity; `decorrelate` does not keep a mono sum (3 dB down for
uncorrelated channels) and smears a click over ~14 ms.

### Modulators and control (`docs/nodes/S9.md`)

- `chaos` {[model logistic|lorenz|henon], rate, [param], [range [lo, hi]],
  [seed]}: a chaotic control signal in -1..1 or `range`.
- `fbm` {rate, [octaves 4, 1..12], [gain 0.5], [range], [seed]}: layered smooth
  random motion (like `wander` in octaves).
- `curve` {input, points [[x, y], ...], [curve linear|smooth]}: lookup table,
  clamped at the ends.
- `switch` {inputs [2..64 sounds], select (signal), [fade 0.005 s], [blend
  false]}: picks an input by `select` (rounded), linear crossfade.
- `pitch_follow` {input (mono), [min 50], [max 2000], [smooth 0.02]} and
  `centroid_follow` {input, [window 0.02]}: audio to a Hz control signal; both
  look ahead (not causal) and hold the last confident value.

<!-- recipe -->
```json
[
  {"type": "fbm", "name": "drift", "rate": 1.5, "octaves": 4, "range": [-1, 1], "seed": 5},
  {"type": "chaos", "name": "wobble", "model": "lorenz", "rate": 6, "range": [0, 1], "seed": 2},
  {"type": "curve", "name": "pitch", "input": "wobble", "points": [[0, 180], [0.5, 240], [1, 520]], "curve": "smooth"},
  {"type": "math", "name": "bend", "op": "mul", "a": "drift", "b": 30},
  {"type": "math", "name": "hz", "op": "add", "a": "pitch", "b": "bend"},
  {"type": "osc", "name": "voice", "shape": "saw", "freq": "hz", "amp": 0.4}
]
```

Limits: Lorenz `rho` under ~24.7 settles to a constant; `fbm` output sits well
inside its range ends (a 20 s run used 123..270 of 100..300); `switch`'s
crossfade is linear, so uncorrelated inputs dip in level; `pitch_follow` is
monophonic and can make octave errors on a weak fundamental.

### Events to control (`docs/nodes/sequence.md`)

Event streams (`pattern poisson bounce burst once on_threshold shatter`) used
to feed only `grains` and `voices`. These make control signals from them. An
event at time t first affects sample ceil(t x sr).

- `trigger_env` {events, [attack 0.002], [decay 0.2], [curve exp|linear|smooth],
  [scale_by_value true], [retrigger restart|add]}: an envelope per event;
  `exp` is a 60 dB fall that reaches exactly 0 at `attack + decay`.
- `event_gate` {events, [length 0.05]}: 1 for `length` seconds after each event.
- `seq` {events, values [..], [mode cycle|random|once], [glide 0 s], [seed]}:
  steps through `values` on each event and holds.
- `notes` {notes [[pitch, beats], ...] (a name `C4`, `Bb3`, semitones from
  `root`, or `"rest"`), [bpm 120], [root 440], [glide], [gate 0.9]}: ports
  `out` (Hz, held), `gate` (0/1) and `events` (one per pitched note, value =
  Hz), read as `"tune"`, `"tune.gate"`, `"tune.events"`.

<!-- recipe 4 -->
```json
[
  {"type": "notes", "name": "tune", "bpm": 150, "glide": 0.03, "gate": 0.8,
   "notes": [["A3", 1], ["C4", 1], ["E4", 1], ["rest", 1], ["A4", 2]]},
  {"type": "osc", "name": "voice", "freq": "tune", "shape": "saw", "amp": 0.4},
  {"type": "trigger_env", "name": "pluck", "events": "tune.events", "attack": 0.003, "decay": 0.25, "scale_by_value": false},
  {"type": "math", "name": "line", "op": "mul", "a": "voice", "b": "pluck"}
]
```

Limits: `notes` plays once (no loop field; use a `pattern` and `sound`
instances); `seq` `glide` is linear in Hz, `notes` glide is linear in
semitones; `random` can repeat a value; `trigger_env` `restart` with `attack`
0 steps to the new peak.

### Lo-fi and loops (`docs/nodes/S12.md`)

- `crossfade_loop` {input, [fade 0.25 s]}: makes the wrap of a loop
  continuous (equal-power crossfade of the end into the start; same length,
  the middle is untouched). Put it last in a loop's chain. Not causal.
- `resample` {input, rate (Hz, signal), [antialias false]}: sample-rate
  reduction with hold.
- `tape` {input, [wow 0.2 %], [flutter 0.1 %], [drive 1], [hiss_db -60], [tone
  8000 Hz], [seed]}: speed wobble, soft saturation, hiss, tone roll-off. The
  output is delayed by ~0.5 ms to 40 ms (the wobble's mean delay).
- `stutter` {input, events, [slice 0.05 s], [repeats 4], [reverse false]}: the
  last slice repeats at each event (`reverse` is not causal).

<!-- recipe 2 -->
```json
[
  {"type": "noise", "name": "air", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 0.3, "range": [300, 2500], "seed": 2},
  {"type": "filter", "name": "band", "input": "air", "mode": "bp", "freq": "sweep", "q": 3},
  {"type": "crossfade_loop", "name": "seamless", "input": "band", "fade": 0.4}
]
```

as a `kind: loop` with `duration: 2`. Measured: `crossfade_loop` passes the
wrap-jump half every time; the spectral half failed on 2 to 5 of 24 loops whose
filter centre moves once a second or faster and on 12 of 24 at three a second.
It changes the first and last `fade` seconds (a blend of material from
elsewhere), can move DC (follow brown noise with a high-pass), and a steady
drone can swell 3 dB mid-ramp.

### Physical models and finished recipes

Models take a few physical fields and run tuned DSP inside. Most are seeded
(`seed` mixed with the document seed and node name), bit-identical at any
thread count, and **not normalized**: set `output.lufs` or `output.peak_db`.

**Blasts** (`docs/nodes/M1.md`): offline one-shots, mono, exactly the
document's length (like `thunder`).

- `explosion` {size (fireball radius m, 0.1..100), distance (m, 0..30000),
  [debris] (an event node: a tick per event, value = level), [seed]}.
- `gunshot` {caliber (mm, 2..60), [barrel 0.4 m], [suppressed false], [distance
  0], [seed]}: blast plus an N-wave crack when the bullet is supersonic; the
  crack leads the blast by the travel-time difference (135 ms at 100 m).
- Both start 2.5 ms in (so the default fade-in does not cut them), delay by
  `distance / 343` s, darken and quieten with distance, and end in `tanh`, so
  every size peaks near 0.8: size and caliber move the sub band and the length,
  not the level. A document shorter than the delay renders silence.

**Elements** (`docs/nodes/m3.md`): `fire`, `wind`, `spark`, all with `loop`
(bool; unset follows the document, so a `kind: loop` document renders
periodic in its duration; `true` forces it, `false` forces a plain render).

- `fire` {size (m, 0.02..10), [intensity 0.5, signal], [crackle 0.5], [seed],
  [loop]}: roar, hiss and Poisson crackle pops under one control.
- `wind` {speed (m/s, signal, 0..60), [gust 0.3], [whistle 0], [diameter
  0.004 m], [seed], [loop]}: five moving resonant bands plus an optional
  Aeolian tone. Speed 0 is exact silence.
- `spark` {rate (strikes/s, 0.1..200), [buzz 50|60 Hz], [body 3000 Hz], [seed],
  [loop]}: an arc gated by the mains waveform into a short resonator.
- Not normalized (a default `fire` is about -30 LUFS, peak -8 dBFS; a pop-heavy
  one is peaky: crest 23.5 dB). A sparse `spark` (rate under ~10) can still fail
  the spectral half of `loop_seam` with `loop: false` (7.2 dB at rate 8); the
  periodic render (jittered strike grid) read 0.9 dB. Memory: fire holds six
  buffers of the document length.

**Water** (`docs/nodes/M4.md`):

- `water` {flow (signal 0..1; 0 is silence), [depth 0.5], [splash 0], [density
  300 bubbles/s, 0..3000], [tone 0.5, 0..1], [loop], [seed]}: a population of
  Minnaert bubbles (rate density x flow^1.5, median radius 0.8 mm x 5^flow)
  plus hiss. Capped at 800000 bubbles a render (an error that says so).
  `tone` is a high shelf at 2 kHz of (tone - 0.5) x 24 dB over everything the
  node makes (0.5 = off, bit-identical); it is primed from the periodic
  buffer in a loop, so put the brightness knob here and not in a `filter`
  after the node (which breaks the seam). The level is not re-normalised.
- `splash` {size (m, 0.005..1), [tail 1, 0..3], [burst 1, 0..2], [at 0],
  [seed]}: a bright, brief impact burst
  (starts 2.5 ms in, so the default fade-in does not cut it), then a cloud of
  bubbles that grow with time and end in the main cavity bubble, so the
  centroid **falls** through the sound (round-1 redesign; it used to rise);
  scaled to a **peak of 0.5 whatever the size**. `lufs -24` is reached by every
  size and `-20` by 0.1 and 0.5; the 0.17 s pebble (0.02) lands at -21.9 with a
  missed-target warning: for a short splash use `peak_db`. `tail` scales the
  length of the bubble cloud after the burst and its bubble count (0 = no
  cloud; a splash never rings past 2.5 s); `burst` is the opening burst's
  level against the main bubble (2 makes it the loudest moment, 0 drops it;
  the splash is re-levelled to peak 0.5, so a louder burst quiets what
  follows). Measured: last fine onset 0.14 / 0.27 / 0.54 s at `tail` 0.5 / 1 /
  2; peak in the first 20 ms 0.135 / 0.461 / 0.500 at `burst` 0 / 1 / 2.
  Defaults are bit-identical to before (`docs/nodes/R4-D.md`).
- Measured: `flow` 0.1 / 0.35 / 0.8 at density 60 gave 1.9 / 9.4 / 17.4 onsets
  a second and centroids 3102 / 2607 / 1971 Hz; no energy under 165 Hz (mix in
  a rumble for a river).

**Machines** (`docs/nodes/machines.md`): `rpm` is a signal, so a `ramp` or
`wander` revs them; set a `duration` (the ramps hold their last value).

- `engine` {rpm (0..20000), [cylinders 4, 1..16], [load 0.5, signal], [stroke
  4|2], [exhaust_length 1.2 m], [seed]}: pitch is the firing rate rpm / 60 x
  cylinders / (stroke / 2) Hz; an open-pipe exhaust comb resonates at odd
  multiples of 343 / (4 L).
- `motor` {rpm, [poles 4, even], [gear_teeth 0..400], [load 0.3], [whine 0.3],
  [carrier 4000 Hz], [seed]}: hum at the pole-pair rate, a PWM whine with
  sidebands, a gear-mesh tone at rpm / 60 x teeth, brush noise; scaled by
  rpm / (rpm + 500), so a spin-up fades in. Partials above 0.47 x the sample
  rate fade out instead of aliasing.

**Waveguides** (`docs/nodes/waveguide.md`): causal, mono.

- `tube` {length (m, 0.029..20), [excite reed|lip|jet|noise], pressure
  (signal 0..1), [bore 0.01 m], [open_end true], [seed]}: pitch c / (2L) open,
  c / (4L) closed at one end (odd harmonics).
- `string` {freq (20..sr/16 Hz, signal), [excite pluck|strike|bow], [position
  0.15], [damping 0.3], [stiffness 0], [bow_pressure 0.5, signal], [seed]}.
- Limits: the bow locks onto a higher harmonic at some pitch / position pairs
  and low pressures (`listen` before trusting one); the pluck and strike are
  fixed 2 ms excitations; `tube` has no toneholes or bell (pitch error up to 1.8 %).

**Surfaces** (`docs/nodes/M6.md`):

- `rolling` {speed (m/s signal, 0..50), [roughness 0.5], [size 0.002 m], [seed]}:
  the force of rolling over a rough surface, an excitation for `modal`; level is
  proportional to speed (up to +34 dB at 50 m/s): scale it before a `modal`.
- `cloth` {motion (signal 0..4; 0 is silence), [weight 0.5], [seed]}: rustle
  bursts with power-law sizes in four bands (cloth, paper, foliage).
- `footstep` {surface concrete|gravel|grass|wood, [weight 0.5], [pace walk|run|
  sneak or 0..1], [shoe hard|boot|sneaker|bare], [body 1], [swish 0.5], [at 0],
  [seed]}: one heel and one toe (70-150 ms apart), a finished sound; a default
  step peaks near -12 dBFS. Build a walk with several nodes or `sound`
  instances. `body` (0.5..2) multiplies the resonant body frequency apart from
  `weight` (wood: all three modes; concrete: the low thud; gravel: the thump;
  grass: the low band; 0.5 an octave down). `swish` (0..1) trades the swish
  for the ticks: grass swish bands x `1 + 2 (swish - 0.5)` against the stalk
  ticks x `1 - 2 (swish - 0.5)`; gravel stone ticks fewer and a swish bed
  fading in above 0.5; concrete and wood ignore it. Both defaults are
  bit-identical to before the fields (pinned hashes); give the human macros on
  them. Measured (`docs/nodes/R2-G.md`): grass energy above 2 kHz at swish 0 /
  .5 / 1 is 0.10 / 0.56 / 0.77; the wood low mode at weight 0.5 goes 78 Hz to
  305 Hz from `body` 0.5 to 2.

- `crumple` {density (signal, bursts/s, 0..400; 0 is silence), [size 0.5],
  [hardness 0.5], [seed]} (`docs/nodes/crumple.md`): paper, foil, plastic or
  leaves as a population of short band-passed noise bursts plus sparse louder
  snaps, arrivals on a jittered grid so the count follows `density` all the way
  (`cloth` saturates: its fine onset count stops rising from `motion` 1).
  `size` lowers the burst centres and lengthens the decays, `hardness` raises
  the centres (1.5-4.5 kHz soft, 3-9 kHz hard) and shortens them. Measured:
  fine onsets per second 20 / 40 / 80 / ~155 at density 20 / 40 / 80 / 160;
  centroid 4.5 kHz at hardness 0 to 7.2 kHz at 1; 3.9 kHz at size 1 to 8.1 kHz
  at 0. Above density 40 each burst shortens so the energy stays level. Not
  normalized and peaky when sparse: `output.lufs` -20 is met at density 40,
  `peak_db` for a few bursts a second. The fine onset counter reads low above
  about 200 per second (checked to 200). Not heard: macros on `hardness`
  (claim `centroid`, 1) and `size` (claim `centroid`, -1) work.

<!-- recipe -->
```json
[
  {"type": "wander", "name": "squeeze", "rate": 3, "range": [30, 140], "seed": 2},
  {"type": "crumple", "name": "foil", "density": "squeeze", "size": 0.4, "hardness": 0.85, "seed": 3},
  {"type": "output", "name": "out", "input": "foil", "lufs": -20}
]
```

**Mechanism clicks** (`docs/nodes/click.md`):

- `click` {kind latch|switch|button|lock|clasp, [size 0.5], [hardness 0.7],
  [at 0], [double true], [seed]}: one or two transients (a press, then a
  release or second snick 24-72 ms later by kind: latch 45, switch 24, button
  70, lock 72, clasp 35 ms) through three or four small body resonances at
  1.5-8 kHz. `size` 0..1 scales the modes by 4^(0.5 - size) (bigger is lower and
  longer; a `switch` centroid ran 4.7 kHz at 0.1 to 2.1 kHz at 0.9),
  `hardness` 0..1 is the contact sharpness; the seed moves modes and gap by
  +-3 %. The press peaks at 0.5 (-6 dBFS) whatever the settings: scale it with
  a `gain`, or `peak_db` on the output. **A click at `at: 0` starts at sample
  0 and the default fade-in cuts it** (peak 0.50 became 0.30): give the
  `output` `fade_in: 0.0005`, or `at: 0.005`.
- Recipe: a door latch is `click` `kind: "latch"` (size 0.6, hardness 0.8)
  after the thud of a `modal` panel; a light switch is `kind: "switch"` size
  0.35 hardness 0.85. Macros on `size` (claim `centroid`, -1) and `hardness`
  (claim `centroid`, 1) work. The descriptors' onset table merges the 24 ms
  `switch` double: count it with `expect onsets {spacing_ms: 5}`.

<!-- recipe -->
```json
[
  {"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.0},
  {"type": "gain", "name": "hit", "input": "latch", "gain_db": -2},
  {"type": "output", "name": "out", "input": "hit", "fade_in": 0.0005, "peak_db": -3}
]
```

**Modal shapes and `shatter`** (`docs/nodes/M7.md`):

- `modal` `shape` `plate` (default) | `bar` (free-free: 1 : 2.757 : 5.404) |
  `tube` (ring modes of a pipe wall; `size` = diameter) | `membrane` (Bessel
  zeros; `strike_position` 0 is the centre) | `string` (sin(n pi x) / n; takes
  `stiffness` 0..1, B in f_n = n f_1 sqrt(1 + B n^2); an error for other
  shapes) | `shell` (the idealized bell series). `modes` 1..256: the lowest
  half are always the lowest modes of the object (so the fundamental falls with
  `size`: wood plate 424 / 166 / 88 / 54 / 42 Hz at 0.5 / 0.8 / 1.1 / 1.4 / 1.6
  m). Output is normalized (unit impulse peaks near 0 dBFS). A `size` outside the
  usable range (lowest mode 24 Hz .. 90 % of the top of the band) errors with
  the range for that material and shape. `loss` and `tilt` work on every shape
  (see "Physical models: modal, contact, friction").
- `shatter` {pieces (1..200000), [spread 0.4 s], [at 0], [exponent 2, 0.5..6],
  [seed]}: an event stream, a crack at `at` (value 1) then fragments whose
  sizes follow a power law (value 0.01..1) and whose times crowd after the
  break. Feed `voices` (a small `modal` per fragment) or `grains`.
- Limits: `size` is a pitch knob; `bar` / `tube` use the material's
  `thickness` (metal is a 5 cm plate: pass `{young, density, poisson, loss,
  thickness}` for a thin pipe wall); sizes and times in `shatter` are drawn
  independently (a big piece is as likely late as early).

**Voice upgrades** (`docs/nodes/M8.md`): every new field is off by default
(the old renders are hash-pinned).

- `glottis` new `roughness` 0..1 (period jitter up to +-4 %, level +-50 %),
  `subharmonic` 0..1 (every second cycle scaled by 1 - x: a line at f0 / 2),
  `fry` 0..1 (strong jitter, alternating long / short periods, pressed): all
  signals, per glottal cycle.
- `tract` new `nasal` 0..1 (a 250 Hz pole and 1 kHz zero), `frication` {freq,
  level} (noise through a band-pass added after the formants; gate it with
  `level`), `growl` 0..1 (AM at ~45 Hz), `seed`.
- Limits: `glottis` still aliases above ~1.5 kHz; the nasal and growl rates are
  fixed; the frication centroid sits above `freq` (2258 Hz for 1500).

<!-- recipe 3 -->
```json
[
  {"type": "ramp", "name": "revs", "from": 1200, "to": 5200, "time": 2.0, "curve": "exp"},
  {"type": "ramp", "name": "throttle", "from": 0.2, "to": 0.9, "time": 2.0, "curve": "linear"},
  {"type": "engine", "name": "eng", "rpm": "revs", "cylinders": 6, "load": "throttle", "exhaust_length": 1.5, "seed": 1},
  {"type": "shatter", "name": "bits", "pieces": 40, "spread": 0.5, "at": 0.02, "seed": 4},
  {"type": "voices", "name": "shards", "events": "bits", "length": 0.3, "max": 50,
   "map": {"size": "0.05 + $value * 0.35"},
   "patch": [
     {"type": "impulse", "name": "tap", "at": [0.0]},
     {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
     {"type": "modal", "name": "piece", "excite": "strike",
      "material": {"young": 70e9, "density": 2500, "poisson": 0.22, "loss": 0.006, "thickness": 0.003},
      "size": "$size", "modes": 16}]},
  {"type": "gain", "name": "quiet_shards", "input": "shards", "db": -12},
  {"type": "mix", "name": "scene", "inputs": ["eng", "quiet_shards"]}
]
```

More recipes: a wind loop is `wind` `speed` 12 `gust` 0.6 in a `kind: loop`
document with `output.lufs` -26; a campfire is `fire` size 0.7 with an
`onepole` hp at 30 Hz and `lufs` -28; a rifle heard at 120 m is `gunshot`
caliber 7.62 barrel 0.5 distance 120 with `output.lufs` -14; a brook is
`water` with `flow` from a `wander` 0.35..0.7 plus a `splash` `at` 1.5; a
struck pipe is a hard `contact` into `modal` `shape: "tube"` metal `size` 0.12;
a creaky nasal vowel is `glottis` `fry` from a `ramp` into `tract` `vowel: "o"`
`nasal` 0.5. Macros worth giving: engine `exhaust_length` and the rpm ramp's
`to`; explosion `size` and `distance`; fire `size` and `intensity`; wind `speed`
and `gust` (claim `swing`); water `depth` and `splash`.

### Spectral additions (`docs/nodes/X1.md`)

Offline, acausal, exactly the document's length; the old modes and
`pitch_shift` / `time_stretch` without `formant` are bit-identical to before.

- `spectral` new `mode`s: `shift` {hz (signal, +-sr/2)}: single-sideband shift
  of every component (a 200/400/600 Hz tone at +50 Hz is 250/450/650);
  `stretch` {[inharmonicity 0.01, 0..2], [f0 100 Hz]}: partial n of a series
  at f0 lands at n f0 sqrt(1 + b n^2) (stiff-string inharmonicity);
  `smear` {time (s, -2..2)}: an all-pass whose group delay rises linearly to
  Nyquist (a click becomes a chirp; the tail past the document is cut, give a
  `duration`); `hps` {[kernel 17, odd 3..101]}: median-filter split with ports
  `out` (= `harmonic`), `harmonic`, `percussive`, read as `"name.harmonic"`;
  the two sum back to the input. A field for another mode is an error.
  `window` applies to `stretch` and `hps` only.
- `pitch_shift` new `formant: true`: keeps the input's spectral envelope
  (cepstral true envelope, +-30 dB per bin) instead of shifting it; about 44
  extra FFTs a frame.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "saw", "shape": "saw", "freq": 180, "amp": 0.4},
  {"type": "decay", "name": "fall", "time": 0.4},
  {"type": "math", "name": "note", "op": "mul", "a": "saw", "b": "fall"},
  {"type": "spectral", "name": "split", "input": "note", "mode": "hps"},
  {"type": "spectral", "name": "bent", "input": "split.harmonic", "mode": "stretch", "inharmonicity": 0.03, "f0": 180},
  {"type": "mix", "name": "both", "inputs": ["bent", "split.percussive"]}
]
```

Limits: `stretch` moves peaks by whole bins, so noise smears by a bin (8 clean
partials stayed within 0.2 dB); `hps` splits by soft masks, not exactly where a
click and a partial share a bin and frame; `formant` assumes envelope features
wider than ~500 Hz (a pitch above ~400 Hz partly follows the harmonics, not
measured); `shift` leaks slightly (-58 dB) on an abrupt start.

### Bands and vocoder (`docs/nodes/X2.md`)

- `filterbank` {input, bands (1..64 log-spaced from `low` to `high`), [gains
  (one per band, linear), default all 1], [q], [low 80], [high 12000]}.
- `vocoder` {carrier, modulator (mono), [bands 16], [attack 0.01], [release
  0.04], [q], [low 80], [high 12000]}: the modulator's level per band scales
  the same band of the carrier. The output is as quiet as the modulator
  (amplitude 0.1 gives -20 dB): follow with a `gain` or `normalize`.
- `multiband` {input, crossovers [1..7 Hz, ascending], bands: [{[drive 0],
  [threshold_db] (turns the compressor on), [ratio 4], [attack 0.01], [release
  0.1], [gain_db 0]}, ...]}: Linkwitz-Riley 4th-order split, per-band drive,
  compressor and gain, summed flat. Ports `out` and `band0`..`band7`.
  `ratio` / `attack` / `release` without `threshold_db` is an error.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "buzz", "freq": 110, "shape": "saw", "amp": 0.5},
  {"type": "noise", "name": "speech", "color": "pink"},
  {"type": "wander", "name": "sweep", "rate": 3, "range": [300, 2500]},
  {"type": "filter", "name": "formant", "input": "speech", "mode": "bp", "freq": "sweep", "q": 4},
  {"type": "vocoder", "name": "voice", "carrier": "buzz", "modulator": "formant", "bands": 16, "attack": 0.005, "release": 0.03},
  {"type": "multiband", "name": "thick", "input": "voice", "crossovers": [150, 1000], "bands": [{"threshold_db": -30, "ratio": 4, "gain_db": 3}, {}, {"drive": 2, "gain_db": -3}]}
]
```

Limits: with all gains 1, `filterbank` raises white noise 2.4-3.0 dB (the bands
overlap) and a single band-pass has 6 dB/oct skirts; `vocoder` has no
sibilance path (noisy consonants pass only where the carrier has energy) and a
harmonic carrier leaves bands without a harmonic quiet; `multiband` settings are
numbers (no automation), the compressor has no lookahead, `drive` is not
oversampled; all hold the whole document per band in memory.

### Grains and clouds (`docs/nodes/X3.md`)

- `grains` {source (a node or a `.wav`), events, size, [position 0..1, signal],
  [pitch 1], [window hann|tri|rect], [pan_spread], [jitter], [seed]}: `size`
  and `pitch` each take a number, a signal (read at the grain's start), or a
  range `{min, max, [curve linear|exp|smooth]}` that draws a value per grain.
- `cloud` {source, density (grains/s, signal, 0..sr), size, [position], [jitter],
  [pitch], [pitch_spread 0 semitones, 0..120], [mode random|regular], [window],
  [pan_spread], [seed]}: grains scheduled from a density signal, no event
  stream (Poisson starts in `random`, even in `regular`). At most 200000 grains
  a sound. Not normalized: the level rises with density x size.

<!-- recipe -->
```json
[
  {"type": "osc", "name": "tone", "freq": 330, "amp": 0.5},
  {"type": "ramp", "name": "swell", "from": 4, "to": 160, "time": 2.0, "curve": "exp"},
  {"type": "cloud", "name": "swarm", "source": "tone", "density": "swell",
   "size": {"min": 0.02, "max": 0.08}, "position": 0.1, "pitch_spread": 2, "pan_spread": 0.6}
]
```

Limits: size and pitch signals are read once at a grain's start; a grain that
reaches the end of the source stops there (no wrap); `pitch_spread` is uniform
in semitones, a `grains` `pitch` range with `curve: "linear"` is uniform in
ratio.

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

## The window

`sounder` (no `--headless`, default `window` feature) opens a system webview
(`wry` on a `tao` event loop, main thread) beside the MCP server: the human
listens and comments there; the agent keeps using the tools. `--headless`
never opens it; `cargo build --no-default-features` has no wry / tao / cpal.

```sh
cargo run                         # window + MCP on stdio
cargo run -- --project DIR        # same, in a project
```

Data flow (the contract is `design/window-api.md`):

- The page (`ui/`, embedded at build time by `build.rs`; a built-in raw-state
  page stands in when `ui/` is absent) calls `sounder.call(name, args)` ->
  `window.ipc` -> a `Cmd::Exec` on the session thread -> `window/api.rs` as
  client `window` -> the reply returns as `sounder._reply(...)`. The window's
  edits are credited to it: `set_field` is one history step `~node` `by window`;
  comments are `by you`; knob moves are recorded as the human's (the agent's
  `summary` shows and clears them; the window's `state` shows them without
  clearing).
- `Session::rev()` bumps on every visible change from any client; after each
  command that moved it the session thread wakes the loop and the page gets
  `sounder.changed(rev)`, then asks for `state` (`src/session/window_state.rs`).
  `state` never waits for the default checks or the macro sweeps: they are
  empty / null until a follow-up command (400 ms after changes pause) runs
  `Session::check` and bumps `rev`.
- Bulk data comes over `sounder://render/<doc>/{spectrogram,wave,loudness}`
  and `.../node/<name>/spark` from `sfx_analysis::view` (the same STFT window,
  dB scale and loudness window rule as `listen`'s image), `?rev=` caches.
- Playback goes through `PlayerControl` (`window/player_iface.rs`, a re-export
  of `src/player.rs`); player events become `sounder.played({...})`.

What the window cannot do yet: no Edit menu (so no Cmd-C / Cmd-V in text
fields), no `catalog_save` call, no resolving comments (the agent does that).

### Driving the real page from a browser (dev only)

`sounder --headless --dev-ui 8767` (window build) also serves the page and the
window's calls / bulk data over plain HTTP on 127.0.0.1 (`window/dev.rs`: no
auth, std only). Open `http://127.0.0.1:8767/app/index.html?bridge=http://127.0.0.1:8767`
in any browser (an agent's included): the page runs against the REAL session
exactly as the webview does (`?bridge=` switches `ui/app.js` from `window.ipc`
to `POST /call`, `GET /render/...`, and a `GET /poll` loop that stands in for
`sounder.changed`). With the driver daemon: `SOUNDER_EXTRA_ARGS="--dev-ui 8767"
python3 mcp_driver.py --daemon ...`, then talk to the same session as an agent
with `--connect`. This is how the plan's window checks were run: a knob drag
and a typed comment show up in the agent's `summary`; an agent's `sound_ops`
redraws the open page without a reload. Caveat for browser tooling: a page
without focus gets no `requestAnimationFrame`, so the first paint can wait for
a screenshot or click.

### Checking the native webview without a screenshot

`SOUNDER_PROBE=/path/out.json sounder` (window build) makes the window measure
its own rendered page 6 s after it opens and write the result: user agent,
loaded fonts, JS errors, rect / font / colour of the layout elements, canvas
sizes and how many sampled pixels are lit (did the spectrogram paint). The
driver's `--daemon --windowed` starts the window. Last run (WKWebView, dpr 2):
no errors, four Plex faces loaded, layout rects equal to the mock's
(top 48, hud 40, dock 162, right 300), spectrogram canvas painted.

## Not now

- A **live runtime** for the causal subset (`summary` reports whether a sound is
  fully causal; the loop-capable nodes have a per-sample form already, the rest
  are block-evaluated and would need one). Firewheel and `bevy_seedling` are
  checked out for that later.
- Compressed export (ogg).
- A project **sound style** (loudness targets per category); project-level
  macros across several sounds ("grit" on all the footsteps) belong with it.
