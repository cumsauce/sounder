# Routing: which part, template or node for which brief

The full lookup behind AGENTS.md "Which ready-made node for which sound". Search
first (`catalog {action: "search", text: "<the sound you need>"}`): parts are
named and described by what the listener heard, so a brief whose wording is
not in a table row is still found there. Then read the rows below.

## Parts the listener picked, by use

Start from these. Each was picked best of its box, or rated good blind.

| Need | Parts | Stack |
|---|---|---|
| a door slam | the `door_slam` template; its layers `wood_brush`, `noise_crack`, `wood_panel_body`, `low_thump`, `wood_knock` | the template is the approved slam |
| footsteps | `all_purpose_step`, `concrete_step`, `gravel_step`, `grass_step`, `dirt_step`, `wood_step`, `carpet_step`, `metal_step` (all `step`) | `gravel_crunch` or `grass_swish` (step textures) over `all_purpose_step`; a walk is `step` with an `at` list |
| a pipe, coin or wooden block dropped, bouncing | `impact_pipe_drop`, `impact_coin_drop`, `impact_block_drop` (`impact`) | a crack from the part's `force` over it |
| a loose lid or part rattling, shaken | `impact_lid_rattle` | |
| a sign, panel or sheet hit while the wind pushes it | `impact_sign_wind` | |
| a metal bar (or glass, a saw) bowed: a sustained eerie tone | `impact_bowed_bar` | |
| a box or crate dragged, scraped | `impact_box_scrape` | |
| a struck sheet that bends or wobbles as it rings | `impact_bending_sheet` | |
| objects acting on each other | `rig_ball_on_drum`, `rig_pan_and_lid`, `rig_tin_roof_screw`, `rig_string_on_box` (`rig`) | |
| a wind chime in a breeze | `rig_wind_chime` (knob `breeze`) | one clapper per tube: `rig` is one axis |
| a chair knocked over | `rig_chair_fall` (knob `weight`) | |
| a hand bell shaken | `rig_hand_bell` (knob `shake`) | |
| stone and metal hits | `brick_or_rock_drop`, `small_brick_drop`, `heavy_stone_hit` (a wall); `large_pipe_drop`, `large_metal_container_hit`, `metal_plate_gong_hit`, `metal_bell_hit`, `metal_into_small_glass` | `wooden_crate_drop` is a whole crate on concrete |
| clicks and taps | `small_object_taps`, `mouse_click_tap`, `keyboard_button_hit` | |
| creatures | `creature_roar`, `creature_growl`, `creature_snarl`, `creature_whine`, `small_frog` | `creature_snarl_breath` under any voice, `creature_roar_voice` for the voice alone |
| rain and water | `rain_on_stone`; `splash_close`, the 5-30 cm splashes, a deep bubble pop | `rain_drops_on_grass` over `rain_hiss` |
| beds and loops | `surf_shingle`, `lava_bubbling`, `wind_in_trees`, `underwater_bed`, `cave_drips`; fire, storm wind, fan, sparks, crickets | |
| machines and alarms | `small_motor_spin`, engine rev, siren, alarm, error beep, laser | |
| others | rattlesnake rattle and scissors snip (cicada patches heard so), door creak, bird, breath, bells, flute, plucked string, twang | |

**Not in the library** (every attempt was heard as something else, or set aside): a ball or marble rolling (`impact` `slide` over `roughness`: four rounds heard as crackle or crunch, set aside 2026-11-19), footsteps in snow or on ice (`step` has both; two rebuilds each were heard bad), a stream (heard as a gurgling tube), and a large splash heard from a distance. Build those from nodes and say they are unheard.

**From the round 6 listen (testers' sounds, judged by ear):**
- **Door slam:** `new_sound` with `template: "door_slam"` (the recipe in AGENTS.md "Layering a designed sound", approved by ear, ready to tune); keep
  its weighty low layer (band-limited noise under 200 Hz) under the detail:
  detail without it reads light, weight without detail reads like a drum. A
  brief that names a latch adds a `click` (`noise` about 0.75) as the after
  layer; the slam is still the layered recipe, not a click over one `modal`.
- **Footstep:** a step is not an impact. Use `step` or a step part (table
  above), not `footstep` (heard as a snare on concrete, a pitter-patter on
  gravel); check with `layers` `{"preset": "footstep"}`.
- **Knock:** the approved pattern: a `pattern` into `voices` of a `contact`
  into a wood `modal` (`noise` 0.6, `noise_width` about 0.15, `tilt` 4,
  `loss` about 3), plus a short crack and a noise thud per knock, through
  `finish`. Pitch it slightly low (lowest mode near 200 Hz).
- **Metal (pipe, bar, lid, bin, tool, coin):** `modal` metal struck by a
  short noise burst with `noise` 0.4, `noise_width` 0.1 and a **low `loss`
  (0.3-0.6) so it rings for seconds**; plus a crack and a thud per bounce.
  All five metal objects built this way were heard as metal (a pipe as
  "large pipe drop"). The same objects as clean sine modes, even with
  split, beating pairs, were heard as "digital bell" / "fm bell"; the
  earlier short rings (0.1-0.4 s) as glass and ceramic. Start a dropped
  pipe low: lowest mode 120-200 Hz (`f0`), not 300+ (a very small pipe).
- **Stone (a rock, a brick):** brown-noise weight low-passed at 200-230 Hz
  (0.15-0.22 s), crackle-noise grit band-passed at 2-2.5 kHz, band-passed
  noise at 700-800 Hz for the body, a white-noise crack, and a quiet
  `modal` stone body (f0 700-900, loss 2-2.5, 6-8 dB under the noise),
  then a few chips or a small bounce. Heard as "brick dropped or medium
  rock hit". A louder, longer-ringing stone body (loss 1.2) reads as a
  woodblock; small bright dead hits read as mouse clicks; a boulder with
  long low weight reads as a wall.

**When one node is still right:** ambience and loops (`fire`, `wind`, `water`),
UI blips, a single texture, or a brief that names one ready-made sound
(`step`, `explosion`): use it as the body layer and add layers only if the
brief asks for more weight or detail. When a project has reference recordings
(`refs/`), tune the layer levels against them (`verify` `expect reference`):
the distance is a coarse gate (about 10 dB is the spread between real
recordings of one event), not a judge.

The heavy wooden door slam (`slam_b2` in Phase 0) is the `door_slam` template:
`new_sound {template: "door_slam"}` or `catalog describe` it for the graph
(brush `scr_g`, crack `crk_g`, noise-struck wood `panel`, band-passed noise
body `body_g`, brown-noise weight `thd_g`, knock `knock_g`, then `transient`
3.1 dB, `shape` tube mix 0.25, `onepole` hp 30, `eq` peak 200 Hz +5.6 dB,
`output` lufs -28).

A sword clash (two inharmonic `modal` bars) and a UI confirm (tonal on purpose,
an echo instead of a reverb) are worked recipes in `docs/nodes/README.md`,
"Worked recipes".

`examples/door_slam.rs` renders the door slam and checks it.

## Ready-made nodes by brief

Round-1 testers built every footstep from primitives and never tried `fire` or
`footstep`: the nodes below exist so that a brief that names one of these
sounds starts from one node (the body layer) plus a few knobs. **Building from primitives is still
fine** (and right for anything not listed: a laser, a UI blip, a whoosh, a
drone); `describe_op {kind}` gives each node's fields and an example, and the
node docs (`docs/nodes/`) give the measured limits. None of these has been heard (the ports from `stream` to `two_tone` were rated once, blind: their rows say how they were heard): pick by
the brief, then `listen`, `verify` and let the human's macros do the rest.

| Brief | Node | Start with | Fits |
|---|---|---|---|
| **a door slam** | `new_sound` `template: "door_slam"` | knobs `weight`, `snap`, `stick`, `knock`, `latch` (a click latch, off at 0) | the layered slam the listener approved (brush, crack, noise-struck wood body, noise weight, knock, dry bus), copied whole and editable. Two rounds of testers given only the recipe built a click over one `modal` instead, heard as weak: **start from the template**, then tune the knobs |
| a footstep, a walk (older model) | `footstep` | `surface`, `weight`, `pace`, `shoe` | heard as a snare on concrete, a pitter-patter on gravel, a blow on grass: use `step` (next row); kept for its wood steps (`ring` 0.6, `weight` 0.25, `noise` 0.35) |
| footsteps on concrete, gravel, grass, dirt, snow, carpet (foley's model) | `step` | `surface`, `speed` m/s (1 a walk), `grit` 0..2, `layer` all / body / texture (they add back exactly), `at` a list for a walk | a Hertz heel strike on the surface's modes, scuff, ball of the foot; grains, swish, snow crackle. Heard: gravel good ("really good footsteps"; `grit` 0 is the all-purpose step), concrete picked (`concrete_step`, `all_purpose_step` are parts); grass on soil with the crunch at the heel picked ("a bit heavy"; `grass_step` lightens it with a high-pass) |
| fire, a campfire, crackle | `fire` | `size`, `intensity`, `crackle`, `loop` | roar + hiss + Poisson pops; loops by itself in a `kind: loop` document |
| wind, a gale, howling | `wind` | `speed` (signal), `gust`, `whistle`, `loop` | five moving bands (+ an Aeolian tone); a `ramp` on `speed` is a rising gale |
| electric arcs, sparks | `spark` | `rate` (snaps/s), `buzz`, `body` | a beating transformer hum, gated arcing chirps and formant snaps (Farnell) |
| a stream, a brook, rain on water | `water` | `flow` (signal), `density`, `splash`, `depth` | a Minnaert bubble population plus hiss; a loop in a loop document |
| a splash, something dropped in water | `splash` | `size` (m) | cavity bubbles and a pour with no noise burst, droplets falling back, then bubbles whose pitch rises (after Farnell's poured liquid and rain drops; rated good by ear at 5-20 cm, bad at 60 cm); `impact: "noise"` is the earlier opening, heard as a snare; a transient: use `peak_db` |
| a single drip, a blip of water | `bubble` | `radius_mm`, `jitter`; in `voices` `map` a `$rand` radius | one Minnaert bubble: drips, water_drips example |
| cloth, foliage rustle | `cloth` | `motion` (signal), `weight` | power-law rustle bursts in four bands; shape `motion` with a `wander` or `ramp` |
| crumpled paper, foil, plastic, dry leaves | `crumple` | `density` (signal, bursts/s up to 400), `size`, `hardness` | band-passed noise bursts whose count follows `density` (cloth's saturates); hardness 0.2 paper, 0.85 foil; `peak_db` for a sparse one |
| rolling, scraping, sliding | `rolling`, `friction` | `speed` (signal), `roughness` | an excitation: put it through `modal` (a scale first: `rolling` is loud at high speed) |
| breaking glass, shards, debris | `shatter` into `voices` | `pieces`, `spread`, `exponent`; a small `modal` per fragment | a crack then power-law fragments; `modal` glass with `loss` and `tilt` per shard |
| many objects falling at once: a wall toppling, a rockfall, a pile of crates | `collapse` into `debris`, heroes from `collapse.hero` through `voices` of a `rig` | `rows` `cols` `mode` `looseness` `pile`; `debris` `f0` `banks` `bed_db` | not heard yet; `docs/nodes/collapse.md` |
| hard hits that break things, and the grit left hanging | `fracture_on` (events over a threshold -> chip events for `voices`/`debris`) plus `dust` (material-tinted grit following the hits' energy) | `threshold` `sharpness` `chips`; `dust` `material` `settle` | not heard yet; `docs/nodes/breakage.md` |
| explosion, gunshot, blast | `explosion`, `gunshot` | `size`, `distance`; `caliber`, `barrel`, `suppressed` | finished one-shots, start 2.5 ms in (the default fade-in does not cut them); distance delays and darkens |
| engine, motor, vehicle | `engine`, `motor` | `rpm` (a `ramp` or `wander`), `cylinders` / `poles`, `load` | rpm-driven pitch; set a `duration` (ramps hold) |
| a whoosh, a swing, a sword or stick through the air | the part `whoosh_sweep` (`catalog search` `text: "whoosh"`, heard as "whoosh") | `at`, `gain_db`, its knobs | noise band-passed wide (q about 1) and swept up then down over 0.3-0.5 s. Narrow bands (q 2.5-6) were heard as "an artificial sweep, filtered noise": keep the band wide |
| a pipe, a flute, a string, a bow | `tube`, `string` | `length` / `freq`, `excite`, `pressure` | waveguides: winds, plucks, strikes, bowed notes |
| a latch, a switch, a button, a lock, a buckle | `click` | `kind` latch / switch / button / lock / clasp, `size`, `hardness`, `noise` about 0.75 for a small plastic or metal part (the listener's setting; the default rings pitched, voiced 0.33-0.78) | one or two transients through small resonators; a hit at t = 0: `fade_in` 0.0005 |
| a bell, a bouncing ball, crickets, a ruler twang, a siren, a creaking door, a jet, a sci-fi klaxon, a stream or a pour, a helicopter or a rocket, a fly or cicadas, surf, lava or a cave, frogs, leaves or underwater, a droid / transporter / torpedo / beam, phone tones, thunder | one node each: `docs/nodes/README.md`, "More ready-made nodes by brief" | its row | Farnell ports and foley beds, each heard once |
| rain, a shower, a downpour, rain on a roof | `rain` | `surface` water / stone / leaf / wood / soft / glass / metal, `rate` mm/h (2 light, 10, 50 a downpour), `roof` + `roof_height`, `loop`, `layer` all / drops / hiss (they add back) | physically sized drops over a surface-coloured hiss (after the foley project); levels differ by 20 dB per surface: set `lufs`. Heard: soft and wood as rain (good), leaf and stone as a stream (good), glass and metal as chimes, water as white noise |
| an animal roar, growl, snarl, whine | `creature` | `call`, `size` 0..1 (tract length and pitch), `aggression`, `roughness`, `length`, `layer` all / voice / breath (they add back) | foley's LF voice through a waveguide tract; every call picked (the creature parts) |
| a struck object: panel, bell, pipe, keys, a door's thud | `modal` fed by `contact`, `impulse` or a noise burst | `shape` plate / bar / tube / membrane / string / shell, `material`, `size`, `thickness`, `loss`, `tilt`; **`noise` for real wood (0.6, `tilt` 4) and metal (pipe, bar, lid, tool, coin: 0.4, `noise_width` 0.1, `loss` 0.3-0.6; 0.85 with `noise_width` 0.5 reads as a thin sheet)** (clean modes read as a tuned instrument) | normalized; `size` and `thickness` are pitch knobs (small thin objects: `thickness` 0.0015 instead of a custom material); `loss` 0.5 rings twice as long; `tilt` flattens a one-mode plate |
| how a hit plays out over time: a dropped pipe / coin / block, a loose lid rattling, a sign in the wind, a bowed bar, a box dragged, a struck sheet that bends | `impact`; **start from its part** (`sound` node or `catalog load`): `impact_pipe_drop`, `impact_coin_drop`, `impact_block_drop`, `impact_lid_rattle`, `impact_sign_wind`, `impact_bowed_bar`, `impact_box_scrape`, `impact_bending_sheet` | `height` or `speed`, `object_mass`, `striker_mass` (left out: a drop onto `floor`), `bounce`, `scatter`, and `modal`'s shape / material / `f0` / `loss` / `noise` | a striker (or the floor) and the modes stepped together (Hunt-Crossley). Picked blind over the approved builds (2026-11-19). The body layer: a crack from its `force` port (white noise x `hit.force`, hp 2 kHz). Lost by ear: rolling, creaks (use `door_creak`), a pitch bend on a low sheet. Single hits stay on the parts / template / `contact` or noise -> `modal`; hand-timed rhythms on `pattern` / `voices`. Peaks at -6 dBFS |
| several objects acting on each other: a ball on a drum, a pan with its lid, a tin roof with a loose screw, a plucked or struck string on a wooden body | `rig`; **start from its part**: `rig_ball_on_drum`, `rig_pan_and_lid`, `rig_tin_roof_screw`, `rig_string_on_box` (picked blind 2026-11-19/20) | `bodies` (point masses, modal objects, `string` / `tube` waveguides) and `joints` (`contact`, `spring` with a `gap` for a rattle); a rattling part needs `noise` about 0.7; leave the `air` joint out (it lost both listens) | `impact` generalised, vertical only; see `docs/nodes/rig.md` |
| a larger-than-life hit (cinematic, trailer, a game's heavy impact) | `subdrop` under it, `riser` into it | `from` / `to` / `decay`, `drive`; `at` (the hit), `length`, `shape` | tonal on purpose: add them to a layered hit only when the brief asks for exaggeration (a realistic slam with a sine sub read as a tuned drum) |
| a robot voice, speech-like texture, band dynamics | `vocoder`, `multiband`, `filterbank` | a pitched `carrier` + a noisy `modulator`; `crossovers` + `bands` | see "Bands and vocoder" in `docs/nodes/README.md` |
| a swarm, a swell, a granular wash | `cloud`, `grains` | `source`, `density` (signal), `size`, `pitch_spread` | grains from any node or WAV |
| a vowel, a creature call | `glottis` into `tract` | `f0` (signal), `tension`, `vowel`, `scale` | `docs/nodes/README.md`, "Foley models" |

Scene sounds layer these: a door slam is the layered recipe in "Layering a
designed sound" (not a `click` latch under a `modal` thud: that read as metal
and as a tuned drum), a campfire is `fire` plus a few `spark`s, rain is the `rain` node.
One `sound` per layer and a `mix` with `gain` / `at` keeps each one editable.

Layer levels: the ready-made nodes are not normalized and sit at different
levels (a default `fire` about -30 LUFS, a `footstep` peaks near -12 dBFS, a
`click` press at -6 dBFS), so balance a scene with a `gain` node (`db`) per
layer **before** the `mix`, then one `peak_db` / `lufs` on the output. For
example `fire` -> `gain` db 0 and `click` -> `gain` db -8 and `footstep` ->
`gain` db -4, each a `sound` or a node, summed by one `mix`. A macro maps the
`gain` node's `db` (or a `mix` input's `gain`), so "fire level" is one knob
per layer. `contact` and `modal` take their own `gain_db` (-60..60, 0 =
unchanged), so a struck layer's level needs no extra node.

