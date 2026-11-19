"""Batch 7: the roar with `wobble` (batch 6 heard it as a cow, too clean a
pitch glide) and rain with its `wash` (batch 6: no surface read as rain). Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch7.json is the key.

  python3 tools/labels/gen7.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261120)

# (design name, node fields, duration s, output target)
DESIGNS = [
    ("roar_w0", {"type": "roar", "wobble": 0}, 1.6, {"peak_db": -3}),
    ("roar_w06", {"type": "roar"}, 1.6, {"peak_db": -3}),
    ("roar_w1", {"type": "roar", "wobble": 1}, 1.6, {"peak_db": -3}),
    ("roar_big_w06", {"type": "roar", "size": 2, "aggression": 0.8, "length": 1.5}, 2.2, {"peak_db": -3}),
    ("roar_big_w1", {"type": "roar", "size": 2.5, "aggression": 1, "length": 1.8, "wobble": 1}, 2.5, {"peak_db": -3}),
    ("rain_leaf_wash", {"type": "rain", "surface": "leaf", "intensity": 30}, 4, {"lufs": -24}),
    ("rain_leaf_wash1", {"type": "rain", "surface": "leaf", "intensity": 60, "wash": 1}, 4, {"lufs": -24}),
    ("rain_water_wash", {"type": "rain", "surface": "water", "intensity": 8}, 4, {"lufs": -24}),
    ("rain_window_wash", {"type": "rain", "surface": "window", "intensity": 20}, 4, {"lufs": -24}),
    ("rain_ground_wash", {"type": "rain", "surface": "ground", "intensity": 60}, 4, {"lufs": -24}),
    ("rain_ground_wash1", {"type": "rain", "surface": "ground", "intensity": 120, "wash": 1}, 4, {"lufs": -24}),
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
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch7"}}]
        batch.append({"id": sid, "family": "farnell_fix", "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls7.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch7.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
