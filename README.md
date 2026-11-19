# sounder

Procedural sound effects that AI agents build over MCP, the audio sibling of
`modeler`. A sound is a JSON graph of nodes rendered offline to WAV / Ogg. The
agent cannot hear, so the analysis (the "senses": descriptors, spectrogram
images, PASS / FAIL checks) matters as much as the synthesis. The human
listens in a window, comments, and turns the knobs the agent gives them.

```bash
cargo run                               # window + MCP server on stdio
```

```bash
cargo run -- --headless                 # MCP server only
```

```bash
cargo run --example laser /tmp/sounder  # a laser zap: WAV, PNG, descriptors
```

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
```

## Layout

```
crates/sfx_graph/     synthesis core: the document, the evaluator, every node
                      (src/nodes/, one file per family). No MCP, no IO but WAV.
crates/sfx_analysis/  the senses: descriptors, the listen image, checks,
                      reference compare. Its tests also cover the nodes,
                      because they need these measurements.
src/                  session, history, catalog, export, player.
src/bin/sounder/      the binary: MCP server (server/) and the window (window/).
ui/                   the window's page (embedded at build time).
templates/            sounds `new_sound {template}` starts from.
parts/                built-in layer-sized sounds `catalog search` offers.
examples/             worked sounds that render and check themselves.
tests/                session, server and driver tests.
mcp_driver.py         proxy / daemon / shell client for the server.
docs/nodes/           per-node reference: fields, limits, measurements.
docs/history/         how it was built: plans, test rounds, listening notes.
tools/labels/         blind-label batches: generators and the cue graphs.
design/               the window API contract and mock, the node roadmap.
```

`AGENTS.md` is the reference for agents using or changing the tool. `docs/STATUS.md` says
what works, what is missing and what comes next.

Local state that is not in git: `.sounder/` (the sound library and session),
`assets/` (exports and source recordings), `refs/` (reference recordings).
