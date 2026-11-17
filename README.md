# sounder

Procedural sound effects that AI agents build over MCP, the audio sibling of
`modeler`. A sound is a JSON graph of nodes (oscillators, noise, envelopes,
filters, delays, reverb, waveshapers, mixers...) rendered offline to WAV. The
agent cannot hear, so the analysis ("senses": descriptors, spectrogram images,
PASS / FAIL checks) matters as much as the synthesis, and the human's comments
are the only real ears.

Full plan: `~/.claude/plans/i-want-something-very-majestic-balloon.md`.

## Layout

```
crates/sfx_graph/     synthesis core: SoundDoc, parse, levels, parallel evaluator,
                      Merkle memo cache, 35 node types, loudness, WAV IO.
                      No Bevy, no MCP, no IO beyond WAV files.
crates/sfx_analysis/  descriptors, the listen PNG, checks (verify / expect).
src/                  history, session (open sounds, per-agent active sound, comments,
                      knob moves, autosave), catalog, export, friction log.
src/bin/sounder/      the MCP server (stdio): tools, bridge, main.
mcp_driver.py         proxy / daemon / shell client (copied from modeler).
design/               window-concept.html: the phase 5 window mock (it plays).
tests/                session tests; the driver-script test (phase 2 check).
examples/laser.rs     a laser zap: writes laser.wav + laser.png, prints the
                      descriptors and the default checks.
```

```bash
cargo run --example laser /tmp/sounder
```

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
```

## Status

**Phase 2 (MCP server, session, macros): done.** See "Phase 2 notes" below.

**Phase 1 (core + analysis, no server): done.**

- `sfx_graph`: `SoundDoc { name, kind, sample_rate, duration, seed, nodes,
  params, inputs, version }`, `SoundDoc::apply` (atomic upsert batch, type
  changes refused, `{"type": "delete"}`, renders for real so a failure leaves
  the doc untouched, `changed: false` when nothing changed, labels `+a ~b -c`),
  `SoundDoc::render` (auto duration: 2 s doubling to 30 s, trailing silence
  trimmed, output stage re-run at the final length), warnings (unread nodes,
  above-Nyquist frequencies, a squashed output input, silent output, DC, never
  decays, mostly silence, missed loudness target).
- `sfx_analysis`: `describe` (levels, true peak, LUFS, attack / decay, silence,
  spectral-flux onsets, YIN pitch, harmonicity, centroid / rolloff / flatness
  per third, 8 octave bands x 5 segments, modes, stereo), `listen_png` (columns,
  zoom), `checks::run` with every check from the plan except `reference`.
- Tests: every node type has a parseable example and renders; identical bytes
  at 1, 3 and 8 threads; 1 kHz sine pitch within 5 cents and centroid ~1 kHz;
  -18 dBFS sine at -21.01 LUFS within 0.2 (48 and 44.1 kHz); white noise
  flatness > 0.9; `silent_edges` fails on a cut buffer; parse errors name the
  node and field; loop seams, clicks, variations, rebuild, bands, pitch.

## Phase 1 notes (differences from the plan, and why)

- **History and annotations are not in `sfx_graph`.** The plan's `SoundDoc`
  has `history: History<SoundDoc>` and `annotations`; both live in the session
  layer in `src/` (`Sound` wraps `SoundDoc`; a history step is recorded when
  `ApplyReport::changed` is true, labelled `ApplyReport::label`). `apply` takes
  a `DocEdit` and `RenderOpts` (cache, seed override, macro overrides,
  `base_dir` for relative `sample` / `convolve` files: the project dir).
- **`expect` is not run by `apply`** (analysis depends on the graph, not the
  other way round): the tool layer calls `sfx_analysis::run(&expect,
  &CheckCx { doc, rendered: &report.rendered, opts })` after `apply`.
  `Expect::defaults()` is the no-argument `verify`.
- **Feedback (phase 3).** Each DSP piece is a state struct with a `tick` in
  `sfx_graph/src/dsp.rs` (Osc, Svf, OnePole, Resonator, DelayLine, Follower),
  but nodes evaluate as whole blocks and any cycle is an error naming the
  nodes. Phase 3 adds Tarjan SCCs and a per-sample evaluator for components
  through a `delay`, reusing those structs. `delay` and `comb` have an internal
  `feedback` field meanwhile.
- **Stereo.** Processor nodes (`filter`, `delay`, `shape`, `gain`, `output`,
  ...) take stereo on `input` and process each channel; modulation fields
  (`freq`, `q`, `amp`, `pan`...) must be mono and say to add a `mix` with
  `"to": "mono"` otherwise.
- **`wander`** joins seeded random points with Catmull-Rom curves instead of
  porting foley's `Wobble` (low-passed noise): bounded, and its `rate` means
  what it says.
- **foley.** Only `foley_dsp::Rng` is used so far (git dep pinned to
  `963d67e`). The BS.1770 loudness is ported from `foley-sim/src/analysis.rs`
  but derives the K-weighting for any sample rate (foley hard-codes 48 kHz).
  The `clicks` check is foley's step detector (12x the local step RMS), not the
  kurtosis "crackle" measure.
- **Output loudness.** `output.lufs` normalizes, then the `ceiling_db`
  lookahead limiter runs; on a very peaky sound the limiter takes some
  loudness back, and the render warns when the target is missed by over 1 LU.
- **Not built yet:** the `reference` check and reference compare (phase 4),
  `extract_modes` (phase 4), `listen` `diff` (phase 2: needs the previous
  render, which the session keeps).
- Copied from modeler: `params.rs` (verbatim), the node macro / strict parsing
  / `refs` / `example` / `describe` pattern, `eval.rs` structure, the
  `MatDoc::apply` flow, `raster.rs` (canvas, 5x7 font, PNG).

## Phase 2 notes

- **Tools: 16, not 15.** The plan's table names 16 (`undo`, `redo`, `jump`,
  `history` count four); all are implemented. `extract_modes` is implemented
  now (strongest spectral peaks and their decay as a `resonators` node);
  phase 4 can refine it. `play` returns the headless error. `listen`'s
  `reference` returns "arrives with phase 4".
- **Macros replace `inputs`** (plan: "Macros"). `sfx_graph::macros`: `Macro`,
  `MapEntry`, `apply` (on param-resolved nodes before evaluation),
  `validate` (names, ends, duplicate writers, and the graph must parse with
  every macro at 0 and at 1). The `macros` check is in `sfx_analysis::checks`
  (measures in `sfx_analysis::measure`), part of the defaults when a sound has
  macros. `claim` is required. `value` is where the knob sits, `default` the
  agent's setting; `Session::move_knob` (for the window) records `KnobMove`s
  that `summary` reports once and clears. Moves are not history steps.
- **The bridge** (`src/bin/sounder/server/bridge.rs`): the MCP server runs on
  its own thread and tokio runtime; the `Session` lives on another and runs
  closures sent over a channel (`Cmd::Exec`). Phase 5 drains the same channel
  from the window's frame loop instead of `run_headless`.
- **`sounder` without `--headless`** exits with a message until phase 5. The
  driver serves every mode headless. `--print-catalog-dir` and
  `--feedback-report` are there for the driver and a shell.
- **`listen diff`** shows before / now / their residual (now - before), not a
  dB-difference spectrogram with a diverging colour map. Only for single-column
  requests. `listen` shows at most 8 columns.
- **`verify`** returns an error result when any check fails (so agents notice);
  `sound_ops expect` does too but keeps the batch.
- **History snapshots** are `Step(SoundDoc)` (the orphan rule keeps the copied
  `Doc` trait off `SoundDoc`); `History` is modeler's, with `Result<_, String>`
  errors. Annotations, knob moves and caches sit outside the snapshots, so undo
  never un-comments.
- **Tests:** `tests/session.rs` (history, attribution, knob moves, comments,
  atomic batches, autosave / restore, catalog, export) and
  `tests/mcp_driver.rs` (the phase 2 check: `mcp_driver.py --calls
  tests/calls/phase2.json` against `sounder --headless`; needs `python3`, skips
  without it).
- **Not done:** the per-client `_meta` project switching works
  (`sounder/project`) but only the single-project case is tested; `session.lock`
  liveness uses `kill -0` through the shell tool (no `unsafe`), fine on
  macOS / Linux.

## Phase 3 notes

- **`expr` (lane A).** `src/calc.rs` is the one expression compiler (flat
  stack bytecode, f64, constant folding) shared by `"=..."` param
  expressions and the `expr` node (`nodes/expr.rs`: `ExprProg` + `ExprState`
  with a per-sample `tick` for the future feedback evaluator). `expr`'s
  `out` / `update` text is exempt from `$` / `=` resolution and from node
  refs (only `inputs` are refs). See "expr" in `AGENTS.md`.
