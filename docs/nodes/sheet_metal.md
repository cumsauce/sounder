# Sheet metal giving way: `buckle` and `tear`

Two sources for metal failing, in the crumple family (code:
`crates/sfx_graph/src/nodes/crackle.rs`; tests:
`crates/sfx_analysis/tests/it/sheet_metal.rs`). Both have three ports:

- `out` (the default): what the air hears, for listening or layering.
- `force`: the excitation alone (no ring, no hiss), for a body: `modal`,
  `plate`, `structure`, `resonators`.
- `events`: an event stream for `voices` / `debris` / `trigger_env`; each
  event carries `value` (strength 0..1) and `$size` (metres).

Both are random (they read `seed`: the same seed renders the same bits, another
seed another sound) and causal. They exist because `crumple` (band-passed
bursts at 1.5-9 kHz) and stick-slip `friction` on modal plates were heard as
"ripping fabric", not metal, for aircraft skin tearing and a fuselage breaking.

Materials (`material`): `steel` (E 200 GPa, 7850 kg/m^3), `aluminium` (alias
`aluminum`; 70 GPa, 2700), `copper` (120 GPa, 8960), `titanium` (110 GPa,
4500).

## `buckle`

`{[load 0.5], [rate], [size 1], [thickness 0.002], [material steel],
[exponent 2], [ring 0.5], [seed]}`

Sheet metal oil-canning and buckling: a process of snap-through events. After
Cirio et al., "Crumpling Sound Synthesis" (SIGGRAPH Asia 2016): each buckling
event is a facet snapping through, its sizes power-law distributed, kicking the
sheet's modes with the momentum of the buckling region; small events are
drawn stochastically (here every event is).

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `load` (alias `force`) | signal | 0.5 | 0..1 | how hard the sheet is pushed: the event rate (unless `rate`) and the strain each facet snaps under |
| `rate` | signal | none | 0..2000 /s | events per second in place of the law below |
| `size` | number | 1 | 0.01..20 m | side of the sheet; the largest facet is half of it |
| `thickness` (alias `gauge`) | number | 0.002 | 0.00001..0.05 m | sets the smallest facet, the pitch and the strength |
| `material` | word | steel | see above | stiffness and density |
| `exponent` | number | 2 | 0.5..6 | facets larger than l go as l^-exponent |
| `ring` | number | 0.5 | 0..1 | each facet's own ring on `out` (not on `force`) |

A number out of range is an error naming the field; a signal is clamped.

### The laws

- **Facet sizes**: l is a Pareto truncated to l_min..l_max, l_max = size / 2,
  l_min = 150 x thickness (never over l_max: a sheet smaller than 300
  thicknesses snaps at one size). 150 is set so that 0.02 mm foil rings where
  `crumple` puts foil (its smallest facets near 10 kHz) and a 2 mm steel
  panel's smallest facets (0.3 m) ring near 100 Hz.
- **Pitch**: each facet's lowest mode is that of a simply supported square
  plate, f = pi sqrt(E / (12 rho (1 - 0.3^2))) h / l^2 (clamped to
  20 Hz..0.9 Nyquist).
- **Rate** (no `rate`): load x 3 x size / l_min per second, Poisson arrivals:
  10/s at load 1 for a 1 m, 2 mm panel; 200/s for a 0.2 m sheet of foil. The
  rate grows with the room for facets (size / smallest facet), so thick metal
  has few events and foil many, by the same law.
- **Strength**: the facet's dimple rises (2 / pi) l sqrt(strain) (an Euler
  buckle, strain = load x 0.01) in half a period of its mode, so its momentum
  is p = (4 / pi) rho k h^2 l sqrt(strain): bigger and thicker facets kick
  harder, thickness counting twice. `value` = (p / p_ref)^(1/4), p_ref a
  1 m facet of 6 mm steel at load 1, so 16x the momentum is 2x the value.
- **Force pulse** (`force`, and on `out`): it rises over 3/4 of the half
  period (half a cosine) and stops in at most 0.1 ms as the facet lands in its
  inverted shape (that abrupt stop is the pop of a big slow snap). Its peak is
  value x l_min / l, so its impulse grows as l: small facets are sharp ticks,
  big ones longer, heavier shoves that carry the lows.
- **Ring** (`out` only): white noise band-passed (Q 2) at the facet's mode,
  starting at an RMS of the pulse peak x `ring`, decaying with a time constant
  of 4 periods (at least 0.8 ms).
- `out` is high-passed at 20 Hz (the pulses carry DC); `force` is not.
- At most 256 snaps sound at once (more still go on `events`); at most 200,000
  events (an error past it).

### Measured

`thicker_and_larger_sheets_snap_fewer_stronger_lower` (load 0.6, 4 s):

| sheet | events / s | mean value | centroid of `out` |
|---|---|---|---|
| foil, 0.2 m, 0.02 mm aluminium | 119.5 | 0.011 | 5.6 kHz |
| steel 1 m, 1 mm | 14.8 | 0.26 | 283 Hz |
| steel 1 m, 3 mm | 5.8 | 0.55 | 190 Hz |
| steel 4 m, 3 mm | 18.8 | 0.61 | 145 Hz |

Rate: 41 / 190 events over 20 s at load 0.25 / 1 (law 50 / 200) for a 0.5 m,
1 mm sheet; `rate` 50 gave 209 in 4 s; a load ramp 0.05 -> 1 over 4 s gave
24 events in the first half, 60 in the second.

Foil against `crumple` (hardness 1): foil buckle centroid 6.1 kHz, energy
shares (under 200 Hz / 200-2k / 2-8k / over 8k) 2.8 / 17.6 / 46.5 / 33.1 %,
about 140 fine events per second at load 0.5, unvoiced; crumple 7.3 kHz,
0.0 / 1.5 / 63.4 / 35.1 %. The buckle has more under 2 kHz: its big, slow
facets.

A 1 m, 2 mm steel panel alone (`out`, load 0.6): centroid 175 Hz, 89 % of
the energy under 200 Hz, 3-7 snaps a second each detected as a fine onset
(the abrupt landing). Through a `modal` metal plate (the recipe below):
centroid 270 Hz, 69 % under 200 Hz. The level is not normalized (foil is
about 35 dB under a thick panel): use `output.peak_db`.

### Recipe: a panel oil-canning, through a body

```json
[
  {"type": "wander", "name": "push", "rate": 1.5, "range": [0.2, 0.9], "seed": 1},
  {"type": "buckle", "name": "panel", "load": "push", "size": 1.2, "thickness": 0.0015, "seed": 2},
  {"type": "modal", "name": "skin", "excite": "panel.force", "material": "metal", "size": 1.2, "noise": 0.4, "noise_width": 0.1, "loss": 0.5},
  {"type": "mix", "name": "m", "inputs": ["skin", "panel"]},
  {"type": "output", "name": "out", "input": "m", "peak_db": -1}
]
```

## `tear`

`{[speed 10], [thickness 0.001], [material aluminium], [grain], [jitter 1],
[pitch_m], [pop_db 6], [hiss 0.5], [rumble 0.3], [seed]}`

Ductile metal tearing along a line: the crack advances by micro-fractures
(void coalescence, a step of about half the thickness), each an impulse into
the sheet (after Zheng and James, "Rigid-Body Fracture Sound with Precomputed
Soundbanks": fracture as impulses into the body), plus the broadband sound the
moving tear radiates.

| Field | Type | Default | Range | What it does |
|---|---|---|---|---|
| `speed` | signal | 10 | 0..300 m/s | how fast the tear runs; 0 is silence |
| `thickness` (alias `gauge`) | number | 0.001 | 0.0001..0.02 m | thinner is brighter and quieter |
| `material` | word | aluminium | see above | stiffer is brighter |
| `grain` | number | thickness / 2 | 0.00001..0.1 m | tear length per micro-fracture |
| `jitter` | number | 1 | 0..1 | gap spread: 1 random arrivals, 0 a regular train at speed / grain per second (still unpitched at the defaults: see Measured) |
| `pitch_m` (aliases `pitch`, `rivet_pitch`) | number | none | 0.005..10 m | rivet / stringer spacing: a pop each time one is crossed |
| `pop_db` | number | 6 | 0..30 dB | a pop over the largest micro-fracture |
| `hiss` | number | 0.5 | 0..1 | the radiated hiss on `out` |
| `rumble` | number | 0.3 | 0..1 | the sheet's flutter under 125 Hz on `out` |

### The laws

- **Micro-fractures** (`force`): speed / grain per second (20,000/s at the
  defaults), gaps (1 - jitter) + jitter x Exp(1) mean gaps, so the default is
  a Poisson train with no pitch. Amplitudes are a Pareto (P(A > a) = a^-2,
  capped at 6x the smallest), random sign, times 0.08 sqrt(thickness / 1 mm).
  Over 20,000/s they are summed into 20,000, each louder by
  sqrt(rate / 20,000), so the energy keeps growing with speed.
- **Ligament snaps**: one micro-fracture per 200 thicknesses of tear (one per
  0.2 m at 1 mm: 50/s at 10 m/s) is the largest; these go on `events`
  (value 0.5, `$size` = grain).
- **Rivet pops** (with `pitch_m`): each time the tear length passes the next
  rivet (spacing +-2 %, the first at half a pitch), a 0.4 ms raised-cosine
  pulse at `pop_db` over the largest micro-fracture, on `force` and as an event
  (value 1, `$size` = pitch_m). At speed / pitch_m per second they are heard as
  that pitch.
- **Radiated** (`out`): the micro-fractures plus noise (level x `hiss`,
  following the speed over 30 ms up and 80 ms down, as sqrt(speed / 10))
  through a band: high-pass at f/3 and low-pass at 2.5 f (12 dB/oct each),
  then -6 dB/oct above f/2 (a one-pole, with 0.35 of the highs kept). The
  centre f = 2.6 kHz x (1 mm / thickness)^0.3 x sqrt(c / c_aluminium) x
  (speed / 10)^0.15, wobbling +-25 % toward a new random target about every
  0.1 s, so it moves and never holds a pitch.
- **Rumble** (`out`): noise band-passed to 70-130 Hz, amplitude-modulated at a
  wandering 8-16 Hz (the flapping sheet), following the speed like the hiss.
- At most 200,000 events (an error past it).

### Measured against recorded tears

Recorded aluminium tears the listener preferred (three takes, 2 s, mono
48 kHz; not in git): centroid 3.0 / 3.0 / 3.9 kHz, 1.3-4.7 % of the energy
under 200 Hz, unvoiced, spectral flatness 0.02-0.07, the middle 80 % of the
energy in about 0.7-7 kHz, octave bands (63 Hz .. 8k+, take 1) -40 -33 -35
-33 -27 -24 -25 -27 dB.

`tear` at the defaults (1 mm aluminium, seed 1, 2 s; `tear_sits_where_recorded_tears_do`):

| speed | centroid | under 200 Hz | voiced | flatness | middle 80 % |
|---|---|---|---|---|---|
| 5 m/s | 2.8 kHz | 3.3 % | 0.00 | 0.027 | 0.8-5.8 kHz |
| 10 m/s | 3.2 kHz | 2.5 % | 0.00 | 0.032 | 0.9-6.4 kHz |
| 20 m/s | 3.5 kHz | 2.4 % | 0.00 | 0.041 | 1.0-7.1 kHz |

Octave bands at 10 m/s, relative to the loudest: 63 Hz -17, 125 -12.5, 250
-16, 500 -9.5, 1k -2, 2k 0, 4k 0, 8k+ -3 dB (the recording, take 1: -16, -9,
-11, -9, -2.5, 0, -0.7, -3). The swell of a recording (0.03-0.6 s) comes
from the speed: give `speed` a `ramp`. 5 -> 20 m/s: +7 dB, centroid 2.9 ->
3.7 kHz, snaps on `events` 104 -> 389 over 4 s (law 100 -> 400).

Pitch: voiced 0.00 at the defaults; with `pitch_m` 0.025 at 10 m/s, voiced
1.00 at 400 Hz (`pop_db` 12). `jitter` 0 with `grain` 25 mm (a regular 400 Hz
train) still measured voiced 0.00: the random signs and sizes of the
micro-fractures and the hiss hide it, so `pitch_m` is the way to ask for a
pitch.

Slow tears (`el_metal_tear_slow`) are something else: centroid 150-240 Hz,
over half the energy under 200 Hz, voiced 0.2-0.3 near 50 Hz, a groan. `tear`
does not make that; build it from `friction` into a large `modal` / `plate`.

### Recipe: aircraft skin torn open

```json
[
  {"type": "ramp", "name": "run", "from": 2, "to": 18, "time": 0.3, "curve": "linear"},
  {"type": "tear", "name": "skin", "speed": "run", "thickness": 0.0012, "pitch_m": 0.03, "seed": 1},
  {"type": "modal", "name": "body", "excite": "skin.force", "material": "metal", "size": 0.6, "noise": 0.4, "noise_width": 0.1, "loss": 0.6, "gain_db": -18},
  {"type": "mix", "name": "m", "inputs": ["skin", "body"]},
  {"type": "output", "name": "out", "input": "m", "lufs": -16}
]
```

Measured (2 s): centroid 1.5 kHz, 87 % of the energy in 200-2000 Hz, voiced
0.00: the rivet pops (up to 600 a second at 18 m/s) and the plate's modes
pull it well under a bare tear; lower `pop_db` or drop `pitch_m` for the
recorded tears' 3 kHz. Not heard yet.
