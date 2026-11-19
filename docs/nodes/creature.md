# `creature`: an animal's call

After the foley project (`foley-synth/src/voice/creature.rs`, `score.rs`,
`tract.rs`, and the ravager / polar bear roar and the wolf growl and whine
in `mod.rs` / `mobs2.rs`). Code: `crates/sfx_graph/src/nodes/creature.rs`;
tests: `crates/sfx_analysis/tests/it/creature.rs`.

The sound of one made-up animal making one call: an LF glottal source
(Rd 0.3 pressed .. 2.7 breathy) with subharmonics, sidebands, chaos (jitter,
shimmer, pitch jumps), creak and ingressive voicing, through a Kelly-Lochbaum
waveguide tract with a nasal branch. A bigger body means a longer tract
(lower formants, dF = c / 2L) and a lower pitch. `glottis` + `tract` in
`voice.md` are a formant cascade; this is the waveguide itself.

| Field | Meaning |
|---|---|
| `call` | `roar`, `growl` (default), `snarl`, `whine` |
| `size` | 0..1 (default 0.7). Tract 4 cm * 20^size (4 cm .. 80 cm), pitch 1500 Hz * 0.04^size (1.5 kHz .. 60 Hz) |
| `aggression` | 0..1 (default 0.6). Pressed rather than breathy voicing, stronger subharmonics |
| `roughness` | 0..1 (default 0.6). Chaos, subharmonics, sidebands, creak |
| `length` | seconds, 0.1..10. Unset: the call's own (roar 1.0, growl 1.2, snarl 1.3, whine 0.8) |
| `at`, `seed` | start time; seed |

Calls:

- `roar`: one bellow, vowel uh -> ah -> oh, pitch up 30 % then down, creak at
  the ends, 40 Hz sidebands (the ravager / polar bear warning).
- `growl`: foley's `creature` growl: uh -> ah at 0.7-0.8 of the pitch, `rough`
  sets the sideband rate (30-90 Hz) and depth.
- `snarl`: the wolf's growl: a pressed rumble (a third of the pitch, lips half
  shut) then the same drawn in on the in-breath.
- `whine`: the wolf's whine: "oo" through the nose (velum open), twice the
  pitch, rising, with a 6 Hz tremble.

Measured (48 kHz, seed 0, 3 s document):

- Centroid, size 0.1 / 0.9: roar 2650 / 216 Hz, growl 2650 / 207 Hz, snarl
  958 / 153 Hz, whine 3781 / 260 Hz. Roar at size 0.9: 55 % of the energy
  under 200 Hz, 45 % at 200 Hz-2 kHz, under 0.3 % above.
- Roar roughness (20-150 Hz envelope energy): 2.5 % at `roughness` 0, 13 % at 1.
- `length` 0.5 s / 3 s gives audible 0.5 / 3.0 s for every call.
- Peak is normalized (roar 0.7, growl 0.6, snarl 0.7, whine 0.5), so set the
  level with `output.lufs` or `peak_db`.

Limits and differences from foley: the tract is at most 180 sections (about
0.64 m at 48 kHz), so `size` above about 0.95 stops lengthening it; section
length follows the document's sample rate (foley is fixed at 48 kHz); frication
and biphonation are not ported (no call here uses them). The calls have not
been heard; the existing `roar` (Farnell) was heard as a cow and a groan, and
this is meant to replace it.
