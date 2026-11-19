# Batch 24: `impact` sliding (2026-11-19)

Five boxes, two designs each, dry and dressed, blind. Key:
`tools/labels/batch24.json`, generator `tools/labels/gen24.py`.

| Box | Picked | Note |
|---|---|---|
| a metal bar bowed | hard pressure (drive 1 -> 8 N), dry | |
| a metal box scraped across stone | a pull slowing 0.6 -> 0.05 m/s, dressed | over a jerky pull |
| a bright steel sheet, bending as it rings | the wobble (pitch 0.9..1.1), dry | the bend is heard now that the noise bands bend and the sheet is bright |
| a wooden door creaking | none | "the other model sounded better for this. this sounds like when you play a glass with a wet finger" |
| a marble rolling on a board | none | the fine roll better, "but still hearing random crackle noises" |

- Friction works for a bow and a scrape. A creak is not a steady stick-slip
  on the door's modes: route creaks to `door_creak` (Farnell's), not `impact`.
- The roll stayed in contact the whole time (measured 100 %), so the crackle
  was the surface: one scale of bumps passing at about 150 a second. The
  profile is now six octaves (`octaves`, default 6), each half the spacing
  and 0.2 the height (0.5 and 0.3 made the fine bumps' curvature grow and the
  ball hop, contact 38-39 %). The crest of the fine roll fell from 32 to
  24 dB. Batch 25 asks again, with the batch 24 roll as a reference.
- Saved as parts: `impact_bowed_bar`, `impact_box_scrape` (`octaves` 1, the
  surface it was heard on), `impact_bending_sheet`.
