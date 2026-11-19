# Status

As of 2026-11-18, branch `swarm/main`. This is the current state of the
project; how it got here is in `docs/history/`.

## The goal

Describe a sound in words and have an agent build it from known base
components. The agent cannot hear, so everything that says a sound is *good*
comes from the human's blind ratings.

## What works

- **The tool:** 122 node types, the MCP server, the window, the senses
  (`summary`, `listen`, `verify`), macros, export. 859 tests pass; clippy and
  fmt are clean.
- **The parts library:** 89 built-in parts in `parts/`, every one rated good
  by ear in a blind listen and named for what the listener heard.
  `catalog search` finds them by text, role, material and approval; the
  `sound` node assembles them. Only the human can mark a part approved (a
  toggle in the window's sound list).

  | Role | Parts | | Material | Parts |
  |---|---|---|---|---|
  | whole | 37 | | any | 24 |
  | body | 16 | | metal | 10 |
  | after | 11 | | wood | 9 |
  | | | | plastic | 8 |
  | | | | water, air, glass | 5 each |
  | | | | stone | 6 |
  | texture | 7 | | synthetic | 4 |
  | bed, pre, crack | 5 each | | ceramic, mechanical | 3 each |
  | weight | 3 | | fire, voice, cloth | 2 each |
  | | | | electric | 1 |

- **The repo:** one test binary per topic (`crates/sfx_analysis/tests/it/`),
  node docs named after node families (`docs/nodes/`), `AGENTS.md` at about
  75 KB with the reference split into `docs/`. Rules for keeping it that way
  are in `AGENTS.md`, "Changing the tool: where things go".

## What the library does not have

Every attempt at these was heard as something else, rated lower, or not
rated. An agent that builds one must say it is unheard.

- A footstep (the three surfaces were heard as snaps, a plastic bag, pitter
  patter), rain, a stream (still "oil bubbling").
- A large splash: 0.6 m was rated bad with no note on why; 5 to 20 cm is good.
- An electric motor ("digital engine simulation"), a creature voice ("low
  grumble", OK only).
- Rated OK and left out: pressure release, engine idle, granular shimmer,
  power-up, tape drone, a fast whoosh, a finger snap, one stone hit.

## What was learned

- **Agents' own judges cannot rank sounds.** They scored every test round 9.1
  to 9.5 of 10 while the listener found whole families unrecognisable.
- **An audio-text model is a hint, not a check.** On 40 labelled cues
  `laion/larger_clap_general` named the intended family first on 18 and
  separated good from bad cues with AUC 0.80 (11 good, 13 bad); two smaller
  models were at or below chance. It still passed a crumple the listener
  called broken. Scripts and audio: `~/.local/share/sounder-hearing/`
  (outside the repo).
- **What was built is often not what is heard.** About half the good-rated
  hits were labelled as another material, so parts carry the listener's label.
- **Ready-made models read better than assembled hits.** 18 of 28 good cues
  from the model nodes (alarm, bird, cricket, creak, engine, flute, siren,
  sparks, bells, wind) were heard as intended.
- **Metal works by synthesis: long-ringing noise-band modes.** In batch 4
  (`docs/history/labels/batch4_findings.md`) all five metal objects built as
  `modal` metal with `noise` 0.4, `noise_width` 0.1 and `loss` 0.3-0.6 were
  heard as metal; the same objects as clean sine modes with beating pairs
  were "digital bell". Earlier tries had rung only 0.1-0.4 s and read as
  glass or ceramic.
- **Stone works too: weight and grit, with a quiet body.** In batch 5
  (`docs/history/labels/batch5_findings.md`) a brick and a fist-sized rock
  built from brown-noise weight, crackle grit, band-passed noise and a
  stone `modal` 6-8 dB under it were heard as "brick dropped or medium rock
  hit". More ring turned stone into a woodblock.
- **The splash was reworked by ear** over four listens: the noise opening
  ("a snare, then bubbles") is replaced by cavity bubbles and a pour, with
  short droplets falling back. Long sine droplets read as "digital tones".

## Next, in order

1. **The brief as data:** done (2026-11-18). `sound_ops` `spec` lists the
   components (role, node, material, slot, length, band); `summary` shows what is
   missing and `verify` checks each one by default. The door slam template
   carries one. Components take a `length_ms` too. First use: a wooden crate
   dropped on concrete, built from a spec, rated "sounds great" (now the part
   `wooden_crate_drop`).
2. **A `heard_as` line in `listen`:** done (2026-11-18), a hint only. The
   worker is `tools/hearing/heard_as.py`; it needs the venv in
   `~/.local/share/sounder-hearing/` (or `SOUNDER_HEARING_PYTHON`). First
   reading: the approved door slam's brief ranks 1 of 102, the crate drop
   rated great ranks 83 (heard as "a snare drum").
3. **Metal and stone by synthesis:** done (2026-11-18). Five metal parts
   heard as metal, two stone parts heard as a brick or a rock, and a heavy
   slab heard as a wall (fine for a very large stone, the listener said).
4. **An evaluation set:** done (2026-11-18), `tools/eval/`. Ten fixed briefs,
   each built by a fresh agent from AGENTS.md alone and rated blind against
   the brief. Baseline: 7 good, 2 ok, 1 bad (16 of 20): the pipe read as a
   sheet, the footstep's heel and toe too far apart, the whoosh as an
   artificial sweep. All three are fixed in the node or the docs; the next
   run measures the fixes.
5. Fill the remaining library gaps above as they come up.

## Local state, not in git

`.sounder/` (sound library and sessions), `assets/sources/pd/` (16 renders of
Farnell's patches by real Pd: the ground truth for the ports), `refs/`
(reference recordings), `target/`.
