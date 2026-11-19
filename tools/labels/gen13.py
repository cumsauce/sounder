"""Batch 13: `step` after batch 12: a damped toe on concrete, gravel with
little or no grit (an all-purpose step), grass rebuilt on the gravel body. A design is a list
of nodes whose last is the source; group is the box on the grouped page. Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch9.json is the key.

  python3 tools/labels/gen9.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261126)


def two(surf, speed, grit):
    return [{"type": "step", "name": "src", "surface": surf, "speed": speed, "grit": grit, "at": [0.005, 0.6], "seed": 1}]

DESIGNS = [
    ("concrete_1_grit1", "footstep_concrete", two("concrete", 1.0, 1.0), 1.4, {"peak_db": -3}),
    ("concrete_1_grit0.5", "footstep_concrete", two("concrete", 1.0, 0.5), 1.4, {"peak_db": -3}),
    ("concrete_1.4_grit0.5", "footstep_concrete", two("concrete", 1.4, 0.5), 1.4, {"peak_db": -3}),
    ("gravel_1.6_grit0", "footstep_allpurpose", two("gravel", 1.6, 0.0), 1.4, {"peak_db": -3}),
    ("gravel_1.6_grit0.25", "footstep_allpurpose", two("gravel", 1.6, 0.25), 1.4, {"peak_db": -3}),
    ("gravel_1_grit0", "footstep_allpurpose", two("gravel", 1.0, 0.0), 1.4, {"peak_db": -3}),
    ("grass_0.8", "footstep_grass", two("grass", 0.8, 1.0), 1.4, {"peak_db": -3}),
    ("grass_1.2", "footstep_grass", two("grass", 1.2, 1.0), 1.4, {"peak_db": -3}),
    ("grass_1.6", "footstep_grass", two("grass", 1.6, 1.0), 1.4, {"peak_db": -3}),
]


def main(project):
    cues = []
    for name, group, nodes, dur, target in DESIGNS:
        for dress in (False, True):
            ops = [dict(n) for n in nodes]
            last = "src"
            if dress:
                ops.append({"type": "reverb", "name": "room", "input": "src", "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
                last = "room"
            ops.append(dict({"type": "output", "name": "out", "input": last, "fade_in": 0.002}, **target))
            cues.append({"group": group, "design": name, "dressed": dress, "ops": ops, "duration": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"s{i + 1:02d}"
        args = {"sound": sid, "ops": c["ops"], "duration": c["duration"], "seed": 1}
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch13"}}]
        batch.append({"id": sid, "family": "gaps", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls13.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch13.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
