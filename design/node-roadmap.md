# Node roadmap: what is missing for a wide range of sounds

Written 2026-11-17 against the node set in `crates/sfx_graph/src/nodes/`
(sources, envelopes, filters, time, nonlinear, control, maths, mixing,
offline, spectral, events, voices, expr, modal, foley).

**How to read this.** "Missing" means one of three things:

- **Impossible** today: no combination of nodes does it.
- **Awkward**: buildable from `delay` loops, `expr` or many nodes, but it
  takes so many nodes (or so much per-sample `expr`) that an agent will get
  it wrong or will not try.
- **Uncheckable**: buildable, but nothing in the senses tells the agent
  whether it worked.

Priorities are by how many families of game sound each item unlocks, not by
how hard it is to write. Each item names the sounds that need it.

## What is already covered

Struck and scraped solids (`modal`, `contact`, `friction`, `resonators`,
`extract_modes`), plucked strings (delay loop), noise textures (`noise` +
`filter` + `wander`), particle sounds (`poisson` / `burst` / `bounce` ->
`voices` / `grains`), simple synth zaps (`osc` + `ramp` + `filter`), water
drips, voice-like calls, birds, thunder, layering (`sound`), and offline
pitch / time / spectral work.

Thin or absent: **weapons and explosions, engines and machines, fire, large
water, whooshes and movement, sci-fi / UI synth timbres, creature growls,
electricity, and modulation effects on anything.**

## P0: blocks whole families of sound

| Node | What it is | Status today | Sounds it unlocks |
|---|---|---|---|
| `fm` (or `osc.phase` as a signal input) | Phase-modulation operator: carrier, ratio, index, feedback. `osc.freq` from a node is linear FM, but with no phase input, no ratio and no self-feedback the classic bell / metallic / growl FM recipes drift in pitch and are hard to aim. | Awkward | UI blips, pickups, sci-fi tones, metallic hits, alarms, robot voices, bass growls |
| `osc` additions: `sync` (hard-sync input), `unison` + `detune`, `phase` reset on event | Sync sweeps and stacked detuned saws. Stacking by hand is 7 `osc` + a `mix` per voice. | Awkward (unison), impossible (sync) | Lasers with bite, energy weapons, engines, sirens, thick drones, power-ups |
| `wavetable` / `additive` | An oscillator whose single cycle is a list of harmonic amplitudes (and a second list to morph to), band-limited. | Impossible (only 5 fixed shapes) | Any pitched timbre that is not saw / square: organs, horns, hums, vowel-ish tones, engine orders, magic shimmer |
| `shape` oversampling + more curves | `shape` runs at the base rate: `fold` and `clip` at high drive alias. Add `oversample: 2/4/8`, plus `asym` (even harmonics), `diode`, `tube`, and a `table` (arbitrary transfer curve). | Uncheckable (aliasing is baked in and no check names it) | Distorted guns, engines, growls, radio voices, any "grit" macro |
| `filter` slopes and a ladder / SVF with drive | Biquad 12 dB only. Add `slope: 12/24/48`, a nonlinear ladder (resonance that screams, self-oscillates) and a `morph` between lp / bp / hp. | Awkward (cascade by hand), impossible (ladder) | Resonant sweeps, acid zaps, wind howl, sci-fi doors, anything with a filter-sweep macro |
| `chorus` / `flanger` / `phaser` | One node each (or one `mod_delay` with modes). Buildable from `delay` + `lfo` + `mix`, 5 to 9 nodes with feedback wiring. | Awkward | Jet and whoosh flange, sci-fi shimmer, thickening any thin synth source, underwater, magic |
| `doppler` / `flyby` | A moving source: distance + velocity -> delay (pitch bend), 1/r gain, `air` absorption, pan. `air` covers absorption only. | Awkward | Pass-bys: bullets, arrows, vehicles, projectiles, swings |
| `transient` (shaper) and `gate` / `expand` | Attack / sustain gain from two followers; downward expander. `compress` exists, the opposite does not. | Awkward | Punch on every impact, tightening tails, gated reverb hits |
| `eq` | N bands in one node `[[type, hz, gain_db, q], ...]`. Today: one `filter` node per band. | Awkward | Fitting every sound to a reference; the main fix after `listen reference` names band differences |
| `noise` additions: `blue`, `violet`, `crackle` (sparse impulses with a density signal), band-limited `lfnoise` | `velvet` + `density` is close to crackle but density is a number, not a signal. | Awkward | Fire, vinyl, electricity, geiger, rain bed, steam |

## P1: common sound families with no good model

Physical and procedural models in the spirit of `modal` / `bubble`. Each is a
single node because the recipe is long, tuned, and the same every time.

| Node | Model | Sounds |
|---|---|---|
| `explosion` | Layered: a shock N-wave (`thunder`'s clap, close), a low `decay`ing sub thump with pitch drop, shaped noise body, debris via an events output. Fields: `size`, `distance`, `material` of debris. | Explosions, grenades, cannons, impacts of large objects |
| `gunshot` | Muzzle blast (Friedlander wave), supersonic crack (N-wave), mechanical click, tail through a reflection model. Fields: `caliber`, `barrel`, `suppressed`, `distance`. | All firearms; with `burst` -> automatic fire |
| `engine` | Firing-order pulse train per cylinder into exhaust / intake waveguides, with `rpm` and `load` signals. | Cars, bikes, generators, chainsaws, propellers (with a blade-pass variant) |
| `motor` | Electric: harmonic orders of `rpm` + PWM whine + gear mesh noise + brush noise. | Servos, drones, robots, drills, sci-fi machinery, elevators |
| `fire` | Three layers on shared control: crackle (events), hiss (hp noise), roar / lapping (lp noise with turbulent AM). Fields: `size`, `intensity`. | Torches, campfires, burning, flamethrowers |
| `wind` / `turbulence` | Noise through a resonant bank whose centres follow a gust signal; vortex-shedding (Aeolian) tones at `speed / diameter`. Buildable now, but 8+ nodes. | Wind, whooshes, sword swings, whistling wires, falling |
| `water` (flow) | Many `bubble`s from a population model (rate and radius distribution from `flow`), plus broadband hiss. `poisson` -> `voices` of `bubble` works but caps at `max` and costs per voice. | Streams, pouring, splashes, rain on water, waterfalls, boiling |
| `waveguide` (`tube`, `string`) | Bidirectional delay line with reflection filters, a bow / reed / lip / jet excitation, and tone holes or a tension signal. The delay-loop Karplus form covers only the plucked case. | Horns, flutes, pipes, bowed metal, creaks, bottles, sci-fi drones, whistles, twangs |
| `spark` / `arc` | Sparse impulses with a mains-rate buzz envelope and a resonant body; random re-strikes. | Electricity, tasers, neon hum, shorts, magic lightning |
| `cloth` / `crumple` | Stochastic bursts of filtered noise with a power-law size distribution driven by a `motion` signal. | Clothing, paper, foliage, bags, wings, footsteps in leaves |
| `rolling` / `scrape` | Contact moving over a rough surface: low-passed noise profile read at `speed`, into `modal`. `friction` is stick-slip only. | Rolling balls, wheels, dragging crates, sliding stone doors, skates |
| `shatter` / `fracture` (events) | An event generator: a power-law cascade of fragment sizes and times for `voices`. `poisson` with a ramped rate is the stand-in. | Glass, ice, ceramics, wood splintering, rock falls |
| `footstep` | `contact` heel + toe timing over a `material` surface with a granular layer for loose ground. | The single most requested game sound; today `examples/gravel_step.rs` by hand |
| Modal shapes: `bar`, `tube`, `membrane`, `string`, `shell` in `modal` | Only a square plate exists. Bars (xylophone, pipes, tools), membranes (drums, skins), shells (bowls, bells, helmets) have different mode ratios. | Bells, chimes, drums, tools, armour, bottles, pipes |
| `tract` upgrades: nasal branch, fricative noise injection, consonant targets; `glottis` subharmonics / roughness / fry | Vowels only today. | Growls, roars, hisses, monster and creature voices, grunts, effort sounds |

## P2: processing that widens every source

| Node | What it is | Why |
|---|---|---|
| `vocoder` / `filterbank` | Analysis bands of one signal drive the band gains of another; also a plain N-band noise-band / resonator bank with per-band gains as a list. | Robot voices, talking machines, imposing one sound's spectrum on another, whisper textures |
| `freq_shift` | Single-sideband shift by Hz (Hilbert pair). `pitch_shift` keeps harmonic ratios; this breaks them. | Metallic / alien timbres, barber-pole risers, detuned clang |
| `ringmod` | `math mul` does it, but a named node with carrier `osc` built in and a `mix`. | Daleks, bells, radio, sci-fi |
| Reverb choices: `reverb.type` (`room`, `hall`, `plate`, `spring`, `cave`), early reflections, and a `shimmer` / `gated` mode | One algorithm today; `convolve` needs an IR file the agent cannot make. | Space is half of how a sound reads: indoor gunshots, caves, bathrooms, metal tanks |
| `ir` generator | Synthesizes an impulse response for `convolve`: shoebox room (size, absorption), tube, small box, speaker cabinet, telephone. | Lets `convolve` work with no assets: "through a wall", "in a helmet", "on a radio" |
| `multiband` (split / compress / saturate per band) | Crossover into N bands with a patch per band, or built-in compress / drive. | Loud, dense weapon and explosion masters without pumping |
| `sub` (subharmonic) and `exciter` | Octave-down tracking generator; high-band harmonic generator. | Weight for impacts and explosions; air on dull recordings |
| `granular` cloud | `grains` with built-in scheduling: `density`, `position` scan, `pitch` spread, `size` spread as signals. Today it needs a separate event node and cannot vary size per grain. | Drones, time-freezes, magic, textures from any sample |
| `pitch_shift` formant mode, and a causal (delay-line) shifter | Offline vocoder only; no formant preserve, no live form. | Creature voices from one source, variations without chipmunking |
| `stereo`: `haas`, `decorrelate` (allpass / velvet), `rotate`, mid / side `eq` | `pan` and `width` only; `width` does nothing to mono. | Mono sources are most of what the graph makes; wide ambience and loops need decorrelation |
| `lofi`: `resample` (sample-rate reduce with / without antialias), `bitcrush` dither, `tape` (wow, flutter, saturation, hiss), `codec` | `shape bitcrush` covers bits only. | Retro / 8-bit SFX, radios, old recordings, glitches |
| `glitch` / `stutter` / `scrub` | Buffer repeat, reverse slices, variable-speed read with a position signal. | UI errors, sci-fi malfunctions, time effects, tape stops |
| `spectral` modes: `shift`, `stretch` (inharmonic warp), `smear` (time only), `denoise`, `harmonic` / `percussive` split, `cross` (convolve spectra) | Four modes today. | Sound design from references: split a recording into tone and transient, rebuild each |

## P3: control, sequencing and analysis gaps

These make no sound themselves but decide how many of the above an agent can
drive well.

| Node / feature | What it is | Why |
|---|---|---|
| `env` shapes per segment and loop points; `adsr` retrigger from events | Envelopes fire once at a fixed time. | Anything rhythmic or event-driven without going through `voices` |
| `events` -> control: `gate`, `trigger_env`, `seq` (stepped values per event), `counter` | An event stream can only feed `grains` / `voices`. A stream should be able to step a pitch, fire an envelope or reset a phase in the main graph. | Alarms, melodies for jingles / pickups, ratchets, machine cycles, gear clicks |
| `seq` / `arp` / `notes` | A pitch + duration list, with `scale` quantize and `glide`. | Coin, level-up, fanfare, failure stings: the whole "musical UI" category |
| `chaos` (`logistic`, `lorenz`, `henon`) and `perlin` / `fbm` | `wander` is one flavour of random. | Organic motion: flames, wind gusts, creature wobble, unstable machines |
| `pitch_follow`, `centroid_follow`, `onset` (as signals) | `follower` gives level only. | Sidechain a filter to another sound's brightness; resynthesize a reference's contour |
| `crossfade_loop` | Makes any render loop: equal-power crossfade of tail into head, or spectral loop. | `loop_seam` fails today for anything not designed periodic; every ambience needs this |
| `switch` / `select` / `xfade` by signal | Choose among N inputs with a control. | Game-parameter blends (`rpm` zones, surface types), variations in one doc |
| `curve` / `table` lookup | Arbitrary breakpoint transfer for control signals. `remap` is linear / exp. | Macro maps that are not monotonic; rpm -> order gains |
| `ducker` / sidechain input on `compress` | `compress` keys on its own input. | Layer management: tail ducks under the transient |
| Senses: an **aliasing** check, an **inharmonicity** read-out, **modulation spectrum** (rates of AM / FM), **roughness** | Not nodes, but P0 items (FM, sync, oversampled `shape`) cannot be verified without them. | The agent cannot hear aliasing or tell a tremolo rate; these make the new nodes checkable |

## Suggested order

1. P0 `shape` oversampling, `filter` slopes / ladder, `eq`, `transient` /
   `gate`: small, each improves every existing sound.
2. P0 `fm`, `osc` sync / unison, `wavetable`: opens the synth half
   (UI, sci-fi, weapons).
3. P3 events -> control and `seq`: opens musical UI and machine cycles, and
   makes every later model drivable.
4. P0 `chorus` / `flanger` / `phaser`, `doppler`; P2 reverb types and `ir`.
5. P1 `explosion`, `gunshot`, `footstep`, `wind`, `fire`, `water`: the most
   requested game categories, each one node.
6. P1 `engine`, `motor`, `waveguide`, modal shapes, voice upgrades.
7. P2 the rest; P3 `crossfade_loop` earlier if ambiences are a near goal.

## Honest limits of this list

- It is a reading of the node structs and `AGENTS.md`, not a test: nothing
  here was rendered. An "awkward" item may be shorter in practice than
  counted, and a P1 model's quality depends on tuning by ear.
- Whether `shape` aliasing is audible at typical drives was not measured;
  the code has no oversampling, which is the only thing checked.
- Priority assumes game SFX across genres. A project that is all foley
  (no synth, no weapons) would put P1 models and modal shapes first.
