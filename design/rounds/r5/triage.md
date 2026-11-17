# Round 5 triage (binary from commit c3c6e98 = swarm/main after R4-A..E)

Core 24: mean judge 9.38 (R4 9.25), difficulty 1.33 (1.38), failed calls 1.25 (1.29), calls 16.8 (17.8). Reserve (arrow_twang, heartbeat_loop, bike_bell, squeaky_hinge): judge 9.25, difficulty 1.5. Round 5 is the last round (swarm plan 9.6), so no fixes follow; no source changed after the binary was built, so this run is also the final check of the core 24.

Only one severity-2 blocker (stream_loop: no claim-able density knob for `water`). Recurring severity-1 themes, all left in the backlog (see REPORT.md): macro claims that pass at seed 1 but fail at another seed (density, spacing under jitter, decay, attack on noise: 9 sounds), `listen` descriptors that mislead on noise or dense textures (density/inharmonicity/modes on wind, paper, arrow: 6), thin seed variation where only one node reads the seed (heartbeat, switch, coin, knock: 5), abrupt ends / trailing silence on fixed-length sounds (4).
