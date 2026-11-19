"""Batch 10: `cave` rebuilt (pool drips in a long echo) and `frog` given
per-call pitch, falling notes and rasping pulses, after batch 9 heard both bad. Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch9.json is the key.

  python3 tools/labels/gen9.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261123)

# (design name, node fields, duration s, output target)
DESIGNS = [
    ("cave_default", {"type": "cave"}, 10, {"peak_db": -3}),
    ("cave_shallow_wet", {"type": "cave", "drips": 1, "depth": 0.2}, 8, {"peak_db": -3}),
    ("cave_deep_sparse", {"type": "cave", "drips": 0.2, "depth": 1}, 12, {"peak_db": -3}),
    ("frog_small", {"type": "frog", "size": 3, "rate": 1}, 6, {"lufs": -24}),
    ("frog_mid", {"type": "frog", "size": 7, "rate": 0.7}, 6, {"lufs": -24}),
    ("frog_bull", {"type": "frog", "size": 15, "rate": 0.4}, 8, {"lufs": -24}),
    ("frog_chorus", {"type": "frog", "size": 6, "rate": 0.6, "chorus": 6}, 10, {"lufs": -24}),
]


def main(project):
    cues = []
    for name, node, dur, target in DESIGNS:
        for dress in (False, True):
            ops = [dict(node, name="src")]
            last = "src"
            if dress:
                ops.append({"type": "reverb", "name": "room", "input": "src", "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
                last = "room"
            ops.append(dict({"type": "output", "name": "out", "input": last, "fade_in": 0.002}, **target))
            cues.append({"design": name, "dressed": dress, "ops": ops, "duration": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"s{i + 1:02d}"
        args = {"sound": sid, "ops": c["ops"], "duration": c["duration"], "seed": 1}
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch10"}}]
        batch.append({"id": sid, "family": "foley", "group": c["ops"][0]["type"], "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls10.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch10.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
