# Graph features

Feedback loops, the offline spectral nodes, event streams, `expr`, `voices`
and `sound` instances. Moved out of `AGENTS.md`; the rules there still apply.

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
(see "Spectral additions" in `docs/nodes/README.md`).

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
"Grains and clouds" in `docs/nodes/README.md`).

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
  (`docs/nodes/water.md`).
