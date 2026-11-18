# voice_fix: natural glottis, tract and bubble

Batch 2 blind labels: glottis -> tract creature calls were heard as "SAM"
(1980s speech synthesis), "muffled groan, artificial"; `bubble` drips as an
"iPhone keyboard click" and a "cartoon thought-bubble pop". Measured causes:
a perfectly periodic, aliasing LF pulse with flat breath noise; fixed,
narrow formants; a lone clean Minnaert sine. Nothing here was heard.

Each node takes `natural` (default **true**); `natural: false` is the old
output bit for bit (the M8 hash pins now set it).

## glottis

- per-cycle jitter 0.6 % and shimmer 6 % (peak), added to `roughness`;
- a slow seeded pitch drift, +-12 cents peak, ~1 Hz;
- the pulse rendered 4x oversampled through the halfband chain (`Oversampler`);
- aspiration high-passed (first difference) and gated by the glottal flow
  (`0.1 + 0.9 flow`), default `breath` 0.08 (was 0.05).

## tract

- every bandwidth x1.4 (presets and `formants` lists);
- each formant drifts on its own by up to +-3 %, ~0.7 Hz, seeded (the node
  is now random by default).

## bubble

- a high-passed noise tick at birth (time constant 0.8 ms, 0.4x the ring);
- extra seeded damping, 1.15..1.5x foley's decay rate;
- +-15 % noisy amplitude wobble (~250 Hz smoothing).

## Measured (`crates/sfx_analysis/tests/voice_fix.rs`)

| what | old | natural |
|---|---|---|
| glottis f0 2900: off/on-grid energy above 4 kHz minus below | 21.1 dB | 6.1 dB |
| glottis f0 140: YIN pitch spread over 2 s | 3.1 cents | 25.0 cents |
| glottis: energy share above 6 kHz | -29.6 dB | -27.3 dB |
| tract /a/: centroid by tenth, max / min | 1.012 | 1.037 |
| bubble 2.5 mm: first 4 ms above 6 kHz | -78.9 dB | -34.5 dB |
| bubble: decay to -20 dB | 0.02 s | 0.01 s |
| bubble: whole-sound flatness | 0.0005 | 0.0113 |
| bubble: pitch | 1657 Hz | 1637 Hz |

Limits: the tract is still a 4-formant cascade with no sub-glottal coupling
or consonants; voiced and harmonicity still read 1.00 on a vowel.
