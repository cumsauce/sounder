# Tests

One test binary, `it/`, with a module per node family or sense
(`it/modal.rs`, `it/water.rs`, `it/onsets.rs`, ...). The node tests live in
this crate, not in `sfx_graph`, because they measure what a node renders with
the senses defined here (spectrum, pitch, onsets, loudness).

```bash
cargo test -p sfx_analysis                 # everything
```

```bash
cargo test -p sfx_analysis --test it modal::   # one family
```

- `it/common.rs`: shared helpers (`render`, `node_audio`, `spectrum`,
  `band_db`, `assert_bit_identical`, ...). Use them in new tests.
- `it/recipes.rs` renders every marked JSON recipe in `AGENTS.md`, `docs/graph.md`
  and `docs/nodes/README.md`.
- `fixtures/`: sound documents some tests load.
- Many tests print the numbers they measure (`-- --nocapture`); the figures
  quoted in `docs/nodes/*.md` come from those lines.
- Tests named `*_is_bit_identical` or that compare a hash pin a render: a
  change to them means a sound changed.
