# <task id>: <node names>

<!-- Copy this file to docs/nodes/<task>.md. One section per node, in the
order below. Never write that a node "sounds" like anything: say what was
measured and by which test (file and test name). Delete these comments. -->

## `<node>`

One sentence: what the node does and its family (source, filter, ...).

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `input` | signal | required | mono or stereo audio | |
| `freq` | signal | required | Hz, 1 .. 0.49 x sample rate | |
| `mode` | word | `lp` | `lp` `hp` `bp` | |

A signal field takes a number, `"$param"`, `"=expr"` or the name of another
node. Optional fields are shown without the `required`. State the cap of
every count, size and factor, and the error it gives past it.

### Recipe

One recipe that builds a useful sound from this node, as JSON that renders as
written (use the stand-in names from rule 9 only if it is also the node's
`example`), then one line on what to listen for and which macro to give the
human.

```json
[
  {"type": "<node>", "name": "x", "input": "src"}
]
```

### Measured

What the tests in `crates/sfx_analysis/tests/<task>.rs` found, with numbers
(copy the `println!` lines): level, frequency, decay time, alias level,
latency in samples, CPU-relevant caps. One line each, naming the test.

| What | Measured | Test |
|---|---|---|
| 1 kHz in, 300 Hz ring: peaks | 698.4 and 1300.1 Hz | `ringmod_sidebands` |

### Honest limits

What it cannot do, where it aliases or blows up, what was not measured, and
that none of it has been heard. Include the extreme-value behaviour (finite
output at the ends of each field's range) and anything that was `#[ignore]`d
with its measured value.
