# cloth_fix: a swish, not ticks

Batch 2 blind listen (design/labels/batch2_findings.md): `cloth` 3/3 heard as
"pellets of something hard flying out of a hopper", "metal objects falling".
Measured cause: each burst was an instant-attack, 3 ms x sqrt(size) decay of
noise through a narrow band-pass (q 1.1): a train of clicky, faintly pitched
ticks.

Change (crates/sfx_graph/src/nodes/surface.rs, no new fields):
- each burst is a difference of exponentials: decay 25 ms x sqrt(size) x
  (1..2.2 by weight), capped at 0.25 s; attack 0.35 x decay, at least 6 ms;
- burst level ~ size^0.35 (was size^0.6), so the power-law large bursts no
  longer stand out as hits;
- a continuous rubbing bed in all four bands, level ~ motion^0.75 with the
  motion smoothed over 40 ms; band centres follow that smoothed motion;
- bands are wider (q 0.6). Motion 0 is still exact silence.

Measured, 2 s, seeds 1-3 (tests/cloth_fix.rs):

| | before (motion 0.5, weight 0) | after (all motion 0.5..2, weight 0..1) |
|---|---|---|
| crest | 25.5-27.0 dB | 13.1-16.5 dB |
| fine onsets (5 ms) | 28.5-31 /s | 0-0.5 /s |
| occupied-band flatness | 0.68-0.80 | 0.77-0.92 |

Motion still raises level and centroid (motion 0.5 / 1 / 4 at weight 0.5:
-42.2 / -33.5 / -17.0 dB rms, 4271 / 4763 / 6568 Hz); weight lowers the
centroid (5960 / 4763 / 3738 Hz at 0 / 0.5 / 1). The `density` claim no
longer fits a cloth knob: the 1 ms counter reads noise grain (375-674 /s).
Not heard: whether it reads as fabric is the listener's call.
