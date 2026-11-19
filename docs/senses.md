# Senses in full: references and extract_modes

Moved out of AGENTS.md to keep it short; AGENTS.md "Senses" has the summary.

## listen reference and reference sets

- `listen reference: "file.wav"` (relative to the project or a sources folder, or absolute): the
  reference is resampled to the sound's rate, moved so its first onset (30 dB
  under its peak) meets the sound's, and scaled to the sound's integrated
  loudness (the shift and the gain are reported). The image is [sound,
  reference, difference]; the text gives the multi-resolution STFT distance
  (windows 256 / 1024 / 4096, dB, 0 = identical), the 5 biggest band x time
  cells that differ by 3 dB or more in words ("0-20% of the sound: 2.8-5.7 kHz
  is 31 dB quieter than the reference"; segments are fifths of the sound), and
  length / attack / decay / centroid differences. A reference with nothing
  above its Nyquist counts the sound's top end as a difference (it says so).
  Not with seeds, sweep or diff. This is the distance, not a taste verdict.
  Reading the picture: x is time, y log frequency, colour dB from -90 (black)
  to 0.
- `listen reference_set: "door_slam"`: a **reference set** is a folder of
  recordings of one kind of sound, `<project>/refs/<set>/` (at least two
  WAVs; a path with a `/` names any folder). One recording is a poor target:
  in Phase 0 a real door slam sat 10.4 dB from two other real slams, further
  than a good synthetic one, so a distance below about 10 dB ranks nothing.
  A set gives the distance to each recording, the nearest against the set's
  own spread (the mean distance between its recordings), and eight features
  against the span the recordings cover: `weight` / `body` / `crack` energy
  (under 200 Hz / 200 Hz-2 kHz / above 2 kHz), `crest`, `tail` (to -20 dB),
  `centroid`, `voiced`, `ringing` partials. A feature passes inside [min,
  max] widened by a quarter of the span, at least a floor (3 % for shares,
  1 % crack, 3 dB crest: the crest of a noise hit moves several dB with the
  seed, 20 ms tail, 10 % centroid, 0.05 voiced). The image compares with the
  nearest. `verify expect reference_set {set, [max_distance]}` reports each
  feature (`reference_set weight` ...) and the nearest. Measured against
  three CC0 door slams (406635, 571797, 724124; they sit 10.06 dB from each
  other): the round-0 build is outside on weight (86 % against 12-39 %),
  body, tail, centroid and voiced, and 13.3 dB from the nearest; the
  layered build the listener called a tuned drum is outside on crest (28.3
  dB against 16-24), centroid and ringing (3 against 0-2); the approved one
  is inside on all eight, 6.9 dB from the nearest. Limits: a few recordings
  make a thin range; the ringing count reads noise partials in band-passed
  noise (a synthetic noise set spanned 0-4), so it catches less against
  noisy sets; sets are for comparison only (keep `refs/` out of git and out
  of shipped sounds unless the licence allows).

## extract_modes

- `extract_modes` fits each partial in the time domain (candidates from a long
  spectrum, then peeling: demodulate the recording minus the other modes,
  line-fit the log envelope and the phase), so decays of fast modes, close
  pairs and the gains are good. A gain is the amplitude the mode has at the
  hit in the recording's own scale: a unit `impulse` into the node reproduces
  the recording's level. Measured (`tests/it/reference.rs`): a rebuilt five-mode
  hit with a noise tick sits 0.69 dB (STFT distance) from its original and
  within 0.1 dB in level; a hard case (30 ms and 20 ms modes, a pair 25 Hz
  apart) 1.21 dB; a rebuild with the modes shifted 10 % sits 13 dB away.
  `residual: "name.wav"` (a file name) also writes the **residual** into the
  sources folder: the recording from 5 ms before its onset minus the modes
  as the `resonators` play them from an impulse at that time. It returns the
  rebuild (`impulse` at the lead-in time, the `resonators`, a `sample` of the
  residual from 0, a `mix`), which sums back to the recording: on the
  five-mode hit 0.22 dB from the original, against 4.85 dB for the modes
  alone (2 modes: 0.22 against 10.44 dB; `the_residual_restores_what_the_modes_miss`).
  The residual holds the hit's noise, rattle and room, and also what the
  fitted partials get wrong, so retuning the modes later leaves their old
  partials in it: keep the modes' pitch and edit their decay and level, or
  re-extract. A recording with under 5 ms before its hit needs the output's
  `fade_in: 0.0005` (the tool says so). **Noisy recordings** (a slam, a
  noisy hit) are not modal: peeled one by one, their partials grow to cancel
  each other (gains in the thousands on band-passed noise peaking at 0.9),
  and the `resonators`, which start every partial at the impulse, would play
  them far louder than the recording (a real door slam: 46 dB). Two guards:
  each peeled amplitude is capped at the recording's peak, and when the
  peeled partials still play over 6 dB louder than the recording the tool
  falls back to an **envelope fit** and says so ("NOTE the recording is not
  modal"): its strongest spectral peaks at least 15 % apart, each with the
  decay of the recording's own envelope around it, levelled so the rebuild
  peaks at the recording's peak. That is a tonal sketch, not the object's
  modes: the real door slam gave 8 peaks at 89-424 Hz, modes alone 16.75 dB
  from the recording (no crack above 2.8 kHz), modes + residual 0.37 dB.
  Band-passed noise: 0.0 dB over its peak, against 85 dB before
  (`a_non_modal_recording_gets_an_envelope_fit`); a clean struck object keeps
  the modal fit (0.2 dB). The residual of an envelope fit also carries the
  sketch's negative (a peak of 1.10 against the noise's 0.89), so change the
  sketch's decays and the residual stops cancelling them.

