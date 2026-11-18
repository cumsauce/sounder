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

# Second set (photos 2026-11-18): clocks, motors, cars, fans, footsteps, guns, explosions

## Clocks (ch. 43)
- Metal click: noise gated by three short decays, each into a narrow band-pass
  (Q 30): `{f1, d1, f2, d2, f3, d3}`, x3. Tick 1: 6543 Hz 30 ms, 3345 Hz 20 ms,
  1356 Hz 10 ms; tick 2: 7543 / 3988 / 2765 Hz (30, 20, 10 ms). Noise hp 200.
- A "bang burst" counter fires ten steps a few ms apart; a `select` picks which
  steps fire the click (a fine-grained click cluster).
- Escapement: two sines, 10 kHz and 8 kHz, alternating under short square
  decays (the compensator spring).
- Body: two delays (box length and width; 0.1 -> 10 ms, filter 1 kHz =
  1 / period) with band-passes Q 3 in the loops, feedback 0.3: hard plastic or
  damped thin metal.
- All parts: metro 125 ms, counter mod 8: tick 1 on beats 1 and 5, tick 2 on 3
  and 7, escapement every beat, a hand clunk once a second; body in parallel.

## Motors (ch. 44)
- Speed envelope: a line 0..1 over runtime, x2, split at 1: attack = 1 - (1 -
  x)^6 (fast rise levelling off), decay = linear fall.
- Rotor: noise band-passed 4 kHz Q 1, times brush level, plus DC (rotor level
  0.2); modulated by the phasor's quartic decay (phase^4): clicking spikes.
- Stator: phasor x2, wrap, cos, shaped 1 / (x^2 + 1): a pulse at 4x the base,
  1/4 width, minus 0.5, x stator level.
- FM body resonance: cos(2 pi 178 Hz t + amount x drive), hp 180 twice: the
  carrier stays fixed (the housing), the modulator moves; sidebands come and go.
- Defaults: main speed 22, motor ratio 20, motor 0.2, brush 0.05, rotor 0.1,
  stator 0.3, fan pulse width 4, fan noise 0.2, fan level 0.5.

## Cars (ch. 45)
- Four cylinders: four copies of a phasor offset 0 / 0.25 / 0.5 / 0.75, each
  cos -> 1 / (1 + k x^2) pulse; the width k = 3 x speed... (2 + 3 x (1 -
  speed)); each pulse also modulated by low-passed noise (lop 20 Hz twice, x10
  and x0.5) through short variable delays (5, 10, 15, 20 ms): uneven firing.
- Overtones: three overtone generators from delayed copies of the engine
  phase (transmission delays, x16 / x4 / x8 multiples, wrapped), each a
  cos-shaped pulse with its own phase, frequency (x12) and amplitude.
- Exhaust: a "warping" circular waveguide: two pairs of delays (50 ms buffers,
  12 ms taps) cross-fed, each tap frequency-modulated by the engine signal x40
  (fm1, fm2), feedback; hp 200 twice. Speed phasor lop 0.8, x30.
- Controls: mix cylinders, mix parabolic, speed, 3 transmission delays,
  parabola delay, warp delay, waveguide warp / feedback, lengths, widths,
  overtone phases / freqs / amps.

## Fans (ch. 46)
- Pulse: osc 12 Hz x width k (5 is good), squared, +1, reciprocal: 24 pulses/s
  (an 8-blade fan at 180 RPM), x0.2.
- Noise: noise band-passed 700 Hz Q 1 times the pulse, x0.4, in parallel.
- Doppler: a vcf (Q 5) swept 0-600 Hz by the oscillator (not the narrow
  pulse) over the noise; wide width = swoosh, narrow = purr.
- Fast blades: noise + pulse into a 400 ms delay read at a position moved by
  the oscillator (x chop 10 + 200): a crude Doppler for propellers.
- Ventilation: motor (ratio 20) drives the fan; a pipe of two delays (22 ms
  feedback 0.3, 70 ms feedback 0.499).

## Footsteps (ch. 49)
- Two-phase walk timebase: a phasor at walk speed x6, the right foot offset
  180 deg; each phase capped (`min`) and rescaled so slow walks overlap.
- GRF pulse: 1.5 (1 - x)(n x^3 - n x), n = 3.3333 (rises faster, falls slower
  than a half cosine). Three pulses per step (heel, ball, edge roll) from the
  phase split 0-0.333 / 0.125-0.875 / 0.667-1, compressed to 3/4.
- Gravel texture: two filtered noises (lop 300, lop 2000) divided, hp 400,
  squared, x0.01, clipped: sparse spikes; a 50 Hz-low-passed noise x50000,
  +1000 x GRF, clipped 500-10000 Hz, drives a vcf Q 3 over the spikes; hp 200,
  x2, times the GRF. Denser and brighter as the force rises.

## Guns (ch. 53)
- Shell chirp: a line 1 -> 0 over 30-60 ms, x1.05, 64^x, wrap, cos (-0.25):
  constant amplitude sweeping 10 kHz -> a few hundred Hz.
- Barrel wave: the same with 2^x (four cycles, 100-200 Hz) times the line;
  1-2 ms after the shell (bullet down a 1 m barrel at 700 m/s).
- Noise: noise x a decay of three squarings (~200 ms).
- Body: eight parallel band-passes Q 1 (four at 200-800 Hz, a cluster near
  2.5 kHz), e.g. 2225 647 1278 1329 2615 500 1682 2243 or 2637 2975 2383 906
  2867 2924 1959 971; x gain, tanh.
- N-wave shock: +1 in 1 ms, -1 1 ms later, back to 0; plus a copy delayed
  ~2 ms through lop 100 (ground reflection); height sets the gap.
- Reload: a slide (noise hp 1000 band-passed 1000 + 200 x 4500 ramp, Q 4,
  over 200 ms) then three click clusters (noise hp 300 into narrow bands:
  4564, 4237, 4876, 345, 256, 11023, 5432, 6290, 2470 Hz, each a 1-70 ms
  decay), fed through the same body bank as the shot.

## Explosions (ch. 54)
- Shockwave: a line 1 -> 0 in 10 ms, squared, x5, wrap, cos, hp 1: five
  cycles sweeping 500 Hz -> 0, a dull thump.
- Blast noise: noise through a vcf swept down from 7 kHz (pow 2 curve; Q 1.5
  bright to 5 low), hp 20, x50, clipped (a random square), then used as phase
  into cos with a fast line ("sinusoidal noise"), bp 6 kHz Q 1, hp 20 x3.
  30 ms long; three in quick succession (30 ms, then random 8-58 ms and
  15-25 ms apart: "bam-bam").
- Fireball: low-passed noise (bp 70, lop 70 twice), swept 40 -> 140 Hz and
  slowly back, quartic amplitude, rise 100 ms, 8 s long, x10, hp 5.
- Effects: a pressure-wave delay (500 ms echo whose delay shortens after the
  shock: rising pitch), and a swept comb (taps 10, 10 + w, 10 + 2w ms) for a
  rising fireball.
