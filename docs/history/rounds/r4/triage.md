# Round 4 triage (binary from commit 639a8ef)

Core 24 sounds, rounds 0 / 1 / 2 / 3 / 4: mean judge 9.12 / 9.12 / 9.50 / 9.25 / 9.25; mean difficulty 2.04 / 1.67 / 1.46 / 1.29 / 1.38; mean failed calls 4.5 / 3.2 / 2.25 / 1.42 / 1.29; mean calls 31.3 / 25.5 / 20.3 / 18.7 / 17.8. Reserve (4 new: book_drop, match_strike, typewriter_key, car_door): judge 9.75, difficulty 1.25.

Stop test (section 9.6) after round 4:
1. qualifying issues (>= 3 sounds, or severity 2) : MORE than 3 (see table) -> NOT met.
2. mean judge rose by less than 0.3 (flat): met.
3. mean difficulty fell by less than 0.2 (rose 0.09): met.
All three must hold, so the fixes below are made and round 5 (the last) runs. Friction is now mostly severity 1 sense gaps; judges sit at the ceiling.

| # | Issue | Sounds | Fix |
|---|---|---|---|
| 1 | no per-hit decay / ring claim: `decay` claim non-monotone on multi-hit sounds (typewriter, knock, coin, footstep_concrete, door_creak, metal_pipe spacing, match density, cloth/grass no attack-vs-layer claim) | 7 | R4-A |
| 2 | `listen` onset table: no per-onset -60 dB decay, pitch columns give octave errors on inharmonic hits and nothing for a short burst; modes line shows "sustained 600 s" partials for short sounds; inharmonicity reading depends on the pitch guess; no readout for sub-20 ms separated events | 9 (book_drop, knock, keys, water_drip, glass_clink, coin, wood_floor, door_creak, switch/typewriter) | R4-B |
| 3 | macro maps cannot reach inside a `voices` patch, an `env` `points` list, `expr` constants, a `contact`/`modal` level (glass_break sev 2, coin, metal_pipe, sword_whoosh, door_slam, splash, match) | 7 | R4-C |
| 4 | `splash` node has no tail / bubble count / burst level: alone it is 0.5 s with the cavity bubble loudest (sev 2); `thunder` has no tail shape; `water` has no tone control | 3 | R4-D |
| 5 | docs: zoom shape (3 sounds again), default fade-in with an event at t = 0 (coin, general), poisson vs pattern for density-claim knobs, full-band vs occupied-band flatness (gravel, rain), which claim suits which knob, `decay` node `time`, density counter plateau about 588 events/s | 8 | docs pass R4-E |

Backlog (not selected, single sound each): `fire` per-layer levels, `footstep` tick/body level and stone count, `click` double gap, `water` brightness (folded into R4-D), `shatter` decaying value law, `bounce` first-impact doc, typewriter click kind, ready-made match/ignition node, claim for raggedness, `bubble.rise` in Hz, `cloth.weight` monotone, thunder length macro vs duration.
