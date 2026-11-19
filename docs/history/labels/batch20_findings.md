# Labels batch 20: tester agents, briefs with no ready-made node

Five Sonnet testers built two takes each, using only AGENTS.md and the tools (`tools/labels/gen20.py`).

| Brief | Pick | Note |
|---|---|---|
| vending machine can | take B (from nodes), in a room (s13) | "motor too loud; I'd expect a bit of a metallic spring / bang as the drink is dispensed. overall good!" |
| ice into a glass | take B (from nodes), in a room (s07) | "good but I'd expect bounce" |
| sci-fi door | take B, dry (s08) | "motor stops a bit before the bang, when I'd expect it to continue and slow down / stop" |
| zipper | take B (resonated teeth), dry (s14) | "good but too low pitch" |
| sword draw | all bad | "we don't have a blade humming node; maybe one to research since it's such a known sound" |

The testers' friction, fixed after this round:
1. Each `mcp_driver.py --calls` run was a fresh session.
2. `pattern` had no accelerating mode.
3. Macro claims failed over noisy or multi-hit sounds.
4. `density` misread (176/s for 3 hits).
5. Auto length ran 7.4 s on a ringing metal clunk.
6. Parts were hard to find by search (the motor).
