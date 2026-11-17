# Swarm journal

- 2026-11-17 setup: branch swarm/main from main (f0ea79a), shared target via .cargo/config.toml, baseline tests green, 62 GB free.
- wave 0: launched T0a, T0b (worktrees .swarm/wt/T0a, T0b)
- round 0 (baseline): launching testers 1-12 on binary built from f0ea79a-equivalent
- launched: T0a, T0b (wave 0) + 12 round-0 testers (sounds 1-12); sounds 13-24 pending a free slot
- round 0: all 24 testers launched; 12 done so far (1-12 minus stragglers); wave 0 T0a/T0b still running
- wave 0: T0a merged (fa5bb26 + a919fa7 refs_typed callers); T0b running. round 0: 21/24 testers done; judging started
- round 0 DONE: 24/24 scored, mean judge 9.12, mean difficulty 2.04, mean failed calls 4.5. triage in design/rounds/r0/triage.md. fix tasks F1 F2 F3 S8b added to plan (section 4b)
- launched F1 F2 F3 S8b (round-0 fixes); T0b still running; waiting for T0b to launch T0c then wave 1/2
- merged T0b (eca6c1d), F3 output stage; T0c, F1, F2, S8b running
- launching wave 1: S1 S2 S3 S4
- merged F1 (tools args); launching S5
- FINDING: shared target dir gives false test results (workspace crates collide across worktrees; deps dir vanished). Removed .cargo/config.toml; every worktree now builds in its own target/ (cold ~80s). Told S1-S5.
