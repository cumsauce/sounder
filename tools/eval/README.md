# Evaluation set

Fixed briefs (`briefs.json`) that a fresh agent builds from AGENTS.md
alone (`agent_prompt.md`, the only thing it is told), rated blind by the
listener. A run is a baseline; after a change to the tool or the docs the
same briefs are built again and the two runs are rated side by side, blind.

A run lives in `runs/<date>/`: each sound's document (`<id>.json`, enough
to re-render it), the agent's report (`reports.json`) and the listener's
ratings (`scores.json`). Audio is rendered from the documents, never
committed.

How a run goes:
1. Start a headless server on the build under test, on its own socket:
   `SOUNDER_BINARY=target/debug/sounder SOUNDER_PROJECT=<scratch project>
   python3 mcp_driver.py --daemon --headless --socket /tmp/sounder-eval.sock`.
2. For each brief, one at a time, a fresh agent gets `agent_prompt.md`
   with `{brief}`, `{sound}` (`<id>_<name>`), `{client}` and `{scratch}`
   filled in. It builds and verifies the sound and replies with a short
   report.
3. `python3 tools/eval/collect.py <scratch project> runs/<date>` saves
   every sound's document and renders a WAV of each into the scratch
   project.
4. The WAVs go on a rating page (shuffled cue ids `r01`..; each card shows
   its brief, since the rating is against the brief); `cues.json` maps cue
   ids to brief ids, and the ratings are saved as `scores.json`.

The daemon's socket path must be short (a Unix socket path is limited to
about 104 bytes): use `/tmp/sounder-eval.sock`, not a scratch folder.

Runs:
- `runs/2026-11-18/`: the baseline, on the build with specs, the metal and
  stone recipes and 89 parts. Rating page: "Sound Brief Ratings".
