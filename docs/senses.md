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


## The descriptor table

What each line of `listen` says. This was moved from AGENTS.md, which keeps a summary.

- `listen`: one PNG (log-frequency spectrogram with octave lines, waveform with
  peak + RMS envelopes, loudness curve; seeds / sweep values tile as labelled
  columns, 8 at most, under ~1.15 MP) and the descriptor table, one line each:
  - `length`, `level` (peak, true peak, LUFS, momentary max, RMS, crest, DC),
    `time` (attack as the 10-90 % rise, peak time, decay -20 / -60 dB, and the silence wording:
    leading / trailing silence, or "all silent"; "loop: n/a (see steadiness)"
    for a loop), `swell/attack (the headline)` (ms from the first sound to 90 %
    of the peak of the 10 ms RMS envelope: a swell reads long, a click near
    zero; the 10-90 % `attack` and `attack detail` measure other things),
    `onsets {n} (40 ms merge); fine {m} (5 ms): ...` (the fine count is the one
    `density` is built on; it resolves hits 5 ms apart; the first 40 are listed
    in ms, 20 to a line, with "... N more fine onsets not shown" after; not
    for a loop), `pitch` (YIN, voiced share, harmonicity).
  - `events: N strong (>= 0.5 of the loudest onset), M weak; last event at T s`
    (from the fine onsets by peak strength: clinks against crackle, a hit
    against its chips; it also says what follows the last event; omitted for a
    loop), and when the ticks overlap the `density` line reads "density
    estimate" (see the `density` claim above).
  - `spectrum (start/middle/end)` centroid / rolloff / flatness, then the
    **whole-sound** line (centroid, rolloff, full-band flatness, and flatness
    over the **occupied band**, the middle 80 % of the energy, "none" when that
    band is too narrow: tonal), `energy shares` (<200 Hz, 200-2k, 2k-8k, >8k),
    `centroid by tenth` and `pitch by tenth` (ten values over the sound; "-"
    where none), and when partials are clearly off a harmonic series an
    `inharmonic: partials at ...` line.
  - the 8 octave-band x 5 time-segment dB table (your main "what does it sound
    like"), `modes`, `stereo` (correlation, width).
  - `attack detail` (first sound to 90 % of peak, and to the sample peak, in
    ms), `density` (events a second, from a 1 ms envelope: the count of hits in
    a rain, a crackle or a footstep) and `density by tenth (events/s)` (ten
    values: does a roll speed up, does a crumple thin out), `steadiness` (loops: loudness over 400 ms
    windows, min / max / spread in LU), and the **onset table** (one row per
    onset: time, strength, attack ms, -20 dB ms, -60 dB ms (`>N` when the hit
    is still sounding at the next onset or the end, N ms in), centroid, pitch
    start>end; pitch reads `n/a` for an inharmonic or unvoiced hit, and a
    burst of 8-60 ms shows `glide up a>b` / `glide down a>b` instead; left out
    for loops: read `density`). Beside it: `modes` ends with `(N ignored:
    slower than the sound)` when partials too slow to fit in the sound were
    left out; `inharmonic: partials at ...` comes from partial *ratios* (the
    separate `inharmonicity:` line below still uses the older pitch-guess
    method, so the two can disagree: trust the ratios for "is it bell-like");
    `closely spaced events: N pairs within 20 ms (at ... ms)` flags double
    hits (a latch, a bounce's end) that the 40 ms onset merge hides.
  - four lines for synthesis checking: `aliasing` (energy off the harmonic grid
    above 4 kHz, relative to the total, dB; n/a without a steady pitched
    segment), `inharmonicity` (cents, mean over the strongest 8 partials; n/a
    under 3 clear partials), `amplitude modulation` and `pitch modulation`
    (the strongest rates and peak-to-peak depths: Hz, dB / cents), `roughness`
    (percent of the loudness envelope's energy at 20-150 Hz).
  `diff: true` shows your previous render of the same request, this one, and
  their dB-difference spectrogram (this minus previous; diverging colours:
  blue = quieter now, red = louder, grey = same, saturating at +-24 dB, the
  scale is drawn). `zoom {t: [from_s, to_s], hz: [low, high]}` (seconds and
  Hz; either key alone; a bad shape errors with this) magnifies an attack or
  a band; with `zoom {t: [a, b]}`
  the descriptor text describes that window too (the line "zoom window a-b s:
  the numbers below describe this window; times count from a" says so), so
  onset, fine-onset and peak times count from `a`. `descriptors: false`
  returns the image alone.

## Density on a regular train

When the counter's own events form a regular train, `density` uses them: each gap must sit close to its neighbours' median, or at a whole multiple of it, so a speed-up or a missed beat still counts. Missed beats are filled in, and the tenths count the filled train. A resonant tick rings into several envelope peaks, and the peak method read a zipper's teeth 5 x too high. The `listen` line says "a regular train of counted events, missed beats filled in" (`tests/it/onsets.rs`, `a_ringing_speeding_train_with_quiet_teeth_reads_its_tooth_rate`).

A periodic envelope under 200 Hz counts as a tick train only if it dips 12 dB or more between ticks (the 1 ms envelope's 90th over its 10th percentile). A servo's 85 Hz gear wobble of 6 dB had read as 111 events/s for 3 hits (`a_wobbling_tone_is_not_a_tick_train`). Known limit: a continuous motor whose envelope repeats above 200 Hz can still read as a dense train through the envelope-peak method. A vending machine's motor read 158/s for 14 events.

## listen in full

- `listen`: one PNG (log-frequency spectrogram, waveform with peak and RMS
  envelopes, loudness curve; seeds / sweep values tile as labelled columns, 8
  at most) and the descriptor table, one line each: length, level, time
  (attack, decay -20 / -60 dB), the headline swell/attack, onsets (40 ms
  merge and a fine 5 ms count), pitch, events strong / weak, spectrum
  start / middle / end and whole-sound (occupied-band flatness), energy
  shares by role band, centroid and pitch by tenth, the 8 octave x 5 segment
  dB table (the main "what does it sound like"), modes, stereo, density, the
  onset table, and aliasing / inharmonicity / modulation / roughness. Every
  line is explained in `docs/senses.md`, "The descriptor table". `diff:
  true` shows the previous render and a dB-difference spectrogram; `zoom
  {t: [a, b], hz: [lo, hi]}` magnifies a window, and the text then
  describes that window (times count from `a`); `descriptors: false` is the
  image alone.
- `heard_as` (a line in `listen`, on when a model is set up; `heard_as:
  false` skips it): the five closest of about 100 sound descriptions
  (`tools/hearing/vocabulary.txt`) an audio-text model
  (`laion/larger_clap_general`, run by `tools/hearing/heard_as.py`) finds
  for each column, and where the spec's brief ranks among them. **A hint,
  not a check:** it called the crate drop the listener rated great "a snare
  drum" and ranked its brief 83 of 102, while the approved door slam's brief
  ranks 1. Read it as "what a stranger might mistake this for". The worker
  starts on the first `listen` (about 4 s) and its Python is
  `SOUNDER_HEARING_PYTHON`, else `~/.local/share/sounder-hearing/venv`;
  without one the line says the hint is off. `SOUNDER_HEARING=off` turns it
  off.

## verify checks in full

- `verify` / `expect` checks: `peak_db` `true_peak_db` `lufs` `duration`
  `silent_edges` `clicks` `dc` `finite` `loop_seam` `bands` `pitch` `onsets`
  `variations` `rebuild` `reference` `reference_set` `macros` `layers` `spec`, and the synthesis checks
  `aliasing` `inharmonicity` `modulation` `roughness`. Each line is PASS /
  FAIL with the measured value; a FAIL says what to change. Four ways to ask:
  - no arguments: the defaults (`finite`, `peak_db` max -0.3, `silent_edges`
    (a loop: `loop_seam`), `dc`, `rebuild`, and `macros` when the sound has
    knobs).
  - `checks: ["clicks"]` adds checks to the defaults; `only: ["clicks", "dc"]`
    runs just those. Only checks that **need no settings** can be named here:
    `finite peak_db true_peak_db silent_edges clicks dc loop_seam variations
    rebuild macros layers spec` (a bare `variations` runs 4 seeds, a bare `layers`
    the `impact` preset). A check that needs settings errors and shows the `expect`
    form to use.
  - `expect` (verify's `expect`, `sound_ops`'s `expect`, or the same fields at
    verify's top level) takes the checks with settings; given alone, only
    those run: `{"duration": {"min": 0.4, "max": 1.2}, "onsets": {"min": 1,
    "max": 3}, "lufs": {"min": -20, "max": -14}, "peak_db": {"max": -3},
    "clicks": true, "pitch": {"hz": 440, "tol_cents": 25}}`. `onsets` takes
    `spacing_ms` (default 40, the descriptors' merge distance, so a fast
    double like a `switch` click or a 200/s tick train reads as one): under
    30 counts with a fine detector above 1 kHz (`spacing_ms: 5` counted a 200/s
    click train within 10 %), over 40 merges more. `onsets` also takes
    `gap: [lo, hi]` (seconds): every gap between consecutive onsets, at the
    `spacing_ms` resolution, must lie in the range, and a FAIL lists the
    offending gap and its times. "Exactly 3 hits 0.15-0.3 s apart" is
    `{"min": 3, "max": 3, "gap": [0.15, 0.3]}`; leave 10 ms of margin on the
    gap bounds (onset times are measured, not exact).
  - **`verify {seeds: N}`** (2..8) re-renders the sound with seeds 1..N (the
    document's seed is replaced for those renders only; nothing is saved) and
    runs the same checks on each; the report groups results by seed ("seed 3:
    loop_seam FAIL ..."). A plain `verify` renders only the seed in the
    document, so a loop or a random texture that passes at seed 1 can still
    fail at another: run `seeds: 4` before calling it finished. `variations`
    (bare, or with settings) is a different check: it compares the seeds with
    each other.
  - Settings of the rest: `bands [{band: "1k" (or Hz), segment 0..4, min_db,
    max_db}]`; `variations {seeds 6, lufs_spread 1, peak_spread_db 1, min_distance 1}`
    (it compares the quantity the output normalizes: the sample-peak spread,
    `peak_spread_db`, when the output uses `peak_db`; the loudness spread,
    `lufs_spread`, otherwise; the other is only reported; the closest pair of
    seeds must also differ by `min_distance` dB of STFT distance);
    a wrong shape (`"variations": 5`) errors with the check's name and its
    fields, as `dc` and `aliasing` do;
    `aliasing true` or `{max_db -40}` (it needs a steady pitch: on a sweep
    it fails "no steady pitched segment", so leave it off a laser or a
    riser); `inharmonicity {min, max}` cents (a
    harmonic tone reads under 2, a bell tens); `modulation {rate_hz, tol_hz
    0.5, min_depth (1 dB or 10 cents), mode "amplitude" | "pitch"}`;
    `roughness {min, max}` percent; `dc {max}` dB; `reference {file,
    max_distance}`.
  - **`layers`**: is it a designed sound? `{"preset": "impact"}` (or bare in
    `checks`) for a hit, slam, knock or blow, `{"preset": "footstep"}` for a
    step (weight 20-70 %, body 10-60 %: a floor's mode under 200 Hz counts
    as weight; crest at most 22), runs seven measures: `weight`
    15-60 % of the energy under 200 Hz, `body` 30-80 % at 200 Hz-2 kHz,
    `crack` at least 1.5 % above 2 kHz, `voiced_max` 0.3 (pitch tracker),
    `ringing_max` 1 (partials within 6 dB of the strongest that take over
    0.6 s to fall 60 dB, and not longer than 1.5 x the sound: a longer estimate
    is a misread), `crest_db` at most 21 over the active frames (10 ms frames
    within 40 dB of the loudest: silence between hits is not counted), and `dry` (no `reverb` or
    `convolve` node, patches included). A field given beside the preset
    replaces the preset's (`tail_ms {min, max}`, the time to -20 dB, has no
    preset value). Each FAIL names the role and the layer to add. The
    approved slam and the real recording pass; the round-0 and tuned-drum
    builds fail (`tests/it/layers.rs`). Ringing metal (a pipe, a sign, a bell)
    fails the preset's `ringing_max` and crest by design: give
    `{"preset": "impact", "ringing_max": 6, "crest_db": {"max": 30}}` or skip
    `layers` for it. Not a default: a UI blip or a drone has
    no weight or crack. `summary` warns when 80 % of a oneshot's energy sits
    in one role band, and `listen`'s energy shares line names the roles.
  `aliasing`, `inharmonicity`, `modulation` and `roughness` cannot be named in
  `checks` / `only`: put them in `expect`. `reference {file, max_distance}` fails
  when the STFT distance to the WAV (aligned and loudness-matched as in
  `listen reference`) exceeds `max_distance` dB, and then names the biggest
  band differences. `variations` needs a node that reads the seed.
