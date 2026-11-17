# Phase 0 results: door slam A / B / C against real recordings

Run 2026-11-17. Plan: [sfx-quality.md](sfx-quality.md). The listening test is
still to do.

## What was built (all in the catalog, all at -28 LUFS)

| Sound | What it is |
|---|---|
| `slam_a` | The round-0 agent door slam (`.swarm/r0/door_slam`), rebuilt node for node: click resonators, low body resonators, brown-noise thump. |
| `slam_b` | Synthetic only, layered: band-passed noise crack, wood `modal` panel + bar frame, sub drop 75 to 38 Hz, `click` latch, `crumple` rattle; bus chain `transient`, `shape` tube (4x oversampled), hp, `eq`; `reverb` room on a 150 Hz high-passed send, stereo. |
| `slam_c` | The same chain and room, with the panel / frame / crack replaced by a real recording (Freesound 571797, rate 0.88, i.e. about 2 semitones down) plus the synthetic sub and latch. |
| `slam_real` | Control: the recording 571797 untouched. |

Macros: B has `weight` (sub level), `room`, `crack`; C has `weight`, `room`,
`pitch`. All unclaimed. A keeps its round-0 macros.

## References

CC0 recordings from Freesound, in `refs/freesound/` (gitignored, not shipped):

- 406635 "Bathroom door slam shut_more air" (tlcolbe), 48 kHz / 24-bit, scoring
- 724124 "Door slam2" (saha213131), 48 kHz / 16-bit, scoring
- 571797 "Door slamming hard" (NachtmahrTV), 44.1 kHz / 24-bit, the source for C (so not scored against)
- 462665 "Angry Door ... Slam Hard A" (hisoul), 192 kHz: not used. Its slam sits at about -1 dBFS for 0.8 s (clipped), and its first event is two hits.

## Method

`listen reference` / `verify reference` distance: a multi-resolution STFT
distance in dB after aligning onsets and matching loudness. Lower is closer.
Leave-one-out: C is built from 571797, so every sound is scored on the other
two references only.

B and C were tuned by a random-then-local search (180 renders each) over their
main levers (layer levels, decays, crack band, panel size and loss, room size,
decay and level, punch), minimising the mean distance to the two scoring
references. Script and best settings: `docs/plans/phase0/`.

## Numbers

| Sound | vs 406635 | vs 724124 | mean |
|---|---|---|---|
| `slam_a` (as round 0 built it) | 15.95 | 13.28 | **14.6** |
| `slam_b` first pass, by hand | 12.06 | 10.32 | 11.2 |
| `slam_b` after search | 11.65 | 8.50 | **10.1** |
| `slam_c` before search | 11.40 | 8.44 | 9.9 |
| `slam_c` after search | 10.45 | 8.53 | **9.5** |
| `slam_real` (a real slam, untouched) | 12.85 | 7.94 | **10.4** |

What the measurements say:

1. **A to B is the large step: 14.6 to 10.1 dB.** It comes from layering, a
   longer ring, a room and the bus chain, all made from existing nodes. A's
   biggest misses were 177 Hz to 1.4 kHz, 42 to 59 dB too quiet in the middle
   of the sound: it had no body or room between the click and the sub.
2. **B to C is small: 10.1 to 9.5 dB.** It is also within the spread between
   real recordings.
3. **The metric saturates.** A real slam scores 10.4 dB against the two other
   real slams, worse than the tuned synthetic B (10.1). Below about 10 dB this
   distance cannot tell a better door slam from a different door. It works as a
   coarse gate ("is this in the family") and not as a quality judge. That
   points Phase 5 at something other than nearest-reference STFT distance
   (per-feature targets from a reference set: ring time, band balance over
   time, crest; or a learned or listening judge).
4. Per the plan's rule ("if a->b is large, Phases 1-3 are the priority"), the
   numbers point to **Phases 1-3** (layering guidance, the `finish` node, the
   design checks). The listening test decides whether that holds.

## Listening test (for the human)

Open the four sounds in the window (`catalog load`), all at -28 LUFS, and play
them in a shuffled order: `slam_a`, `slam_b`, `slam_c`, `slam_real`.

Things to note per sound: is it a door; size and weight; does the attack crack
or click; does the tail sound like a room or like ringing; anything that marks
it as synthetic. Then: is A to B a bigger step than B to C?

## Listening test result (the human, 2026-11-17)

- **A to B is the bigger step, clearly.** So Phases 1-3 come first.
- B is still "a bit too melodic, like a tuned drum". The real door is atonal,
  weightier and snappier. Measured: B's pitch tracker reads 104 Hz with 0.9 of
  the sound voiced (the sine sub drop and a few clean `modal` modes); the real
  slam reads 0.15 voiced, with dense inharmonic partials from 180 Hz to 1.9 kHz,
  weight at 125-500 Hz (its 63 Hz band is at -44 dB) and a 16 dB crest
  (B: 28 dB).
- B "has a reverb tail; SFX should usually be dry".
- The real recording has a subtle friction (the door sticking on the frame)
  before the hit. Measured: a bright 10-40 ms brush at about 2.5 kHz centroid
  before the main impact at 53 ms.
- B's second hit (the `click` latch plus a hard `crumple` rattle) reads as "a
  chime, or little rusted pieces of metal clinking", not wood on wood.

None of these showed up in the STFT distance, which is the case for Phase 3's
design checks: tonality (voiced share on an impact), dryness (tail energy after
the source stops), and a crest target.

`slam_b2` answers the notes: dry (no reverb, mono), atonal weight (noise
band-passed at 220 Hz plus low-passed brown noise, no sine), a wood `modal`
excited by a noise burst with high loss, a friction + noise brush 8-50 ms before
the hit, and a wood-on-wood knock (`contact` -> small `modal` wood, low-passed)
instead of the metal latch and rattle.

B2 was tuned by the same search (mode `b2` in `docs/plans/phase0/search_slam.py`,
on all three clean references since it uses no recording). The search pushed
the friction brush down to -17.6 dB, because the distance does not reward it, so
it was held at -10 dB for the listening test (a cost of 0.25 dB in distance).

| Measure | `slam_b` | `slam_b2` | `slam_real` |
|---|---|---|---|
| distance, mean of 406635 + 724124 | 10.1 | 9.2 | 10.4 |
| voiced share (pitch tracker) | 0.9 | 0 | 0.15 |
| crest | 28 dB | 21.6 dB | 16 dB |
| occupied band (middle 80 % of energy) | 70-470 Hz | 117 Hz-1.1 kHz | 117 Hz-1.2 kHz |
| energy <200 / 200-2k / 2-8k | 41 / 58 / 0.6 % | 29 / 67 / 4 % | 38 / 56 / 5.5 % |
| -20 dB decay | 120 ms | 130 ms | 210 ms |
| reverb / stereo | room, stereo | none, mono | none, mono |

Macros on `slam_b2`: `stick` (brush level), `weight` (low thump), `snap` (crack),
`knock` (second hit). B2 still rings shorter than the real slam (130 vs 210 ms
to -20 dB); `weight` and a longer body decay are the levers if it reads thin.

**Verdict (the human, 2026-11-17): B2 is "basically perfect".** The recipe that
got there, which is what Phases 1-3 should teach and check: layered roles
(pre-impact brush, crack, noise-excited wood body, band-limited noise weight,
a small wood knock), a dry mono bus (transient, light tube saturation, low-mid
lift), then tuning the layer levels against real references by search, with the
listener's notes as constraints the search may not undo.

## Issues found along the way

- `output.lufs` -28 on `slam_c` delivered -26.3 LUFS with no limiting (peak
  -8.6 dBFS) and warned "too short or too sparse". `slam_real` (the same
  recording, untreated) hit -28 exactly. Worked around with `gain_db` -1.7,
  which then gave -28.00. Looks like a normalization bug; it needs a repro.
- Freesound originals need a logged-in browser; the in-app browser pane
  downloads them with a click from the human on each save popup. Requests sent
  back to back can lose a popup: send them one at a time.
- The round-0 door slam's own score file says its click and body start
  together (one onset). The reference comparison adds that it has nothing
  between 177 Hz and 1.4 kHz after the first 100 ms.
