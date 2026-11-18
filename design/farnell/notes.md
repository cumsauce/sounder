# Designing Sound (Farnell): recipes, as sounder graphs

Notes from the user's copy of the book (photos, 2026-11-18): the
signal flow and the numbers of each patch, rewritten in our own words for
rebuilding with sounder nodes. No Pd code is copied. `lop`/`hip` are one-pole
filters, `bp f q` a band-pass, `line` an envelope ramp.

## Fire (ch. 34) — sections: crackle, hiss, lapping, fire-all
- **Crackle**: a random trigger (noise low-passed at 1 Hz, gated so only rare
  peaks fire), each trigger an envelope falling 1 -> 0 over a random 0-30 ms,
  raised to the 4th power (two squarings), times white noise through a
  band-pass Q 1 whose centre is 1500 + 500 x (the same random 0-30), so short
  pops are low and long ones bright (1.5-16.5 kHz).
- **Lapping (the roar)**: white noise through a band-pass at 30 Hz Q 5, x100,
  high-pass 25 Hz, hard clip at +-0.9, high-pass 25 Hz again, x0.6. The clip
  turns the slow 30 Hz swell into the flapping.
- **Hiss**: white noise high-passed, amplitude modulated by a slow
  noise-derived envelope (the photo of this section was missing; built from
  the description of the hiss band 1.5-16 kHz).
- **Fire-all**: four independent generators (crackle + hiss + lapping each)
  through band-passes at 600 Hz Q 0.2, 1200 Hz Q 0.6, 2600 Hz Q 0.4 and a
  high-pass at 1 kHz, summed x0.2. The four bands are what make it big.

## Bubbles (ch. 35)
- Event pattern: a cycling counter that fires on primes (29 37 47 67 89 113
  157 197 of 200) and culls half at random; later a Poisson / probability
  version (intensity 70-99 out of 100 per 20 ms tick).
- One bubble: a sine whose pitch rises (exp curve, ~100 ms) and an
  attack-decay amplitude (10 ms attack, 80 ms decay) placed so the amplitude
  peaks during the rise; base 1-3 kHz; output x0.1 then high-pass 60 Hz.
- Bubble with size s (0..1): duration 90 ms x s, pitch from (1 - s) x 300 +
  100 on a 2 kHz base, amplitude proportional to s plus a small offset.
  Bigger bubbles: lower, longer, louder.

## Running and poured water (ch. 36-37)
- Running water: a sine whose frequency jumps to a random value every 6 ms
  (bilinear-exponential random, x1600 + 800 Hz, mostly near the centre),
  slewed (2.69 ms), and gated by the positive part of its own derivative
  (clipped 0..1, low-passed 10 Hz, squared): only rising sweeps sound.
  Several instances for density.
- Poured into a vessel: bubbles + two running-water generators through a
  semi-open pipe (band-passes at 1, 3, 5, 7 x f, Q 30) whose length falls as
  the vessel fills (cavity 2000 Hz x fill + 100).

## Rain (ch. 38)
- A raindrop on water: a pressure bump (impact) then a bubble "pinch"
  (a short rising tone).
- Rain on ground: Gaussian noise band-passed 50 Hz Q 0.4, low-passed 500 Hz,
  squared x10 to modulate a 40-80 Hz oscillator; thresholded at 0.35 and
  high-passed 500 Hz twice: sparse positive pulses.
- Poisson drops as short parabolic pulses: the pulse width sets the apparent
  pitch, level ~ sqrt(period); ~10 generators summed, high-pass 900 Hz.
- Ambient rain: thresholded filtered noise spikes ("drops" freq 7.7, res
  0.013, threshold 0.16, amp 3), high-pass 2 kHz; plus low noise bands at
  10 Hz and 300 Hz for the rumble on big surfaces.

## Electricity (ch. 39)
- Hum: two phasors (sawtooths) at 99.8 and 100.2 Hz summed, minus 1, clipped
  hard (adds harmonics, beats at 0.4 Hz), amplitude modulated by low-passed
  squared noise; through a comb (delay ~10-30 ms, feedback 0.2, low-pass 3 kHz
  in the loop) whose delay the same modulator moves.
- Arcing: chirp pulses at the hum rate (a reversed phasor squashed in time by
  a random modulator, squared and cos'd: x12 + 0.1 offset puts the peak near
  5 kHz), high-passed 300 Hz, switched on and off by a random gate (~3 Hz)
  and when the hum level is high.
- Snap (a big spark): a noise blast with a quartic envelope plus a sine swept
  7 kHz -> 20 Hz, through a spark formant bank: band-passes 4600 Hz Q 5 (x2),
  7200 Hz Q 5, 480 Hz Q 7 (x1.2), 720 Hz Q 8 (x2.5), then 2500 Hz Q 0.5.
  Fired when the hum level crosses a threshold.

## Thunder (ch. 40)
- Strike: ~20 noise bursts with growing gaps (until 100 ms), each a pair of
  band-passes (100-1300 Hz and half that, Q 3), brighter and shorter early
  (decay 50 ms to 3 s).
- Rumble: two independent slow noises: one rectified drives a phasor
  (10-20 Hz), its falling edges sample-and-hold the other, a leaky integrator
  gives a wandering triangle, abs, parabolic shaper: half-circle pulses;
  8 s decay.
- Afterimage: noise x heavily gained, clipped low noise, band-pass 333 Hz Q 4,
  from 200 ms (building reflections).
- Deep noise: noise low-passed twice at 80 Hz, high-pass 15, x12 clipped,
  low-passed twice at 80 again; starts after a second strike, swells.
- Echoes: 30-40 delay taps, short ones brighter and left, long ones darker.

## Wind (ch. 41)
- Speed: slow sine (0.1 Hz) x0.25 + 0.25, plus gusts (noise lop 0.5 Hz twice,
  DC-blocked, x50; scaled by (speed + 0.5)^2 - 0.125) and squalls (above 0.4
  of speed only, (x - 0.4) x 8 squared, noise lop 3 Hz). Clip 0..1.
- Static noise: a wide band-pass at 800 Hz Q 1 x (speed + 0.2) x 0.3.
- Swept noise: a notch swept from 800 Hz up to a few kHz by speed.
- Whistling wires: narrow band-passes (Q 60) at 600 + 400 x speed and
  1000 + 1000 x speed, amplitude (speed + 0.12)^2, each reading speed delayed
  100 ms / 1000 ms.
- Leaves: speed delayed 2 s and low-passed 0.1 Hz; noise above a falling
  threshold passes (sparse crackle at low speed), hp 200, lp 4 kHz.
- Howls: within a speed window (clip 0.35-0.6), one cosine cycle, lop 0.5 Hz;
  amplitude-modulates a narrow band (bp 400 Q 40) plus an oscillator at
  200 x speed + 30 Hz.

## Switches (ch. 42)
- Click: noise x an envelope rising in 1 ms and falling in 20 ms, band-pass
  5 kHz Q 12. Four clicks at 3, 4, 5, 7 kHz, 0 / 30 / 140 / 150 ms (the plastic
  ones first, the metal ones later), into a body: delay 50 ms with feedback
  0.1 through bp 700 Q 3.
- Short ping: two sines near 10 kHz x a 50 ms square decay.
- Slide switch: "pnoise" (three ring-modulated phasors at 3345, 2980, 4790 Hz,
  cos, hp 1 kHz: dense band noise with steady partials) under an envelope
  that rises to 0.46 over 100 ms, jumps to 1 and decays in 50 ms (quartic),
  two resonant poles 0.68; a second click 200 ms later with other colours;
  the ping (10500, 9453 Hz) on the second; into a body delay 25 ms, lp 400,
  feedback 0.3.

Engine pages were not in this set.
