# `surf`: waves on a shore

After the foley project's shore bed (`foley-synth/src/surf.rs` with
`outdoors::surf`, MIT / Apache-2.0), ported with the same constants. Unheard.

One wave lasts 7 s. It rises as a low roar (two 250 Hz low-passes of noise,
growing with `(t / break)^2`), breaks 1.2-1.8 s in, and then:

- the crash: broadband noise (900 Hz band-pass) plus the roar, falling over
  about 0.7 s;
- trapped air torn into up to 256 Minnaert bubbles (foley's `Bubble`, radii
  from `plunge_radius`, Deane and Stokes' spectrum), `4000 e^(-t/0.5) sqrt(h)`
  a second;
- the foam: a 5 kHz band-pass hiss, wobbling, plus tiny pops, running up the
  beach and soaking in over about 2 s;
- on shingle, the backwash draws pebbles: 10 ms knocks at 2.5-5.5 kHz thinning
  out from `break + 2.5 s` to 6.8 s.

Each wave is peak-normalised (0.25 to 0.45 by height), faded to an exact zero
over its last 50 ms, and scaled by `0.12 * height * (0.4 + 0.6 * shore)`.
Under the waves runs a continuous wash (noise through three 900 Hz band-pass
stages, `0.15 * height / 3 * shore`). The sum is soft-clipped `x / (1 + |x|)`.

## Fields

| Field | Default | Meaning |
|---|---|---|
| `shore` | `sand` | `sand` or `shingle` (foley picks one at random per wave) |
| `wave_height` | 1 | m, 0.2 to 3 |
| `period` | 8 | s from one wave to the next, 1 to 60; under 7 they overlap (2 is foley's full storm, 12 its calm) |
| `distance` | 0 | 0 at the water's edge to 1 far from it (foley's `shore` = 1 - `distance`): waves fall to 40 %, the wash goes |
| `seed` | 0 | mixed with the document seed and node name; each wave gets its own |
| `loop` | in a loop doc | waves that run past the end wrap onto the start; the wash is crossfaded |

Waves start at t = 0 (the first breaks 1.2-1.8 s in). Deviations from foley:
no storm / exposure inputs (`period` replaces them), and the shore is a field
rather than a coin flip.

## Measured

Defaults, 8 s: the loudest 250 ms window starts at 1.25 s (the break); with
`period` 12 over 30 s a wave lifts the 0.5 s envelope over the wash floor
three times, with `period` 2 the waves overlap and the rms is 1.3 times the
calm one's (0.0089 against 0.0067). `distance` 1 is 14 dB under `distance` 0
(0.0014 against 0.0069); 2.5 m is over twice the rms of 0.3 m. The 2 kHz+
energy share in the backwash is only slightly higher on shingle (10.4 against
10.2 %): the foam and wash noise carry most of the highs. Levels are low (the
wash peaks near -45 dBFS): set `output.lufs` or `peak_db`. A `kind: loop`
document passes the default checks (`loop_seam`) for both shores.

Cost: one wave is 7 s of per-sample synthesis, so a minute at `period` 2 is 30
waves, about a second.
