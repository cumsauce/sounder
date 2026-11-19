# frog

`{"type": "frog", "size", "rate", "chorus", "seed"}`.
Code: `crates/sfx_graph/src/nodes/frog.rs`; tests:
`crates/sfx_analysis/tests/it/frog.rs`.

Frogs calling, after `crates/foley-synth/src/life.rs` of the foley project
(github.com/cumsauce/foley, MIT / Apache-2.0). One call is foley's `frog`
(the crate sounder already links), unchanged: 60-150 glottal pulses a second
(a trill of 180-260 in a frog under 8 cm, 35 % of the time) drive two
band-passed resonances at the vocal sac's frequency
`2500 * 3 / size_cm` Hz and 2.3 times it, in one to three croaking notes of
0.1-0.5 s (or one trill of 0.6-1.4 s), the sac rising 6 % through each note,
with a breath of air back into the lungs between notes. The node adds:

- `rate` (calls a second per frog, default 0.5): each call follows the last
  by 1/rate s, a tenth either way.
- `chorus` (frogs, default 1): the first is `size` exactly, the others
  0.8-1.25 times it, each with its own pace (0.8-1.2 times `rate`), start
  and distance (0.35-1), summed at 1/sqrt(n).
- every call is levelled to a peak of 0.4 before the sum (foley's call is not
  normalized; its level follows the sac's resonance). Set `lufs` or
  `peak_db` on the output.

Measured (whole-sound centroid, 6 s): 3 cm 1 288 Hz, 15 cm 512 Hz (the sac
itself rings near 2.5 kHz and 500 Hz; the centroid reads lower because the
pulses are broad and the breath sits at half the sac). Share of 100 ms windows
holding sound in 20 s: rate 0.1 0.12, rate 2 0.98; one frog at 0.5 0.49, eight
0.99.

Birds: foley's `birds.rs` and `birds/` (57 species) are what `syrinx`
already wraps (`species`, snake_case real common names, `rate`, `seed`), so
there is no `birdsong` node: use `syrinx`.
