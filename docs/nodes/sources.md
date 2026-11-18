# `osc` sync / unison / pm, `noise` colours, `impulse` blurb

## `osc`

A band-limited oscillator (source). New fields: hard sync, unison stack with detune and stereo spread, phase modulation.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `shape` | word | `sine` | `sine` `saw` `square` `tri` `pulse` | waveform |
| `freq` | signal | required | Hz | pitch (a node here sweeps or FMs) |
| `phase` | number | 0 | 0..1 | start phase (with unison, voice 0 only) |
| `width` | signal | 0.5 | 0.01..0.99 | pulse width |
| `amp` | signal | 1 | linear | level; unison voices are summed, not averaged |
| `sync` | signal (mono) | none | rising zero crossing | each crossing resets the phase, polyBLEP at the reset, crossing placed to a fraction of a sample by linear interpolation |
| `unison` | whole number | 1 | 1..16 (error past 16) | stacked voices; more than 1 makes the node random |
| `detune` | number | 0 | cents, 0..1200 (error past 1200) | voices spread evenly from -detune to +detune |
| `spread` | number | 0 | 0..1 | above 0 the output is stereo; voices alternate sides, outermost hard-panned at 1; balance law, a centred voice is unchanged in both channels |
| `pm` | signal | none | cycles | added to the phase; an `osc` here makes FM |
| `seed` | whole number | 0 | | picks the unison start phases (voices 1.. start at seeded random phases) |

A signal field takes a number, `"$param"`, `"=expr"` or the name of another node.

### Recipe

```json
[
  {"type": "ramp", "name": "pitch", "from": 400, "to": 180, "time": 0.4, "curve": "exp"},
  {"type": "osc", "name": "stack", "shape": "saw", "freq": "pitch", "unison": 7, "detune": 18, "spread": 0.6, "amp": 0.3}
]
```

Give the human macros on `detune` (width of the cluster) and `spread`. For hard sync, sweep the slave `freq` while a fixed-rate `osc` is the `sync` master.

### Measured

Tests in `crates/sfx_analysis/tests/s1.rs`.

| What | Measured | Test |
|---|---|---|
| sync that never fires vs plain saw | max sample difference 0 | `sync_that_never_fires_changes_nothing` |
| synced saw, master 110 Hz, slave 250 Hz fixed: alias_db | -38.8 dB (YIN 0.02 % off 110) | `sync_adds_no_aliasing_beyond_the_plain_saw` |
| same, slave 777 / 1234.5 / 1999 Hz | -32.7 / -31.1 / -29.8 dB; a plain saw at the slave rate reads -32.6 / -30.9 / -29.8 | same |
| synced saw, slave 120 -> 200 Hz sweep: YIN | within 0.54 % of 110 Hz, 90 of 90 frames voiced | `synced_saw_pitch_reads_the_master_when_the_slave_is_close` |
| unison 7, detune 30, sine 440 Hz: spectral lines | 7 lines from -30.0 to +30.0 cents (60 wide) | `unison_makes_a_cluster_as_loud_as_sqrt_n_voices` |
| same: RMS vs one voice + 10 log10(7) | 5.45 dB vs 5.44 dB | same |
| `pm` sine at 2 f0, index 1 rad: 3 f0 / 5 f0 re carrier | -11.4 / -19.0 dB (unmodulated: -117 dB) | `phase_modulation_makes_sidebands` |
| unison 5 saw, spread 1 | stereo, bit-identical on a second render, another `seed` differs | `unison_is_seeded_and_stereo_with_spread` |
| every shape at freq 0, +-1e6, -3000, 24000, pm 40 / -1000, with sync; unison 16 detune 1200 | finite | `deterministic_and_finite_at_the_extremes` |

### Honest limits

- Not met, `#[ignore]`d with the measured value: the card's "alias_db under -50 dB" for a synced saw (master 110 Hz, slave 200 -> 2000 Hz). Measured -38.8 dB at a fixed 250 Hz slave and +2.1 dB on the sweep (`synced_saw_alias_under_minus_50`, `synced_saw_sweep_alias_under_minus_50`). A plain polyBLEP saw already reads -41.6 dB at 110 Hz and -29.8 dB at 1999 Hz on this metric, so the two-point polyBLEP cannot reach -50; sync adds no aliasing beyond the plain saw (passing test above). On a sweep the waveform changes from one master period to the next (correlation at one period 0.675), so the signal is not periodic at 110 Hz and the grid metric does not apply.
- Not met, `#[ignore]`d: "YIN pitch stays 110 within 1 %" on the 200 -> 2000 Hz sweep (`synced_saw_sweep_yin_reads_110`). YIN reports the slave's rate (169 Hz at 0.07 s, 1834 Hz at 0.91 s; 65 of 90 frames voiced) because within a window the slave cycles repeat almost exactly. It holds (0.54 %) when the slave is under twice the master.
- The sync crossing time is a linear interpolation between samples, so a master far from a straight line at its zero crossing (low-rate or very high-rate masters) places the reset slightly off.
- The triangle under sync resets its phase but not its integrator, and takes the reset's band-limiting correction into the integrator input; its amplitude can drift. Sync of a sine uses the same step correction, which is approximate.
- `pm` changes the polyBLEP width by the modulation slope, so large, fast `pm` on a saw or pulse still aliases.
- Unison is a sum: 7 voices are about 17 dB louder than 1 (the output stage normalizes; set `amp` to about 1/sqrt(unison) otherwise).
- Output is finite at every extreme tried (above); none of it has been heard.

## `noise`

Seeded noise (source). New colours `blue`, `violet`, `crackle`; `density` is now a signal.

### Fields

| Field | Type | Default | Range / unit | What it does |
|---|---|---|---|---|
| `color` | word | `white` | `white` `pink` `brown` `blue` `violet` `velvet` `crackle` | spectrum / kind |
| `density` | signal | 2000 velvet, 50 crackle | impulses per second, > 0 for a number | velvet (one impulse per cell, cell length = 1/density at its start) and crackle (Poisson, probability density / sr per sample, random sign and height 0.1..1); velvet is capped at one a sample |
| `seed` | whole number | 0 | | mixed with the document seed and the node name |
| `amp` | signal | 1 | linear | level |

`blue` is the first difference of pink noise (+3 dB/oct), `violet` the first difference of white (+6 dB/oct); both are scaled to about the same RMS (-7.9 / -7.8 dB). Velvet with a constant `density` is bit-identical to before.

### Recipe

```json
[
  {"type": "ramp", "name": "rate", "from": 20, "to": 400, "time": 2, "curve": "linear"},
  {"type": "noise", "name": "fire", "color": "crackle", "density": "rate", "amp": 0.8}
]
```

Listen for the impulse rate rising; give the human a macro on a `ramp`'s `to`.

### Measured

| What | Measured | Test |
|---|---|---|
| blue, octave steps 200 Hz .. 6.4 kHz (PSD) | 3.29, 2.98, 3.01, 2.90 dB; 200-400 Hz to 5-10 kHz 2.98 dB/oct | `blue_and_violet_tilt_up` |
| violet, same | 6.31, 5.99, 6.06, 5.87 dB; 5.99 dB/oct | same |
| crackle density 50, 1 s, seed 0 | 45 impulses; mean of 20 seeds 48.7 | `crackle_counts_follow_density` |
| crackle density ramped 100 -> 500 over 4 s | 400 then 823 (ratio 2.06) | same |
| velvet 900 | 900 impulses | `velvet_density_is_a_signal_too` |
| velvet density ramped 200 -> 1000 over 2 s | 400 then 799 | same |
| crackle / velvet density 1e9, blue | finite, bit-identical | `deterministic_and_finite_at_the_extremes` |

### Honest limits

- Crackle is a Poisson process: one seed's count in 1 s at density 50 has standard deviation about 7, so "40 to 60" holds for seed 0 (45) but not for every seed; the mean over 20 seeds is 48.7.
- The first difference tilts by 2 sin(pi f / sr), about 0.6 dB under a straight slope at 10 kHz at 48 kHz; the pink source follows Kellet's filter.
- Velvet with a signal density reads the density at the start of each cell, so a very low density followed by a high one is only noticed at the next cell.
- None of it has been heard.

## `impulse`

Only the blurb and field comment changed: the field is `at`, one time or a list of times in seconds (default 0). The describe_op blurb now says so.
