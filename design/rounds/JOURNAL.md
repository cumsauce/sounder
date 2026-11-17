# Swarm journal

- 2026-11-17 setup: branch swarm/main from main (f0ea79a), shared target via .cargo/config.toml, baseline tests green, 62 GB free.
- wave 0: launched T0a, T0b (worktrees .swarm/wt/T0a, T0b)
- round 0 (baseline): launching testers 1-12 on binary built from f0ea79a-equivalent
- launched: T0a, T0b (wave 0) + 12 round-0 testers (sounds 1-12); sounds 13-24 pending a free slot
- round 0: all 24 testers launched; 12 done so far (1-12 minus stragglers); wave 0 T0a/T0b still running
- wave 0: T0a merged (fa5bb26 + a919fa7 refs_typed callers); T0b running. round 0: 21/24 testers done; judging started
- round 0 DONE: 24/24 scored, mean judge 9.12, mean difficulty 2.04, mean failed calls 4.5. triage in design/rounds/r0/triage.md. fix tasks F1 F2 F3 S8b added to plan (section 4b)
- launched F1 F2 F3 S8b (round-0 fixes); T0b still running; waiting for T0b to launch T0c then wave 1/2
