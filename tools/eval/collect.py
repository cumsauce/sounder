"""Collect an evaluation run: save each brief's sound document and render
it to WAV for the blind rating page.

  python3 tools/eval/collect.py <autosave session json> <run dir> <wav dir>

The documents come from the eval server's autosave (the agents never save
to the catalog). Each is written to <run dir>/<id>.json, then rendered by a
throwaway server and exported as <wav dir>/<id>.wav. Briefs with no sound
are listed and skipped.
"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


def main(session, run_dir, wav_dir):
    briefs = json.load(open(os.path.join(HERE, "briefs.json")))
    sounds = json.load(open(session))["sounds"]
    os.makedirs(run_dir, exist_ok=True)
    os.makedirs(wav_dir, exist_ok=True)
    calls, missing = [], []
    for b in briefs:
        name = f"{b['id']}_{b['name']}"
        s = sounds.get(name)
        if not s:
            missing.append(name)
            continue
        doc = s["doc"]
        json.dump(doc, open(os.path.join(run_dir, f"{b['id']}.json"), "w"), indent=1)
        args = {k: doc[k] for k in ("kind", "seed") if k in doc}
        if doc.get("duration"):
            args["duration"] = doc["duration"]
        calls += [{"name": "new_sound", "args": {"name": b["id"], **args}},
                  {"name": "sound_ops", "args": {"doc": b["id"], "ops": doc["nodes"], "params": doc.get("params") or {},
                                                 "macros": doc.get("macros") or []}},
                  {"name": "export", "args": {"doc": b["id"], "format": "wav", "path": os.path.abspath(wav_dir)}}]
    cf = os.path.join(os.path.abspath(wav_dir), "collect_calls.json")
    json.dump(calls, open(cf, "w"))
    env = dict(os.environ, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env,
                       capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print("collected", len(briefs) - len(missing), "missing", missing)


if __name__ == "__main__":
    main(*sys.argv[1:4])
