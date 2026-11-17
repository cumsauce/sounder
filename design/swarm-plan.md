# Swarm plan: building the node roadmap with parallel Sonnet agents

Implements `design/node-roadmap.md`. The orchestrator is the Sonnet session
the owner starts with "run design/swarm-plan.md"; it runs the whole plan,
waves and test rounds, **start to finish with no further input** (section 0).
Each task card below is one Sonnet agent in its own git worktree and branch,
all sharing one cargo target directory. Written 2026-11-17 against commit
`f0ea79a`.

Contents: [running unattended](#0-running-it-unattended) ·
[why a prep wave](#1-why-a-prep-wave) ·
[rules for every worker](#2-rules-for-every-worker) ·
[wave 0](#3-wave-0-prep-serial-blocks-everything) ·
[wave 1](#4-wave-1-synthesis-and-processing-12-agents) ·
[wave 2](#5-wave-2-models-8-agents) ·
[wave 3](#6-wave-3-spectral-granular-worked-examples) ·
[integration](#7-integration-orchestrator) ·
[worker brief](#8-worker-brief-template) ·
[test and feedback rounds](#9-test-and-feedback-rounds)

## 0. Running it unattended

The owner starts this once and is not asked anything again. Every decision
that could need them is made here.

**Never ask.** Do not use a question tool, do not end a turn to wait for an
answer, do not stop at a wave boundary. When something is unclear, take the
default below, write one line about it in the journal, and go on.

| Situation | Default |
|---|---|
| A worker fails or returns without the three commands passing | Send it the failure once (same agent, same worktree). Fails again: mark the task `deferred` in the journal, remove its worktree, go on without it. |
| A card's tolerance cannot be met | The worker keeps the test `#[ignore]`d with the measured value (section 8). Accept the branch; list it in the final report. |
| A merge conflict outside the slot lines | Rebase the later branch by hand if it is under 20 lines of conflict; otherwise send it back to its worker with the conflict. |
| `cargo test --workspace` fails on the integration branch after a merge | Revert that merge, send the failure to the worker once, then defer. The integration branch is never left red. |
| A later task needs a deferred one | Defer it too and say why. |
| Network is down for the first build | Retry three times ten minutes apart, then stop and write the final report with what exists. |
| Disk under 10 GB free | Remove finished worktrees and old round binaries; still under: stop launching, finish what runs, report. |
| Anything that would touch `main`, a remote, or files outside the repo, `/tmp` and the scratchpad | Do not do it. |

**Where the work lands.** Create branch `swarm/main` from `main` at the
start. Every merge goes to `swarm/main`. Never commit to `main`, never push,
never delete a branch that is not `swarm/*`. The owner merges `swarm/main`
when they have listened.

**One shared target directory.** Worktrees are created under the repo
(`git worktree add .swarm/wt/<id> -b swarm/<id> swarm/main`; the harness's
own worktrees under `.claude/worktrees/` also qualify). Cargo reads config
from every parent directory, so one untracked file at the repo root points
them all at the same place:

```sh
mkdir -p .cargo .swarm && printf '[build]\ntarget-dir = "%s/target"\n' "$PWD" > .cargo/config.toml
printf '.cargo/\n.swarm/\n' >> .git/info/exclude
```

Check it before launching anyone: in a fresh worktree,
`cargo metadata --format-version 1 --no-deps | grep -o '"target_directory":"[^"]*"'`
must print the repo's `target`. If it does not, put
`CARGO_TARGET_DIR=<repo>/target` in front of every cargo command in every
brief instead (shell variables do not survive between commands).

What sharing means in practice:

- Third-party crates compile once. Each worktree's own three crates still
  compile separately (cargo keys them by path), which is correct: branches
  differ.
- Cargo locks the directory while compiling, so builds queue. "Blocking
  waiting for file lock on build directory" is normal: wait, never kill it,
  never delete the lock.
- **Only the orchestrator builds the root crate.** Every worktree's `sounder`
  binary would land on the same `target/debug/sounder` and overwrite the
  others. Workers build and test `-p sfx_graph -p sfx_analysis` only. The
  orchestrator builds the binary from `swarm/main` and copies it out
  (section 9) before anyone runs it.
- Do the first full build once, alone, before the first launch, so twelve
  agents do not queue behind a cold dependency build.

**Concurrency.** At most 8 building workers at once (they queue on the lock
anyway), at most 12 sound-building testers at once (they do not compile).

**Journal.** Keep `design/rounds/JOURNAL.md`: one line per event (launched,
merged, deferred, round started, scores, stop decision) with the branch or
file it refers to. Update it before and after every step. If the session is
resumed or its context is summarized, read the journal first and continue
from the last line; never restart a finished step.

**Order of the whole run.**

1. Setup above; first full build; branch `swarm/main`.
2. Round 0 (baseline, section 9) and wave 0 at the same time: testers do
   not compile, so they do not compete with the prep tasks.
3. Waves 1 and 2 together (no shared files), then wave 3 (X1 to X3).
4. Rounds 1 to at most 5 (section 9), each followed by its fixes.
5. Final report (section 9.7), then stop.

## 1. Why a prep wave

Adding a node today edits five shared places, so twelve agents would collide
on every one:

| Shared place | What a new node adds there today |
|---|---|
| `crates/sfx_graph/src/node.rs` | a `node_types!` line, a `blurb` arm, an `example` arm, sometimes a `WORDS` entry in `refs` |
| `crates/sfx_graph/src/nodes/mod.rs` | `mod x;` and `pub use x::*;` |
| `crates/sfx_graph/src/dsp.rs` | any shared primitive |
| `crates/sfx_graph/tests/graph.rs` | `every_node_type_renders_its_example` has a hard-coded stand-in list and event-type list |
| `AGENTS.md` and the MCP `INSTRUCTIONS` (`src/bin/sounder/server/tools.rs`) | the node's docs |

Wave 0 reduces this to **one line in `node_types!` and two in `mod.rs`** per
task, both in a slot reserved for that task. Everything else lives in files
the task owns.

## 2. Rules for every worker

1. **Stay in your files.** Each card lists the files you own. Outside them
   you may add exactly: your slot line in `node_types!`, your slot lines in
   `nodes/mod.rs`. Never touch `AGENTS.md`, `README.md`, `tools.rs`,
   `dsp.rs`, `feedback.rs`, `eval.rs`, `doc.rs`, or another task's files
   unless your card says so. If you need a change elsewhere, stop and put it
   in your report under "needs from integrator".
2. **Node shape.** A strict serde struct (`#[serde(deny_unknown_fields)]`,
   `schemars::JsonSchema`), a `///` comment on every field giving unit,
   default and range (it becomes the `describe_op` schema), `impl Eval`, and
   the `Describe` impl from wave 0 (blurb + example). Signal-capable fields
   are `In` / `Option<In>` read with `cx.mono` / `cx.audio` / `cx.mono_or`.
   Copy the style of `nodes/nonlinear.rs` (`Shape`).
3. **Flags.** Override `causal()` to `false` if the node needs lookahead or
   the whole buffer. Override `random()` to `true` if it reads the seed
   (use `cx.rng(self.seed)`; never another RNG, never time or thread ids).
4. **Length.** Every audio output is exactly `cx.n` samples. Stereo in is
   handled per channel (`per_channel`) or refused with `cx.mono`.
5. **Enum-string fields** must be named one of `mode shape curve color
   window material model preset style` (these are not read as node
   references). Anything else: declare it in `Describe::WORDS`.
6. **Errors** name the field, give the value, and say what to do:
   `"slope: 18: give 12, 24 or 48"`. Use `positive` / `non_negative` from
   `nodes/mod.rs`.
7. **Work caps.** Any count, size or oversampling factor a caller can set
   gets an upper bound that errors (not clamps) with the limit in the
   message. No allocation proportional to an unchecked field.
8. **No non-finite output.** Guard divisions and logs; a filter that can blow
   up is clamped to a stable range and the test proves it at the extremes.
9. **Example.** `Describe::example()` may only reference these stand-in
   names: `src noise1 hit burst amp_env lfo1 sweep cutoff_env tone steps
   drums n1 col verb` (stereo) `body click mix1 ev` (events) `contact1`.
10. **Tests** go in your own file `crates/sfx_analysis/tests/<task>.rs`, use
    `tests/common/mod.rs` (wave 0), and **measure**: each card lists the
    assertions. Always include: bit-identical on a second render; finite
    output at each field's extremes; one error-message test. `println!` the
    measured numbers; they go in your report.
11. **Docs** go in `docs/nodes/<task>.md` using `docs/nodes/TEMPLATE.md`:
    fields table, one recipe per node, measured numbers, honest limits. Never
    write that something "sounds" like anything; say what was measured.
12. **Done means** these pass in your worktree, and you paste their tails:
    ```sh
    cargo fmt --all
    cargo clippy -p sfx_graph -p sfx_analysis --all-targets -- -D warnings
    cargo test -p sfx_graph -p sfx_analysis
    ```
    Run each as `cd <your worktree> && cargo ...` (the working directory
    does not carry between commands). The target directory is shared with
    every other worker: "Blocking waiting for file lock" means another build
    is running; wait. **Never `cargo build` or `cargo run` the root crate**; `cargo check` and
    `cargo clippy -p sounder --all-targets` are fine. Only a task whose card
    owns root files (F1) may also run `cargo test -p sounder`. Never `cargo clean`. Commit to your branch;
    do not push, do not merge.
13. **Report** (final message): nodes added, files touched, measured numbers,
    anything skipped and why, "needs from integrator".

## 3. Wave 0: prep (serial, blocks everything)

Small, but every later task rests on it. Run by the orchestrator or one
agent at a time, reviewed before wave 1 starts. T0a and T0b touch different
files and can run together; T0c after both.

### T0a: registry so a node lives in one file

Files: `node.rs`, `nodes/*.rs` (mechanical), `nodes/mod.rs`,
`tests/graph.rs`.

- Add to `node.rs`:
  ```rust
  pub trait Describe {
      const BLURB: &'static str;
      /// Field names whose string values are words, not node references.
      const WORDS: &'static [&'static str] = &[];
      fn example() -> serde_json::Value; // the body, without type / name
  }
  ```
  Implement it beside every existing struct, moving the text out of
  `blurb()` and `example()` unchanged. `node_types!` generates `blurb`,
  `example` and a per-type words lookup from the trait. `refs()` unions the
  global list with the type's `WORDS` (the body carries `type`).
- Add `model preset style` to the global words list.
- In `node_types!` and in `nodes/mod.rs`, add one slot comment per task id
  of waves 1 to 3 (`// slot S1` ... `// slot X3`), each followed by a blank
  line, so parallel one-line insertions merge without conflict.
- `tests/graph.rs` `every_node_type_renders_its_example`: decide "is an
  event stream" from the rendered `Buf` kind, not the hard-coded list; skip
  list stays for file-needing nodes.
- Check: `cargo test --workspace` passes; `describe_op` output for three
  types is byte-identical to before (snapshot it first).

### T0b: shared DSP and the test kit

Files: `dsp.rs` (append only), new `crates/sfx_analysis/tests/common/mod.rs`,
new `docs/nodes/TEMPLATE.md`.

- `dsp.rs` additions, each with a unit test:
  - `Biquad` (RBJ cookbook: lp hp bp notch peak lowshelf highshelf; `set`,
    `tick`), for cascades and EQ.
  - `Oversampler` (2x / 4x / 8x polyphase half-band up and down;
    `process(&mut self, x, f: impl FnMut(f32) -> f32) -> f32`). Test: a
    5 kHz sine through `clip` at drive 8 has at least 30 dB less energy at
    non-harmonic bins with 4x than with 1x.
  - `Hilbert` (two allpass chains giving a 90 degree pair, 20 Hz to
    20 kHz within 1 degree at 48 kHz).
  - `Lagrange3` fractional delay read on `DelayLine` (modulated delays
    without the linear-interp high loss).
  - `Smooth` (one-pole parameter smoother in seconds).
- `tests/common/mod.rs`: `render(nodes, duration) -> Audio`,
  `node_audio(nodes, name, duration) -> Vec<f32>`, `assert_bit_identical`,
  `spectrum(x, sr) -> Vec<(hz, db)>`, `peak_hz`, `band_db(x, sr, lo, hi)`,
  `alias_db(x, sr, f0)` (energy off the harmonic grid of `f0` relative to
  on it), `rms_db`, `assert_finite`.
- `TEMPLATE.md`: the doc fragment layout from rule 11.

### T0c: one worked node as the pattern

Files: new `nodes/ringmod.rs`, `tests/ringmod.rs`, `docs/nodes/ringmod.md`.

`ringmod {input, freq (signal), [shape sine], [mix 1]}`. Test: a 1 kHz sine
with `freq` 300 has peaks at 700 and 1300 Hz and under -60 dB at 1 kHz.
This is the reference every brief points at; get it reviewed.

## 4. Wave 1: synthesis and processing (12 agents)

No task depends on another. All need wave 0 merged.

| Id | Owns | Nodes / changes |
|---|---|---|
| S1 | `nodes/sources.rs`, `dsp::Osc` only | `osc` sync, unison, phase mod; `noise` colours |
| S2 | new `nodes/synth.rs` | `fm`, `wavetable` |
| S3 | `nodes/nonlinear.rs`, the `Shape` arm of `feedback.rs` | `shape` oversampling and curves; `compress.key` |
| S4 | `nodes/filters.rs`, the `Filter` arm of `feedback.rs` | `filter.slope`, `ladder`, `eq` |
| S5 | new `nodes/dynamics.rs` | `transient`, `gate`, `sub`, `exciter` |
| S6 | new `nodes/modfx.rs` | `chorus`, `flanger`, `phaser`, `freq_shift` |
| S7 | new `nodes/motion.rs` | `doppler`, `haas`, `decorrelate` |
| S8a | `sfx_analysis/src/{checks,measure}.rs` | aliasing, inharmonicity, modulation, roughness |
| S8b | `sfx_analysis/src/descriptors.rs` | round-0 descriptor gaps (section 4b) |
| S9 | new `nodes/modulators.rs` | `chaos`, `fbm`, `curve`, `switch`, `pitch_follow`, `centroid_follow` |
| S10 | new `nodes/sequence.rs` | `trigger_env`, `seq`, `notes`, `event_gate` |
| S11 | `nodes/time.rs`, new `nodes/space.rs` | `reverb.model`, `ir` |
| S12 | new `nodes/lofi.rs` | `crossfade_loop`, `resample`, `tape`, `stutter` |

### S1: oscillator and noise

- `osc` gains `[sync]` (a mono signal: each rising zero crossing resets the
  phase, band-limited with polyBLEP at the reset), `[unison 1]` (1..16
  voices), `[detune 0]` (cents, spread across voices, seeded start phases;
  then `random()` is true when unison > 1), `[spread 0]` (stereo when > 0),
  `[pm]` (phase modulation in cycles, a signal).
- `noise` gains colours `blue`, `violet`, and `crackle` (sparse signed
  impulses); `density` becomes `In` (events per second for `crackle` /
  `velvet`, a signal).
- Tests: synced saw at master 110 Hz, slave swept 200 to 2000: YIN pitch
  stays 110 within 1 %; `alias_db` under -50 dB. Unison 7 detune 30: one
  spectral cluster around f0 at least 20 cents wide, RMS within 3 dB of
  one voice times sqrt(7). `pm` by a sine at ratio 2, index 1: sidebands at
  f0 +- 2 f0 present. Blue rises 3 dB / octave within 1 dB over 200 Hz to
  10 kHz, violet 6. Crackle at density 50: 40 to 60 impulses in 1 s;
  a ramped density doubles the count in the second half.

### S2: `fm` and `wavetable`

- `fm {freq (signal), [ratio 1], [index 0] (signal), [feedback 0],
  [mod_ratio 1], [mod_shape sine], [amp]}`: one carrier and one modulator,
  phase modulation, operator self-feedback with the usual two-sample
  average. Carrier Hz = `freq * ratio`, modulator Hz = `freq * mod_ratio`.
- `wavetable {freq (signal), harmonics: [..], [harmonics_to: [..]],
  [morph 0] (signal), [phases], [amp]}`: additive single cycle from
  harmonic amplitudes (cap 256); harmonics above Nyquist at the current
  `freq` are dropped per sample block (mip tables or direct additive, your
  choice; state which).
- Tests: `fm` index 0 is a pure sine (THD under -80 dB); index 2, ratio
  1:1: sideband levels match Bessel J0..J3(2) within 1 dB; ratio 1:1.4
  gives non-harmonic partials (S8 not needed: assert peaks at
  `fc +- k fm`). `wavetable` `[1, 0, 0.5]`: peaks at f0 and 3 f0, 6 dB
  apart, nothing else above -70 dB; at f0 = 5 kHz with 32 harmonics,
  `alias_db` under -60 dB; `morph` 0 to 1 moves the centroid monotonically.

### S3: `shape` and sidechain

- `shape` gains `[oversample 1]` (1, 2, 4, 8 via `dsp::Oversampler`),
  modes `asym` (`[bias]`), `diode`, `tube`, `table` (`points: [[x, y]..]`,
  linear between, odd-extended when only x >= 0 is given), and `[mix 1]`.
  Inside a feedback loop, `oversample` > 1 is an error naming the field
  (update the `Shape` ticker in `feedback.rs`; new modes must work there).
- `compress` gains `[key]` (a signal that drives the detector instead of
  the input).
- Tests: 5 kHz sine, `clip`, drive 8: `alias_db` improves by at least
  22 dB (below 20 kHz) from oversample 1 to 4 (T0b measured 24.8 dB for a
  hard clip at drive 8: the 30 dB figure in the first draft was not
  reachable, the clip's own 1/k^2 spectrum folds back at any filter quality). `asym` produces a 2nd harmonic above
  -30 dB where `tanh` has it under -70 dB. `table` with the identity is
  the input bit for bit. `key`: a steady tone keyed by a 4 Hz pulse train
  shows gain dips at the pulses of the stated ratio within 1 dB.

### S4: filters and EQ

- `filter` gains `[slope 12]` (12, 24, 48: cascaded `Svf` / `Biquad`
  sections with Butterworth Q spread; `q` applies to the last section).
  In a loop, slope other than 12 errors naming the field.
- `ladder {input, freq (signal), [resonance 0..1.2] (signal), [drive 1],
  [mode lp|bp|hp]}`: four-pole nonlinear (tanh per stage), zero-delay or
  half-sample-compensated; self-oscillates at resonance >= 1.
- `eq {input, bands: [{mode, freq, [gain_db], [q]}..]}` (cap 16 bands),
  modes as `filter`.
- Tests: white noise through lp 1 kHz: level an octave above the cutoff is
  -12, -24, -48 dB within 2 dB by slope. `ladder` resonance 1.1 with a
  silent input after an impulse rings at `freq` within 2 % for 0.5 s;
  finite at freq 20 Hz and 20 kHz, drive 20. `eq` with one +6 dB peak at
  1 kHz measures +6 within 0.5 dB there and 0 within 0.5 dB two octaves
  away; an empty `bands` is the input bit for bit.

### S5: dynamics

- `transient {input, [attack_db 0], [sustain_db 0], [speed 0.02]}`: fast
  and slow followers, gain from their difference.
- `gate {input, threshold_db, [ratio inf], [attack 0.001], [release 0.05],
  [hold 0.01], [key]}`: downward expander.
- `sub {input, [octaves 1|2], [mix 0.5], [lowpass 120]}`: zero-crossing
  divider of the low-passed input shaped to a sine-like wave, following the
  input's envelope.
- `exciter {input, [freq 3000], [drive 2], [mix 0.3]}`: high-passed,
  shaped, high-passed again, summed.
- Tests: `transient` attack +6 on a noise burst with a 20 ms rise raises
  crest factor by 3 dB or more and leaves the level 200 ms in within 1 dB.
  `gate`: a -50 dB noise bed under -10 dB bursts drops to under -80 dB
  between bursts while burst peaks move less than 0.5 dB. `sub` on a
  220 Hz saw: a 110 Hz peak within 12 dB of the fundamental. `exciter` on
  a 1 kHz sine: nothing new below 2 kHz above -70 dB, harmonics above
  3 kHz appear.

### S6: modulation effects

- `chorus {input, [rate 0.8], [depth 0.003], [delay 0.02], [voices 3],
  [spread 1], [mix 0.5]}` (stereo out when spread > 0), `flanger {input,
  [rate], [depth], [delay 0.003], [feedback -0.95..0.95], [mix], [manual]
  (a signal that replaces the lfo)}`, `phaser {input, [rate], [stages 4|6|
  8|12], [center 800], [depth octaves], [feedback], [mix]}`. Delay reads use
  `Lagrange3`.
- `freq_shift {input, shift (Hz, signal), [mix 1]}`: `dsp::Hilbert`
  single-sideband.
- Tests: `flanger` with `manual` fixed at 1 ms and mix 0.5 on white noise:
  notches every 1 kHz starting at 500 Hz, at least 15 dB deep. `phaser`
  4 stages, fixed: exactly 2 notches. `chorus` on a 440 Hz sine: energy
  spreads to +-(depth * rate * 2 pi * 440) Hz within 30 %, and L / R
  correlation is under 0.9 at spread 1. `freq_shift` +100 on a 1 kHz sine:
  peak at 1100, the 900 Hz image under -40 dB; shift 0 is within 0.1 dB of
  the input spectrum.

### S7: motion and stereo

- `doppler {input, [speed 30] m/s, [closest 5] m, [at] (time of closest
  approach), [pan true], [air true]}`: straight-line pass. Distance over
  time gives the delay (hence the pitch bend), 1 / r gain, optional
  absorption (call the same foley cutoff `air` uses; ask the integrator if
  it is private), equal-power pan from the bearing. Stereo out when `pan`.
  Also `[distance]` (a signal) to drive it directly instead of `speed`.
- `haas {input, [delay 0.012], [side right]}`; `decorrelate {input,
  [amount 1], [seed]}` (mono to stereo through two different seeded
  allpass / velvet filters, flat magnitude).
- Tests: `doppler` on a 1 kHz sine at 34.3 m/s, closest 1 m: pitch well
  before is 1111 Hz and well after 909 Hz within 1 %; peak level at `at`
  within 5 ms; pan crosses centre at `at`. `decorrelate`: L / R
  correlation under 0.3 on noise, each channel's octave-band levels within
  1 dB of the input, mono sum within 3 dB per band.

### S8a: senses for the new sources

Owns `checks.rs` and `measure.rs` in `sfx_analysis/src` (S8b owns `descriptors.rs`). It adds descriptors and
`verify` checks; it does not touch `tools.rs` (report the new check names
and descriptor lines; the integrator documents them).

- `aliasing {max_db}` check and descriptor line: for a pitched segment
  (YIN confident), energy off the harmonic grid above 4 kHz relative to
  total, in dB. Says "raise `oversample` on shape `x`" when it can name a
  `shape` node upstream; else "likely aliasing".
- `inharmonicity`: mean absolute deviation in cents of the strongest 8
  partials from integer multiples of f0; listed in the descriptor table.
- `modulation`: the 3 strongest rates (0.5 to 200 Hz) of the amplitude
  envelope with depth in dB, and the same for the pitch track.
- `roughness`: envelope energy in 20 to 150 Hz relative to total.
- New macro claim measures `inharmonicity`, `roughness` in `measure.rs`.
- Tests: a naive (non-band-limited) saw at 3 kHz built with `expr` fails
  `aliasing`, the `osc` saw passes. Harmonic tone: inharmonicity under 2
  cents; partials at 1, 2.1, 3.3: over 50. A 6 Hz tremolo of depth 6 dB
  reads 6 Hz within 0.2 and 6 dB within 1. Two sines 40 Hz apart are
  rougher than two 400 Hz apart.

### S9: modulators and control

- `chaos {model logistic|lorenz|henon, rate, [param], [range], [seed]}`
  (bounded, clamped; `rate` is steps or integration speed per second),
  `fbm {rate, [octaves 4], [gain 0.5], [range], [seed]}`.
- `curve {input, points: [[x, y]..], [curve linear|smooth]}` (lookup,
  clamped at the ends), `switch {inputs: [..], select (signal), [fade
  0.005]}` (index, crossfaded; fractional select blends neighbours when
  `fade` is 0 and `blend: true`).
- `pitch_follow {input, [min 50], [max 2000], [smooth 0.02]}` (Hz, holds
  the last confident value; acausal is fine, say so in `causal()`),
  `centroid_follow {input, [window 0.02]}` (Hz).
- Tests: `fbm` spectrum of the control signal falls with frequency and
  stays in `range`; two seeds differ, one seed repeats. `lorenz` stays
  finite and inside `range` for 30 s at any `rate` up to the cap.
  `curve` hits each breakpoint exactly. `switch` between a 200 and an
  800 Hz sine has no click (use `sfx_analysis::clicks`). `pitch_follow`
  on a 200 to 800 Hz exponential sweep is within 2 % after the first
  50 ms.

### S10: events into the main graph

Event streams today feed only `grains` and `voices`. These nodes turn a
stream into control signals. Read streams with `cx.events`.

- `trigger_env {events, [attack 0.002], [decay 0.2], [curve exp],
  [scale_by_value true], [retrigger restart|add]}`: an envelope fired at
  each event.
- `event_gate {events, [length 0.05]}`: 1 for `length` after each event,
  else 0. `seq {events, values: [..], [mode cycle|random|once],
  [glide 0], [seed]}`: steps through `values` on each event, holds between.
- `notes {notes: [[semitones_or_name, beats]..], [bpm 120], [root 440],
  [glide], [gate 0.9]}`: ports `out` (Hz, held), `gate` (0 / 1), and
  `events` is not possible on a port today: if `Outs` cannot carry an
  event buffer on a second port, emit only `out` and `gate` and report it.
  Note names `C4 D#5 Bb3`.
- Tests: `pattern every 0.1` into `trigger_env` gives peaks at 0, 0.1, ...
  within one sample, each of height equal to the event value. `seq` with
  `[100, 200, 300]` cycles in order; `random` is seed-repeatable. `notes`
  `[["A4", 1], ["C5", 1]]` at 120 bpm: 440 Hz for 0.5 s then 523.25 within
  0.1 Hz; an `osc` driven by it reads those pitches with YIN.

### S11: spaces

- `reverb` gains `[model fdn]` (current, default, bit-identical to today),
  `room` (early reflections from a shoebox image-source set, then the fdn),
  `plate` (dense, bright, fast build), `spring` (dispersive allpass chain:
  the chirp), `[er_mix]`, `[width]`.
- `ir {model room|tube|box|cabinet|phone, [size], [absorption], [length],
  [seed]}`: a source node that outputs an impulse response for `convolve`
  (which already accepts a node as `ir`). `room`: image-source early part
  plus exponentially decaying filtered noise with T60 from Sabine; `tube` /
  `box`: a few comb modes from `size`; `cabinet` / `phone`: band-limiting
  filters with a short ring.
- Tests: default `reverb` output is byte-identical to before this task
  (record a hash first). T60 measured from an impulse is within 20 % of
  `decay` for each model. `spring` impulse response: the 4 kHz band
  arrives later than the 500 Hz band. `ir room` energy decay matches
  Sabine T60 within 25 %; `ir phone` passes 300 Hz to 3.4 kHz and is
  30 dB down at 100 Hz and 8 kHz.

### S12: loops and lo-fi

- `crossfade_loop {input, [fade 0.25]}` (acausal): equal-power crossfade of
  the last `fade` seconds into the first, output is the document length and
  wraps cleanly. In `lofi.rs` (`offline.rs` belongs to F3).
- `resample {input, rate (Hz, signal), [antialias false]}` (sample-rate
  reduction with hold), `tape {input, [wow 0.2], [flutter 0.1], [drive 1],
  [hiss_db -60], [tone 8000], [seed]}`, `stutter {input, events, [slice
  0.05], [repeats 4], [reverse false]}` (at each event, repeat the last
  slice; acausal only when `reverse`).
- Tests: a noise-through-`wander`-filter loop of 2 s that fails
  `loop_seam` passes it after `crossfade_loop`, and its level away from the
  seam is unchanged within 0.5 dB. `resample` rate 8000 without antialias
  shows images of a 1 kHz sine at 7 and 9 kHz; with it they are 30 dB
  lower. `tape` wow: `modulation` of the pitch track (or a YIN track) shows
  a sub-2 Hz component of the set depth within 30 %.

## 4b. Round-0 fix tasks (from the baseline feedback)

Round 0 (24 sounds, mean judge score 9.1, mean difficulty 2.0, mean 4.5
failed calls) reported these issues, counted by sounds. Only issues the
section 9.5 rule selects are here; the rest are in `design/rounds/backlog.md`.
Scores are near the ceiling, so from round 1 on **friction (failed calls,
difficulty, blockers) is the main signal**, with score used to catch
regressions. Full evidence: `design/rounds/r0/triage.md`.

### F1: server tools and arguments (root crate)

Owns `src/bin/sounder/server/tools.rs`, `src/session.rs`,
`src/session/window_state.rs` (only if needed), `src/export.rs`, and
`tests/session.rs` additions in a new file `tests/args.rs`.

1. **Unknown arguments are errors** on every tool (24 of 24 sounds hit a
   silent ignore: `describe_op` with `type`, `verify` with `checks`,
   `listen` with `zoom`). The error names the tool, the bad key, and the
   keys it takes. `describe_op` takes `kind` and also accepts `type` and
   `node` as aliases.
2. **`verify` accepts `checks`** and `expect` in the same shape as
   `sound_ops` `expect` (read `crates/sfx_analysis/src/checks.rs` `Expect`
   for the shape): `verify {checks: ["clicks"]}` runs the defaults plus
   those; `verify {only: ["clicks"]}` runs only those; `verify {expect:
   {"duration": {"min": 0.4, "max": 1.2}, "onsets": {...}}}` as in
   `sound_ops`. Document the shape with one example in the tool's
   description.
3. **Top-level `delete: [...]` in `sound_ops` is an error** that shows the
   working form `ops: [{"type": "delete", "name": "x"}]` (it was accepted
   and did nothing). Check that the real form works for several names.
4. **`export` with `variations: N`** warns when two variations render
   bit-identically ("no node reads the seed: the variations are the same;
   add jitter or a seeded node").
5. **A node named `mono`/`stereo`** (words used by `to`) is refused with an
   error that says so, not the current misleading one.
6. Update `INSTRUCTIONS` (the MCP `initialize` text) with the delete form,
   the `expect` shape and `describe_op {kind}`.
Tests (`tests/args.rs`, drive `Session` as `tests/session.rs` does): each
unknown-arg case errors and names the keys; the alias works; `verify
{checks}` adds a line `clicks` to the report; the identical-variations
warning appears for a noise-free sound and not for a seeded one; the error
for `delete:` includes the working form.

### F2: macros

Owns `crates/sfx_graph/src/macros.rs`, `doc.rs`, `params.rs`,
`crates/sfx_analysis/tests/macros.rs` (new).

1. **`claim` is optional.** A macro without it is not swept by `verify`'s
   `macros` check (the report says "N macros unclaimed"). The deserialize
   error today names no macro; any macro error names the macro.
2. **A macro map entry may target a doc param**: `{"param": "size", "from":
   0.2, "to": 0.8, "curve": "exp"}` writes the sound's param `size`. A
   `voices` patch and its `map` already see the sound's params, so a knob
   can now drive the inside of a patch (9 of 24 sounds hit this). A param
   written by a macro is overridden the same way mapped node fields are
   (the "value is ignored" warning applies). Two macros may not write the
   same param.
3. **`delete_macros` plus `macros` naming the same macro in one
   `sound_ops` call replaces it** (today the new one is silently dropped).
4. A macro whose sweep fails for a reason in the render (e.g. "3251 events
   but max is 2500" in a `poisson`) reports the position and the render
   error in the same call that creates the macro, not only later in
   `verify`.
Tests: a macro driving `$size` inside a `voices` map changes the rendered
centroid monotonically over 0..1; no claim -> no sweep, no failure; replace
in one call keeps the new definition; a claimless macro with bad JSON names
the macro.

### F3: output stage

Owns the `Output` struct in `crates/sfx_graph/src/nodes/offline.rs` and the
warning text in `doc.rs` that reports a missed loudness target (coordinate
with F2: F3 only edits that function), plus new test file
`crates/sfx_analysis/tests/output.rs`.

1. **`peak_db` target** on `output`: normalizes the pre-limiter peak to the
   value instead of loudness (clicks, ticks, short peaky hits: 10 of 24
   sounds could not reach their `lufs` target). `lufs` and `peak_db`
   together is an error saying to pick one.
2. **The missed-target warning tells the truth**: it states the pre-limiter
   peak, the post-limiter peak, the crest factor, how many dB the limiter
   removed, and either "the limiter took N dB" or "the sound is too short or
   too sparse to reach this loudness: use peak_db". It must not blame the
   ceiling when the peak is more than 3 dB under it.
3. **Normalization up works**: a sound at -70 LUFS with `lufs: -20`
   reaches -20 within 1 LU if the peak allows (round 0: the thunder case
   stayed at -70).
4. **`fade_in`**: if the first onset of the render (first sample above -60
   dBFS) is within `fade_in` of 0, the summary warns once ("the default 2 ms
   fade-in is cutting the first hit by N dB: set fade_in 0 or start the
   hit later"). The default value is unchanged.
Tests: `peak_db -3` on an impulse train reaches -3 within 0.1 dB; the
warning text for a case with a 20 dB peak margin does not contain
"ceiling"; a quiet noise burst normalizes up to a stated LUFS within 1 LU;
a click at t=0 triggers the fade-in warning, a click at 5 ms does not.

### S8b: descriptor gaps

Owns `crates/sfx_analysis/src/descriptors.rs` (+ `tests/descriptors.rs`
new). All additions are extra lines or columns in `Descriptors` and
`table()`; existing lines keep their text and numbers (tests that read
them must pass untouched).

1. **Whole-sound centroid and rolloff** (energy-weighted) next to the
   start / middle / end values (ask: footstep, sword, 8 sounds).
2. **Centroid track** with 10 segments instead of 3 (sword, splash,
   wind): one extra line "centroid by tenth: ...".
3. **Per-onset table** (up to 24 rows): time, strength, attack ms, decay-20
   ms, centroid, YIN pitch if voiced, start and end pitch (for chirps,
   drips). Replaces the guesswork in knock, drip, pipe, glass.
4. **Onset density**: onsets per second at a fine hop (5 ms) with the
   minimum spacing used, separate from the existing onset list (which
   merges ticks closer than about 40 ms): crackle, rain, paper, stream,
   gravel (6 sounds). State both numbers and the spacing.
5. **Band shares**: percent of energy below 200 Hz, 200-2k, 2k-8k, above
   8k (punch, thunder, door).
6. **Pitch contour**: YIN Hz per tenth of the sound when voiced (creak,
   drip, squeak), with "inharmonic: partials at ..." when harmonicity is
   under 0.5 (glass, keys, pipe: "listen reports 919 Hz for inharmonic
   partials").
7. **Attack** gets two values: time to 90 % of the peak and time from onset
   to peak; the line says which is which (door slam, punch).
8. **Flatness in the occupied band**: flatness computed between the 10th
   and 90th percentile of the energy-weighted spectrum, in addition to the
   existing full-band one (rain, gravel, cloth, wind).
9. **Loops**: a steadiness line (loudness spread in LU over 400 ms
   windows, min / max), and the attack, peak-at and decay lines are
   replaced by "loop: n/a" (stream, rain, fire).
10. **`silence start / end`** is renamed and explained in the same line
    ("trailing silence from 0.173 s" or "none"); today `0.000 s / end
    0.046 s` confuses (punch, slam, clink).
Tests: synthetic signals with known answers for each item (a two-click
signal gives two rows at the right times; a 1 kHz sine's band shares; a
chirp's start/end pitch within 3 %; a crackle train at 200/s gives density
within 10 %; white noise full-band flatness > 0.5 and a band-limited
noise's occupied-band flatness > 0.5 while its full-band one < 0.2).

### M7 addendum: modal bugs from round 0

`modal` size/pitch (reported by footstep_wood_floor, keys_jingle, knock,
pipe): the manual says pitch goes as 1/size^2 and wood size 0.5 gives
about 425 Hz, but size 0.8 gave a lowest partial of 408 Hz and 1.4 gave
537 Hz. Find out why (print the mode table for sizes 0.25 .. 4 for each
material), fix it so the lowest mode falls monotonically with size down to
at least 40 Hz for wood, document the real range per material in the error
hint ("for metal try 0.1 to 0.3 m" is currently wrong), and normalize the
node's output level (it came out +22 dBFS peak for a unit impulse: make a
unit impulse input give a peak within 6 dB of 0 dBFS, and say so in the
blurb). Tests: lowest mode vs size is monotonic for every material; the
hint's range is derived from the same table.

## 5. Wave 2: models (8 agents)

Each is a finished-recipe node like `modal` / `thunder`: a few physical or
perceptual fields in, tuned DSP inside, in its own file. No file overlap with
wave 1, so waves 1 and 2 can run together if the orchestrator can review 20
branches; the split is for review load. Each needs wave 0. Use S8's checks in
tests only if S8 is merged; otherwise use `tests/common`.

Every model card adds, beyond rule 10: **each named field moves its measure
the stated way at 3 settings** (the same idea as macro claims), and an
`examples/<name>.rs` that renders a WAV and a PNG and prints descriptors.

| Id | File | Nodes | Structure | Field -> measure that must move |
|---|---|---|---|---|
| M1 | `nodes/blast.rs` | `explosion {size, distance, [debris], [seed]}`, `gunshot {caliber, [barrel], [suppressed], [distance], [seed]}` | Friedlander pressure wave; sub thump with falling pitch; shaped noise body; for supersonic rounds an N-wave crack ahead of the blast by the geometry delay; tail = dense decaying noise. `debris` is an `events` port if ports allow, else a count of built-in ticks. | `size` up: centroid down, decay up, sub-band (under 80 Hz) level up. `distance` up: onset later by d / 343, centroid down. `suppressed`: peak down 15 dB or more, crest down. Attack under 2 ms. |
| M2 | `nodes/machines.rs` | `engine {rpm (signal), [cylinders 4], [load 0.5] (signal), [stroke 4], [exhaust_length], [seed]}`, `motor {rpm (signal), [poles], [gear_teeth], [load], [whine]}` | Engine: per-cylinder combustion pulses on the firing order with seeded cycle-to-cycle jitter, into an exhaust delay-line resonator and an intake noise path. Motor: harmonic orders of rpm, PWM carrier with sidebands, gear-mesh tone at `rpm / 60 * teeth`, brush noise. | Pitch = `rpm / 60 * cylinders / (stroke / 2)` within 2 %; a ramped rpm gives a rising pitch track. `load` up: level and centroid up. Motor: peak at the mesh frequency. |
| M3 | `nodes/elements.rs` | `fire {size, [intensity] (signal), [crackle], [seed]}`, `wind {speed (signal), [gust], [whistle], [seed]}`, `spark {rate, [buzz 50\|60], [body], [seed]}` | Fire: crackle impulses (power-law sizes) + hp hiss + lp roar with slow turbulent AM, shared control. Wind: noise through 3 to 5 resonant bands whose centres and gains follow `speed` and an internal fbm gust; `whistle` adds an Aeolian tone at 0.2 * speed / diameter. Spark: sparse re-striking impulses gated by a mains-rate envelope into a short resonator. | `size` up: low-band share up. `intensity` up: lufs up, onset rate up. `speed` up: centroid up, lufs up. `gust` up: `swing` up. Spark: `modulation` at the mains rate (or its envelope spectrum peak). |
| M4 | `nodes/water.rs` | `water {flow (signal), [depth], [splash], [seed]}`, `splash {size, [at], [seed]}` | A bubble population: rate and radius distribution from `flow` (log-normal radii; reuse the Minnaert / damping maths of `bubble`, summed as decaying sinusoids in one pass, no per-voice graph) plus broadband hiss. Cap bubbles per second; error over it. | `flow` up: onset density up, lufs up, centroid down (bigger bubbles). 30 s at max flow renders under a stated time budget. |
| M5 | `nodes/waveguide.rs` | `tube {length, [excite reed\|lip\|jet\|noise], pressure (signal), [bore], [open_end]}`, `string {freq (signal), [excite pluck\|bow\|strike], [position], [damping], [stiffness], [bow_pressure] (signal)}` | Bidirectional delay lines, reflection filters, nonlinear excitation tables (reed, bow friction curve), allpass dispersion for stiffness. | Pitch = c / (2L) open-open or c / (4L) closed within 3 %; `pressure` below threshold is near silence, above it a steady tone; `stiffness` up: `inharmonicity` up; `damping` up: decay down. Finite at every extreme. |
| M6 | `nodes/surface.rs` | `rolling {speed (signal), [roughness], [size], [seed]}`, `cloth {motion (signal), [weight], [seed]}`, `footstep {surface, [weight], [pace walk\|run\|sneak], [shoe], [at], [seed]}` | Rolling: a seeded low-passed surface profile read at `speed`, band set by `size`, as an excitation for `modal` (output is the excite signal; doc the recipe). Cloth: power-law noise bursts whose rate and band follow `motion`. Footstep: heel and toe `contact` pulses (timing from `pace`) into a surface model: hard = short resonant, loose = granular ticks. | `speed` up: centroid up, level up; speed 0 is silence. `weight` up: centroid down. `surface` gravel vs wood: flatness higher, onset count higher. Two seeds differ, STFT distance under a stated bound. |
| M7 | `nodes/modal.rs` (also the round-0 modal bugs, section 4b), new `nodes/fracture.rs` | `modal.shape plate\|bar\|tube\|membrane\|string\|shell` (default plate, bit-identical to today); `shatter {pieces, [spread], [at], [seed]}` (an event generator: power-law fragment sizes as values, times clustered early) | Mode ratios: free bar 1 : 2.757 : 5.404 : 8.933; ideal membrane 1 : 1.593 : 2.136 : 2.296 : 2.653; string harmonic with optional stiffness; tube and shell from the standard tables (cite the source in the code). | First 4 extracted mode ratios match the table within 2 % for each shape. Default output hash unchanged. `shatter` values follow the power law (log-log slope within 20 %), count = `pieces`. |
| M8 | `nodes/foley.rs` (`Glottis`, `Tract` only) | `glottis` `[roughness]`, `[subharmonic]`, `[fry]`; `tract` `[nasal]`, `[frication {freq, level (signal)}]`, `[growl]` | Roughness: seeded jitter / shimmer per cycle. Subharmonic: alternate-cycle amplitude. Nasal: an added pole-zero pair near 250 Hz / 1 kHz. Frication: band-passed noise injected after the cascade. | Defaults bit-identical to today. `roughness` up: S8 `roughness` (or envelope 20 to 150 Hz energy) up. `subharmonic` > 0: a peak at f0 / 2. `frication.freq` moves the noise band centroid. |

## 6. Wave 3: spectral, granular, worked examples

Needs waves 1 and 2 merged.

| Id | Owns | Work |
|---|---|---|
| X1 | `nodes/spectral.rs` | `spectral` modes `shift`, `stretch` (inharmonic warp), `smear`, `hps` (ports `harmonic`, `percussive`); `pitch_shift.formant: true` (cepstral envelope kept). Tests: `hps` of sine + clicks puts 20 dB more of each in its port; formant mode keeps the spectral envelope peak within 5 % when shifting +7 semitones. Existing modes bit-identical. |
| X2 | new `nodes/bands.rs` | `filterbank {input, bands, [gains: [..]], [q]}`, `vocoder {carrier, modulator, [bands 16], [attack], [release]}`, `multiband {input, crossovers: [..], compress / drive per band}` with Linkwitz-Riley crossovers. Tests: `multiband` with neutral settings sums flat within 0.5 dB; `vocoder` output band levels track the modulator's within 3 dB. |
| X3 | `nodes/events.rs` (`Grains` only) | `grains` per-grain `size` and `pitch` as `{min, max}` or signals; new `cloud {source, density (signal), position (signal), size, pitch_spread, [seed]}` that schedules itself. Existing `grains` bit-identical. |
| (sounds) | | Building real sounds over MCP is no longer a wave 3 task: it is the test rounds of section 9, which start with a baseline before wave 1 and repeat after wave 3. |

## 7. Integration (orchestrator)

Per finished branch, in any order within a wave:

1. Read the worker's report; reject if the three commands' output is missing
   or a card assertion was weakened (compare the test file to the card).
2. `git rebase swarm/main` in the worktree. Conflicts should only be in the slot
   lines; keep both sides.
3. Merge into `swarm/main` (step 5), then in the main checkout, which only
   the orchestrator uses: `cargo test --workspace` and
   `cargo clippy --workspace --all-targets -- -D warnings`. Red: revert the
   merge (section 0). Hash-check tasks (S11, M7, M8, X1, X3) must show the unchanged
   default.
4. Review the diff yourself against the card (rules 1 to 9 of section 2);
   send findings back to the same agent (SendMessage keeps its context)
   rather than fixing by hand. One review round per branch.
5. Fast-forward merge into `swarm/main`. `git worktree remove` the worktree.

Per wave, after all merges:

6. Fold `docs/nodes/*.md` into `AGENTS.md` (families line, a section per
   group, the recipes), update the tool-count / check lists, and mirror the
   critical rules into `INSTRUCTIONS` in `tools.rs`. One agent, serial.
7. Add the new nodes to the hostile-input test (`tests/session.rs`) and to
   `LOOP_TYPES` only where a card added a ticker.
8. **Listening queue, not a stop.** Nothing in this plan can tell whether an
   explosion sounds like one. Do not wait for the owner: add every example
   and every round's best render to the listening queue in the final report
   (section 9.7) and go on.

Sizing: wave 0 is 3 tasks; waves 1 and 2 are 20; wave 3 is 3; each test round
is 24 to 28 testers, as many judges, and up to 12 fix tasks. The shared
`target/` holds one copy of the dependencies plus the three local crates per
live worktree; removing a worktree does not free its artifacts, so if disk
runs short, wait for running builds, then delete `target/debug/incremental`.

## 8. Worker brief template

Fill the braces; everything else is fixed. Launch with model `sonnet`, in
the background, several per message. The worktree `.swarm/wt/{id}` on branch
`swarm/{id}` exists before the launch (section 0).

```text
You are implementing task {id} of design/swarm-plan.md in the sounder repo
(a procedural sound-effects tool; nodes are Rust structs rendered offline).
Your worktree is {abs path}/.swarm/wt/{id}, branch swarm/{id}. Work only
there: start every shell command with `cd {that path} && `. Nobody will
answer questions: decide, and note the decision in your report.

Read first, in this order:
1. design/swarm-plan.md: section 2 (rules, all binding) and your card "{id}".
2. crates/sfx_graph/src/nodes/ringmod.rs, crates/sfx_analysis/tests/ringmod.rs
   and docs/nodes/ringmod.md: the pattern to copy.
3. crates/sfx_graph/src/node.rs (Cx, Eval, Describe, node_types!) and
   crates/sfx_graph/src/dsp.rs (primitives you may use, not edit).
{4. the existing file(s) you extend: ...}

Build exactly what the card lists: the fields, the tests with the stated
tolerances, the doc fragment. If a tolerance cannot be met, do not loosen
it: keep the test, mark it #[ignore] with the measured value in the reason,
and say so first in your report. If you need a change outside your files,
do not make it: report it under "needs from integrator".

You cannot hear. Do not describe how anything sounds; report measurements.

Finish by running the three commands in rule 12, committing to your branch,
and replying with the report in rule 13.
```

## 9. Test and feedback rounds

The node set is only as good as what an agent can build with it. Each round,
fresh Sonnet agents (**testers**) each build one common foley sound over MCP,
as a user of the tool would, and write down how hard it was. The orchestrator
**judges** what they made. The feedback becomes fix tasks, the fixes merge,
and the next round runs on the new build. Rounds repeat until the feedback
thins out: at least 3 rounds after the baseline, at most 5.

```text
round N:  build binary -> testers (parallel) -> judges (parallel)
          -> triage -> fix tasks (parallel worktrees) -> merge -> round N+1
```

### 9.1 Setting up a round

1. On `swarm/main`, in the main checkout: `cargo build` (the only place the
   root crate is built). Copy the result out so later builds cannot change
   it under a running tester:
   `.swarm/bin/r<N>/sounder`, plus `mcp_driver.py` and `AGENTS.md` of the
   same commit beside it.
2. Per sound, a project folder `.swarm/r<N>/<sound>/` containing an empty
   `.sounder/` (that makes it a project).
3. Per tester, its own server: `SOUNDER_BINARY=.swarm/bin/r<N>/sounder`,
   `SOUNDER_PROJECT=<its folder>`, `SOUNDER_SOCKET=/tmp/sr<N>-<k>.sock`
   (absolute paths; keep the socket path short, unix sockets cap near 100
   characters). One daemon per tester, never shared.
4. **Before round 0 only:** work out the exact driver commands once
   yourself (start a headless daemon, send a `--connect --calls` file, save
   images with `--save-dir`, stop the daemon; the driver's docstring has
   the flags) and write the working lines to `.swarm/TESTER_SETUP.md`.
   Every tester and judge brief points at that file.

Round 0 is the baseline: it runs on the code as it is at the start, while
wave 0 is being built. Round 1 runs after wave 3 merges. Each later round
runs after the previous round's fixes merge.

### 9.2 The sounds

24 core sounds, the same every round so scores can be compared, plus 4
reserve sounds new to each round from round 2 on (to show whether fixes
generalize or only fit the core set). The targets are estimates from general
acoustics, written before anything was rendered; they are frozen for the
whole run in `design/rounds/briefs.json` so a score change means the tool
changed, not the target. Write a reserve sound's brief in the same form
when it enters, and freeze it too.

| # | Sound | Kind, length | Must show |
|---|---|---|---|
| 1 | `footstep_concrete` | oneshot 0.15-0.4 s | heel then toe: 2 onsets 60-150 ms apart; attack under 10 ms; centroid 0.8-3 kHz |
| 2 | `footstep_gravel` | oneshot 0.25-0.6 s | 8 or more onsets; high flatness; centroid 2-6 kHz |
| 3 | `footstep_grass` | oneshot 0.25-0.6 s | soft attack 10-40 ms; no pitch; centroid 1.5-5 kHz |
| 4 | `footstep_wood_floor` | oneshot 0.2-0.5 s | a low body mode 80-300 Hz under a short tick |
| 5 | `door_creak_open` | oneshot 0.8-2.5 s | pitched stick-slip between 200 and 1500 Hz whose pitch moves; no impact |
| 6 | `door_slam` | oneshot 0.4-1.2 s | latch click, then a body under 150 Hz; attack under 5 ms |
| 7 | `knock_wood` | oneshot 0.7-1.2 s | 3 onsets 0.15-0.3 s apart; modes 200-1200 Hz; each under 150 ms |
| 8 | `glass_break` | oneshot 0.8-2.5 s | one hit, then 15 or more shard onsets thinning out; centroid over 4 kHz |
| 9 | `glass_clink` | oneshot 0.3-1.5 s | inharmonic modes 1.5-6 kHz; smooth decay; one onset |
| 10 | `metal_pipe_drop` | oneshot 1-3 s | 2-5 onsets with shrinking gaps; inharmonic ringing modes; decay over 0.5 s |
| 11 | `coin_drop` | oneshot 0.6-1.8 s | 4 or more bounces with shrinking gaps; modes 3-9 kHz |
| 12 | `water_drip` | oneshot 0.4-1.5 s | 1-3 drips, each 30-120 ms, pitch 600-2500 Hz rising within the drip |
| 13 | `stream_loop` | loop 4 s | `loop_seam` passes; dense bubble onsets; energy 300 Hz-6 kHz |
| 14 | `splash` | oneshot 0.6-1.8 s | broadband burst, then a bubbling tail; centroid falls over time |
| 15 | `rain_loop` | loop 4 s | `loop_seam` passes; high flatness; centroid 3-8 kHz; steady level |
| 16 | `wind_loop` | loop 6 s | `loop_seam` passes; centroid 200-1200 Hz and moving; loudness swings; no clicks |
| 17 | `fire_loop` | loop 5 s | `loop_seam` passes; 5 or more crackles a second over a roar under 300 Hz and a hiss |
| 18 | `thunder_distant` | oneshot 6-14 s | silence first; centroid under 150 Hz; long decay |
| 19 | `cloth_rustle` | oneshot 0.4-1.2 s | 2-4 soft swells; no pitch; centroid 2-7 kHz; no hard onset |
| 20 | `paper_crumple` | oneshot 0.6-1.5 s | dense crackle (20 or more onsets); centroid 3-8 kHz |
| 21 | `sword_whoosh` | oneshot 0.25-0.6 s | swell in over 50-150 ms; centroid rises then falls; no click |
| 22 | `punch_thud` | oneshot 0.15-0.4 s | attack under 5 ms; most energy under 200 Hz; a short slap at 1-4 kHz |
| 23 | `switch_click` | oneshot 0.03-0.12 s | 1-2 onsets; centroid 2-6 kHz; ends clean |
| 24 | `keys_jingle` | oneshot 0.6-1.5 s | 8 or more onsets; inharmonic modes 3-10 kHz |

Reserve, 4 per round in this order: `chain_rattle`, `zipper`,
`pencil_scribble`, `drawer_slide`; `rock_debris`, `ice_in_glass`,
`boiling_loop`, `rope_creak`; `book_drop`, `match_strike`, `typewriter_key`,
`car_door`; `arrow_twang`, `heartbeat_loop`, `bike_bell`, `squeaky_hinge`.

### 9.3 The tester

One agent per sound, fresh each round (no memory of earlier rounds: that is
the point). It is a **user**: it may read the round's `AGENTS.md`,
`.swarm/TESTER_SETUP.md` and whatever the tools answer (`describe_op`,
errors, `summary`). It may not read `crates/`, `src/`, `examples/`,
`design/` or another tester's folder, and it compiles nothing.

Budget: 60 tool calls to the server. It stops at the budget whatever the
state and reports honestly; an unfinished sound is a valid result.

It must: build the sound; get `verify` to pass or say which check it could
not pass; add 2 to 4 macros; `catalog save`; `export` with `variations: 4`;
call `session_log(feedback: ...)` once per blocker as it happens; and write
`feedback.json` in its folder:

```json
{"sound": "door_slam", "round": 2, "finished": true,
 "calls": 41, "failed_calls": 6,
 "difficulty": 3,
 "self_score": 6,
 "nodes_used": ["contact", "modal", "noise", "..."],
 "blockers": [
   {"kind": "missing_field", "where": "modal",
    "wanted": "damp the ring after 80 ms, as a door held by its frame",
    "tried": "a decay multiplied on the output; it also cuts the latch click",
    "workaround": "two modal nodes, one per layer",
    "severity": 2}],
 "wishlist": ["a latch / mechanism click model", "..."],
 "what_worked": ["contact -> modal gave the body at once"]}
```

- `difficulty`: 1 the first plan worked; 2 a few corrections; 3 a real
  detour or workaround; 4 got something only by giving up part of the
  brief; 5 could not build it.
- `kind`: `missing_node`, `missing_field`, `wrong_result` (the tool did
  something other than documented), `bug` (crash, hang, non-finite),
  `confusing_error`, `doc_gap`, `sense_gap` (could not tell from `listen` /
  `verify` whether it worked), `slow`.
- `severity`: 1 cost a few calls; 2 forced a workaround; 3 blocked part of
  the brief.
- `wishlist`: at most 3, most wanted first. `what_worked`: at most 3.

Tester brief:

```text
You are testing a sound-design tool as its user. Build one sound and report
how hard it was. Nobody will answer questions.

Sound: {name}. Brief: {the row from briefs.json, in words}.
Your project folder: {abs}/.swarm/r{N}/{name}
How to talk to the server: {abs}/.swarm/TESTER_SETUP.md (your socket is
{socket}; use exactly those commands).
The manual: {abs}/.swarm/bin/r{N}/AGENTS.md. Also ask the server:
describe_op lists every node; describe_op "<type>" gives one.

Rules: do not read any other file in the repository. Do not compile
anything. At most 60 calls to the server. You cannot hear: use summary,
listen (look at the saved PNG) and verify, and say what you measured.

Do: build it; make verify pass; add 2-4 macros whose claims pass; catalog
save; export with variations 4; call session_log(feedback: "...") each time
the tool gets in your way. Then stop the daemon and write feedback.json in
your project folder in the format below, honestly: an unfinished sound with
precise blockers is more useful than a finished one with none.
{the JSON format and the scales from section 9.3}
```

### 9.4 The judge

The orchestrator owns the scores. To keep 24 sounds in one round from
flooding its context it uses one **judge agent per sound**, fresh, which
sees the brief and the tester's project folder but **not** `feedback.json`
(delete nothing; tell it not to open that file). The judge starts its own
daemon on the folder, `catalog load`s the sound, and runs `summary`,
`verify`, `listen` with `seeds: [1, 2, 3, 4]`, and one `listen sweep` per
macro. It reads the PNGs. It writes `score.json`.

The orchestrator then judges 3 sounds of the round itself the same way (the
judges' highest, lowest, and one more) and compares. If its total differs
from a judge's by more than 2 on any of them, it tightens the wording of the
rubric line that caused the gap, re-runs all judges of that round once, and
notes it in the journal. A wording change after round 0 also means
re-judging the earlier rounds' saved sounds with the new wording (their
project folders are kept for this), so every column of the final table was
scored the same way.

Rubric, 0 / 1 / 2 each, total 0 to 10. 0 = absent or failing, 1 = partly,
2 = fully, each with the measured evidence quoted:

| Line | 2 points when |
|---|---|
| Checks | `verify` with the default checks has no FAIL (loops: `loop_seam` included) |
| Time shape | length, attack, decay and onset count / spacing are all inside the brief's ranges |
| Spectrum | centroid and the dominant octave bands at start / middle / end match the brief, including any stated movement |
| Parts | every component the brief names is separately visible in the descriptors or the spectrogram (the latch before the thump, the hit before the shards) |
| Control and variation | 2-4 macros, every claim passes, and the 4 seeds differ measurably while all staying inside the brief |

A sound that was not saved, or does not render, scores 0. The judge also
writes at most 3 `notes`: things that look wrong and are not in the rubric
(a click at the tail, a level 20 dB off, a second pitch nobody asked for).

**What the score is.** A measure of fit to a written brief by something
that cannot hear. A 10 means "plausible on every measure", not "good". The
report must say so wherever it shows scores.

### 9.5 Triage: feedback into fix tasks

After the judges, the orchestrator writes `design/rounds/r<N>/`:

- `scores.md`: one row per sound: judge total and lines, tester difficulty,
  self-score, calls, failed calls, finished.
- `friction.md`: `session_log(report: true)` from each tester's project
  (the server's own record of failed and slow calls: objective, unlike the
  self-report).
- `triage.md`: every blocker, wishlist item and judge note merged into
  **issues**. Two reports are the same issue when a single change would
  resolve both. Per issue: kind, where, the sounds that reported it, summed
  severity, and the fix type:

| Fix type | Means | Goes to |
|---|---|---|
| `doc` | the tool could do it; the manual, a `describe_op` text or an error message did not say how | one doc agent for all `doc` issues of the round (`AGENTS.md`, `INSTRUCTIONS`, blurbs, examples, error strings) |
| `bug` | wrong result, crash, non-finite, a check that lies | a fix task on the owning file, with a test that fails before the fix |
| `field` | an existing node needs a field or a mode | a fix task on that node's file |
| `node` | nothing existing does it | a new-node task in the section 4 card format |
| `sense` | the tester could not tell whether it worked | a task in `sfx_analysis` (a descriptor, a check, a clearer line) |

**Which issues get fixed this round:** every `bug`; every issue reported by
2 or more sounds; every single-sound issue of severity 3; every core sound
whose judge total fell by 2 or more since the previous round (a
**regression**: the task is to find which merge caused it). Order by summed
severity, cap at 12 tasks. Issues touching the same file are merged into
one task so no two tasks share a file. The rest go to
`design/rounds/backlog.md` with their counts; a backlog issue reported
again adds to its count and can qualify later.

Write each task as a card in `design/rounds/r<N>/fixes.md` in the section 4
form (owner files, the change, tests with numbers). Then run them exactly
as a wave: section 2 rules, section 8 brief (pointing at `fixes.md`
instead of a section 4 card), worktrees `.swarm/wt/r<N>-<k>`, integration
per section 7. A fix that changes an existing node's default output must
say so in its card; otherwise defaults stay bit-identical.

Do not fix a sound by hand, do not edit a tester's patch, and do not add a
node whose only use is one brief (a `door_slam` node): fix what made it
hard.

### 9.6 When to stop

Rounds 1, 2 and 3 always run. After round 3 and after round 4, stop if all
three hold, else run another. Round 5 is the last whatever it shows.

1. **Feedback has thinned:** 3 or fewer issues qualified for fixing this
   round (by the 9.5 rule, before the cap).
2. **Scores have flattened:** the mean judge total over the 24 core sounds
   rose by less than 0.3 since the previous round.
3. **Difficulty has flattened:** the mean tester difficulty over the core
   sounds fell by less than 0.2 since the previous round.

The fixes of the final round are still implemented and merged, and the
core sounds are then run **once more as a check** (testers and judges, no
triage) so the report's last column describes the code that is actually on
`swarm/main`.

If the mean core score falls two rounds in a row, something merged made
the tool worse: stop adding features, make the next round's tasks only
regressions and bugs, and say so in the report.

### 9.7 What is left at the end

Commit to `swarm/main` as you go: `design/rounds/JOURNAL.md`,
`briefs.json`, `backlog.md`, and each `r<N>/` folder (`scores.md`,
`friction.md`, `triage.md`, `fixes.md`, every `feedback.json` and
`score.json`). WAVs and PNGs are git-ignored and stay in `.swarm/`.

Then write `design/rounds/REPORT.md` and stop:

- A table: one row per core sound, one column per round, judge total and
  tester difficulty in each cell; means in the last row. Reserve sounds in
  a second table.
- Per round: issues found, qualified, fixed, deferred; calls and failed
  calls per sound (mean).
- Why the run stopped (which 9.6 conditions, with the numbers).
- What was built: nodes and fields by wave and by round, each with its
  measured test numbers.
- What was not: deferred tasks, `#[ignore]`d tests with their measured
  values, the backlog by count.
- **The listening queue** for the owner: for each sound the path of the
  best-scored round's four exported WAVs, and the same sound from round 0
  beside it where round 0 produced one; then every `examples/*.rs`
  render. Ordered so the lowest-scored come first. With one line on top:
  none of this has been heard.
- The exact commands to review and merge: `git log main..swarm/main
  --oneline`, `git diff main...swarm/main --stat`.
