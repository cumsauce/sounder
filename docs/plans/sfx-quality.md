# Plan: raise sounder's output from "plausible" to designed SFX

## Context

Sounds built through sounder pass every check and still come out thin. The
diagnosis (from AGENTS.md and a map of the code, not from listening):

1. Agents are steered to one ready-made node plus knobs; designed SFX are 4-8
   layers (transient, body, sub, sweetener, tail).
2. No finishing chain, and mono by default: the `output` stage is gain, target,
   limiter, fades.
3. `verify` checks correctness (level, clicks, seams), not design: nothing asks
   for low-end weight, a distinct attack, a tail, or width.
4. No recorded source material and no reference set. `sample`, `grains`,
   `convolve`, `listen reference` exist but there is no library to point them
   at, and `extract_modes` discards the noisy residual.
5. `modal` is linear and idealized, so struck objects read as synth bells.
6. Nothing exaggerates (sub drop, pitched-down double, anticipation).

The work is ordered cheapest-and-most-likely first. **Phase 0 is a listening
test that decides how far down the list to go**; do not skip it.

Rules that hold for every phase (from AGENTS.md):

- Never write that something "sounds" like anything; state measurements.
- New fields are off by default and bit-identical to before (hash-pinned
  renders must hold).
- Every tool/node rejects unknown fields; errors name the field and the fix.
- AGENTS.md and `INSTRUCTIONS` (`src/bin/sounder/server/tools.rs:895`) are
  kept in sync; the "109 node types" count is hard-coded at `tools.rs:899`,
  `AGENTS.md:49` and `:181` and must be bumped per new node type.
- A node doc goes in `docs/nodes/<task>.md` following `docs/nodes/TEMPLATE.md`
  (Fields / Recipe / Measured / Honest limits), measurements from a test file
  `crates/sfx_analysis/tests/<task>.rs` using `tests/common/mod.rs`.

## Phase 0: the A/B listening test (no code)

Build one brief three ways over MCP and let the human compare in the window.
Brief: **a heavy wooden door slam**. Save all three to the catalog.

- `slam_a`: as agents build today (a `click` latch under a `modal` thud, one
  `peak_db` output).
- `slam_b`: hand-layered with existing nodes only:
  transient (`click` + short noise burst through `transient`), body (`contact`
  -> `modal` wood, `loss` < 1), sub (`osc` sine 55 -> 35 Hz `ramp` x fast
  `decay`), sweetener (`friction` creak tick or `crumple` rattle at low
  level), tail (`reverb` `model: "room"` on a send, `at` a few ms); then a bus:
  `multiband` (glue the low band), `shape` `mode: "tube"` `oversample: 4` at a
  low `mix`, `eq`, `width`/`haas` on the tail only (keep the low band mono).
- `slam_c`: `slam_b` with the body and sweetener replaced by recordings via
  `sample` / `grains` (the human supplies 2-3 WAVs under `<project>/refs/`).

Outcome recorded with `session_log(feedback: ...)`: which step (a->b, b->c)
the human hears as larger. If a->b is large, Phases 1-3 are the priority; if
only b->c matters, go straight to Phase 4.

## Phase 1: layered guidance and a worked scene (docs + one example)

Goal: an agent's default for an impact / mechanism / blast brief is a layered
scene, not one node.

- `AGENTS.md`: add a section **"Layering a designed sound"** before "Which
  ready-made node for which sound": the five roles, the band and time slot each
  owns (transient 2-8 kHz first 10 ms; body 150 Hz-2 kHz; sub 30-90 Hz; tail
  after the body via `mix` `at`), and when one node is still right (ambience
  loops, UI blips). Include the Phase 0 listening lessons
  (`docs/plans/phase0-results.md`): SFX are **dry** by default (no reverb
  unless the brief names a space); impact weight is **band-limited noise**
  (90-500 Hz), not a sine sub drop, which reads as a tuned drum; keep `modal`
  loss high or excite it with noise; `click` and hard `crumple` read as metal,
  so keep them out of wooden sounds; add pre-impact detail (friction, brush)
  where the object would make it. Soften "a brief that fits a ready-made node is one node and
  a few knobs" in "Getting started" step 2 to "start from the ready-made node
  as the body layer".
- Add 3 `<!-- recipe -->` blocks (door slam, sword hit, UI confirm with a tail)
  so `crates/sfx_analysis/tests/agents_recipes.rs` renders them.
- Mirror a 3-line form in `INSTRUCTIONS` (`tools.rs:895-906`).
- `examples/door_slam.rs`: the first example that layers different models
  (pattern: `examples/glass_break.rs`).

## Phase 2: a `finish` node (bus chain in one node)

Goal: the finishing chain is one node with knobs, so agents apply it and the
human can dial it.

- New node type `finish` in `crates/sfx_graph/src/nodes/finish.rs`, composed
  from existing DSP (reuse the code behind `transient`, `shape`, `multiband`,
  `eq`, `width`; do not reimplement). Fields, all off at default so
  `finish {input}` is bit-identical to its input:
  `punch` 0..1 (transient attack gain), `glue` 0..1 (low/mid band compression),
  `warmth` 0..1 (oversampled tube saturation mix), `air_db` (high shelf),
  `weight_db` (low shelf), `width` 0..2 applied **above 150 Hz only** (low band
  stays mono). No reverb inside `finish`: Phase 0 showed SFX should be dry by
  default.
- **Done (2026-11-17).** As built it differs from the above: an added
  `tail_db` (a second `transient` stage at the default speed; `punch` runs at
  `speed` 0.005 so it catches the first milliseconds), and `glue` uses an
  instant attack (a 10 ms attack raised the crest instead of lowering it).
  `width` on a mono input is an error. Measured in `docs/nodes/finish.md`; on
  the door slam's layers it matched the hand-built bus (9.32 vs 9.23 dB).
- Registration steps: struct with `Describe` + `Eval` (pattern: `Haas`,
  `crates/sfx_graph/src/nodes/motion.rs:292`), `mod`/`pub use` in
  `nodes/mod.rs`, a line in `node_types!` (`crates/sfx_graph/src/node.rs:231`).
- Tests `crates/sfx_analysis/tests/finish.rs`: defaults bit-identical; each
  field moves its measure monotonically (crest, LUFS at fixed peak, centroid,
  stereo width) and the sub-150 Hz correlation stays 1.
- Docs: `docs/nodes/finish.md`, AGENTS.md group section, macro advice ("punch"
  claim `crest` 1, "air" claim `centroid` 1).

## Phase 3: design checks in `verify` and `listen`

Goal: the senses notice a thin sound.

- New `expect` check **`layers`** in `crates/sfx_analysis/src/checks.rs`
  (field on `Expect` at `:213`, block in `run` at `:529`, entry in
  `bare_check` at `src/session.rs:1235`). Settings, each optional:
  `voiced {max}` (share of the sound the pitch tracker calls voiced: an
  atonal impact should read low; Phase 0's tuned-drum B read 0.9, the real
  slam 0.15), `dry {max}` (energy after the last source event ends, so a
  reverb tail is caught), `crest_db {min, max}` (real slam 16 dB, B 28 dB),
  `sub_share {min}` (energy under 90 Hz), `attack_ms {max}`, `tail_ms {min}`
  (time from peak to -40 dB), `width {min}`, `bands_occupied {min}` (octave
  bands within 30 dB of the loudest). Reuse `Descriptors` fields
  (`descriptors.rs:135`; energy shares `:1196`, attack `:1227`, stereo `:1147`).
  A FAIL names the missing role and the layer to add.
- **Not in the defaults**: a UI blip legitimately has no sub. Instead, a
  one-line `summary` hint for a oneshot whose energy sits in a single octave
  band and has no tail ("one band, no tail: see Layering").
- Add an `energy by role` line to `Descriptors::table()` (`:1353`).
- Tests: a bare `modal` hit fails `layers` with sub and tail settings; the
  Phase 1 door slam passes.
- **Done (2026-11-17).** As built: `layers` takes `preset` (`impact`), `weight`
  / `body` / `crack` share ranges (under 200 Hz / 200 Hz-2 kHz / above 2 kHz,
  the bands `listen` already had), `voiced_max`, `ringing_max`, `crest_db`,
  `tail_ms`, `dry` (no `reverb` / `convolve` node: structural, not
  measured). Dropped: `attack_ms` (the approved slam's pre-impact brush puts
  its 90 % point at 42 ms), `width` (SFX are mono), `bands_occupied` (it
  did not separate the slams). Added `ringing_max`: the search-tuned `slam_b`
  the listener heard as a tuned drum reads voiced 0.00; its three strong
  partials ringing 0.65-0.98 s are what separate it. Tests run on the Phase 0
  slams as fixtures; the real recording passes too.

## Phase 4: source material

Goal: recordings and real IRs are first-class, and a recording can be rebuilt
with its noise.

- **Library folders**: `<project>/assets/sources/` and `<library>/sources/`
  (`src/store.rs:41` `library_dir`). `Cx::path` (`node.rs:120`) tries
  `base_dir`, then the sources folders, so `"file": "wood_hit_03.wav"`
  resolves. `catalog` gains `action: "sources"` listing files with length,
  peak, centroid (computed once, cached by mtime).
- **Fixes found while mapping**: `convolve` errors on an IR at another sample
  rate (`nodes/time.rs:257`): resample instead. `grains`/`cloud` `source` WAVs
  are not in the render cache key (`crates/sfx_graph/src/eval.rs:366-379`):
  add path + mtime as for `file` / `ir`.
- **Residual extraction**: `modal::extract` (`crates/sfx_analysis/src/modal.rs:214`)
  already forms recording-minus-modes inside its loop (~`:246`); return it.
  `extract_modes` (`tools.rs:671`) gains `residual: "name.wav"`, which writes
  the residual into the sources folder and returns a second node (a `sample`
  of it) mixed with the `resonators`. Test in `tests/reference.rs`: STFT
  distance of modes + residual to the original is under the modes-only figure
  (0.69 dB for the five-mode case).
- **Done (2026-11-17).** As built: `sfx_graph::node::resolve_file` (project,
  then each sources folder by path, then by file name) behind `Cx::path`, the
  cache key, `reference::load` and `extract_modes`; `RenderOpts.sources` /
  `Ctx.sources` / `ExportOpts.sources` carry the folders. `catalog sources`
  lists the WAVs (no mtime cache: computed per call on the first 10 s).
  The residual is the recording minus the **rendered** `resonators` (not the
  fit's own models: those carry the recording's phases while the resonators
  start at phase 0, and subtracting them made the rebuild worse, 0.93 to
  1.41 dB), with a 5 ms lead-in. Measured 0.22 dB modes + residual against
  4.85 dB modes alone. The resampler moved from `sfx_analysis::reference` to
  `sfx_graph::wav::resample` (the former now calls it).
  Found on the real door slam: `extract_modes` fits a noisy hit as close
  partials whose phases cancel, and the `resonators` (all starting at phase
  0) then play them 46 dB louder than the recording. A pre-existing limit of
  the extractor (the synthetic tests have clean, separated modes), now
  caught: a warning over 6 dB, and `residual` refuses.
- **Extractor fixed for noisy recordings (2026-11-17).** The blow-up was the
  peeling itself, not only close pairs: neighbours inside the demodulation's
  band (50-100 Hz) grew to cancel each other pass after pass. Merging
  cancelling clusters changed nothing (83.5 dB over on band-passed noise),
  so it was dropped. Now: each peeled amplitude is capped at the recording's
  peak (83.5 to 18.2 dB), and when the peeled partials, played as
  `resonators` do, still peak over 6 dB above the recording, an envelope fit
  takes over (`modal::extract_fit`, `FitKind::Envelope`): spaced spectral
  peaks with their own envelope decays, levelled to the recording's peak.
  Real door slam: modes at its level, modes + residual 0.37 dB from it.
- Sourcing the WAVs is the human's call (own recordings or CC0); the repo
  ships none. **Open decision for the human**: which library and licence.

## Phase 5: a target to aim at

- **Reference sets**: `<project>/refs/<category>/*.wav`. `listen` and the
  `reference` check accept `reference: {set: "impacts"}`: compare against each
  (`reference::compare`, `crates/sfx_analysis/src/reference.rs:356`), report
  the nearest and the band-by-time cells that differ from the set's median.
- AGENTS.md step 3 becomes "`listen` with a reference when the project has one".
- **Done (2026-11-17).** As built: a separate `reference_set` (not
  `reference: {set}`, whose `file` is required) on `listen` and `verify`.
  Instead of the median band x time cells, eight features per recording
  (weight / body / crack shares, crest, tail, centroid, voiced, ringing)
  checked against the set's span with a tolerance (a quarter of the span, at
  least a floor), plus every distance and the nearest against the set's own
  spread (Phase 0 showed the distance saturates). On three CC0 door slams
  (`refs/door_slam/`, local): slam_a outside on 5 features, slam_b on 3
  (crest, centroid, ringing), slam_b2 inside on all 8. The crest floor had
  to be 3 dB (a synthetic in-family hit read 23.3 dB against a 19.6-20.9
  span: noise crest moves with the seed).
- Out of scope here, noted for later: an audio-capable critic model or an
  embedding distance. Needs an external service; decide separately.

## Phase 6: models and exaggeration (largest effort, only if Phase 0 says so)

- `modal`: `strike_glide` (pitch drops N cents over the first tens of ms on a
  hard strike) and `rattle` (a threshold-gated buzz on the output); off by
  default, hash-pinned.
- New ready-made nodes for the exaggeration layers: `subdrop` (sine sweep with
  its own envelope) and `riser` (noise + pitched swell ending at `at`), each a
  finished layer like `click` (pattern: `crates/sfx_graph/src/nodes/mechanism.rs:110`).

- **Done (2026-11-17).** As built (`docs/nodes/phase6.md`): `strike_glide`
  follows the ring's level (not a fixed time), so the glide is full only at
  the loudest instant (100 cents measured +45.5 at 20-110 ms); `rattle`'s gap
  is set from the ring's loudest 5 ms level (the sample peak, an impulse
  response's first spike, left almost nothing over the gap) and its buzz
  gain is 8 (at 2 it sat 30 dB under a wood body). `subdrop` and `riser` in
  `nodes/exaggerate.rs`. A cinematic hit with a `subdrop` reads voiced 0.58,
  so the `layers` impact preset fails it by design: documented. Not heard.

- **Listening (2026-11-17)** and the fix that followed: `strike_glide` ~35
  cents helps; `rattle` on wood reads as plastic; `riser` / `subdrop` were
  off-brief ("cinematic" meant production-quality foley). Clean `modal` read
  tonal again (a marimba, a tuned gong), so `modal` gained `noise` (each
  mode a band of noise): wood voiced 0.89 to 0.17, metal 0.92 to 0.00
  (`docs/nodes/phase6.md`). Three rounds: round 1 "right direction"; round
  2 the lid still tonal, because `strike_glide` dropped `noise` (fixed) and
  modulating the rings left clean lines (now real noise bands, plus
  `noise_width`); round 3 "much better" (wood 0.6 / tilt 4 / loss 1.8, thin
  metal 0.85 / width 0.5 / tilt 6).

## Verification (every phase)

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all
cargo run --example door_slam /tmp/sounder   # Phase 1 onward
```

Then over MCP against the real server: `new_sound`, build the door slam with
the phase's feature, `listen`, `verify {seeds: 4}`, `catalog save`, and leave
it open in the window for the human with macros on it. Each phase ends with
the human's verdict against `slam_a` from Phase 0; a phase the human cannot
hear a gain from is reported as such, not as a success.
