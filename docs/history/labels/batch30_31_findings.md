# Batches 30 and 31: five new sounds built with `rig` and `impact` (2026-11-20)

Batch 30 (`tools/labels/batch30.json`): a chair knocked over, keys dropped,
a hand bell shaken, a wind chime, a marble in a bowl, each dry and dressed.

- The wind chime was heard as great: saved as `rig_wind_chime`.
- The chair sounded like a small wooden object; the bell was too noisy and
  not like a hand bell.
- Bugs found while building: `sound_ops` reset a fixed `duration` on edits
  that left it out (clients fill `null`); `force` kept reading the weight
  at rest; a light ball lost its bounce to the object's modes. All fixed.

Batch 31 (`tools/labels/batch31.json`): the reworked chair (three bodies at
chair scale landing in turn, crack and thud gated by `force`) and bell (no
noise, a 0.8 N push so the clapper flies and strikes instead of pressing)
were both picked over the first versions, labelled, on a remote device.
Saved as `rig_chair_fall` and `rig_hand_bell`.
