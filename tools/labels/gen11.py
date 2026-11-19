"""Batch 11: the library's gaps: footsteps on concrete, gravel and grass
(two steps each), a large splash and an electric motor. A design is a list
of nodes whose last is the source; group is the box on the grouped page. Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch9.json is the key.

  python3 tools/labels/gen9.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261124)


def walk(surface, **kw):
    a = dict({"type": "footstep", "name": "st1", "surface": surface, "at": 0.005, "seed": 1}, **kw)
    b = dict(a, name="st2", at=0.6, seed=2)
    return [a, b, {"type": "mix", "name": "src", "inputs": ["st1", "st2"], "to": "mono"}]

def spin(**kw):
    return [{"type": "env", "name": "spd", "points": [[0, 0], [0.8, 1], [2.4, 1], [3, 0]], "curve": "smooth"},
            {"type": "math", "name": "rpm", "op": "mul", "a": "spd", "b": kw.pop("top", 6000)},
            dict({"type": "motor", "name": "src", "rpm": "rpm"}, **kw)]

DESIGNS = []
for surf in ("concrete", "gravel", "grass"):
    DESIGNS += [
        (f"footstep_{surf}_sneaker", "footstep_" + surf, walk(surf, weight=0.3), 1.4, {"peak_db": -3}),
        (f"footstep_{surf}_hard", "footstep_" + surf, walk(surf, weight=0.35, shoe="hard"), 1.4, {"peak_db": -3}),
        (f"footstep_{surf}_boot", "footstep_" + surf, walk(surf, weight=0.3, shoe="boot", swish=0.6), 1.4, {"peak_db": -3}),
    ]
thump = [{"type": "noise", "name": "tn", "color": "brown"}, {"type": "decay", "name": "te", "time": 0.45, "at": 0.02},
         {"type": "math", "name": "tr", "op": "mul", "a": "tn", "b": "te"},
         {"type": "filter", "name": "tl", "input": "tr", "mode": "lp", "freq": 160, "slope": 24},
         {"type": "gain", "name": "tg", "input": "tl", "db": 6}]
DESIGNS += [
    ("splash_0.5", "splash_large", [{"type": "splash", "name": "src", "size": 0.5, "at": 0.02, "tail": 1.5}], 3, {"peak_db": -3}),
    ("splash_0.6_soft_burst", "splash_large", [{"type": "splash", "name": "src", "size": 0.6, "at": 0.02, "burst": 0.5}], 3, {"peak_db": -3}),
    ("splash_0.8_long", "splash_large", [{"type": "splash", "name": "src", "size": 0.8, "at": 0.02, "tail": 2}], 3.5, {"peak_db": -3}),
    ("splash_0.4_thump", "splash_large", [{"type": "splash", "name": "sp", "size": 0.4, "at": 0.02, "tail": 1.5}] + thump + [{"type": "mix", "name": "src", "inputs": ["sp", "tg"], "to": "mono"}], 3, {"peak_db": -3}),
    ("motor_plain", "motor", spin(poles=4, whine=0.2, load=0.3), 3, {"lufs": -22}),
    ("motor_drill", "motor", spin(gear_teeth=18, load=0.5, whine=0.4), 3, {"lufs": -22}),
    ("motor_toy", "motor", spin(top=12000, poles=2, whine=0.05, load=0.2), 3, {"lufs": -22}),
    ("motor_fan_geared", "motor", spin(top=3000, poles=8, gear_teeth=30, load=0.4, whine=0.15), 3, {"lufs": -22}),
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
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch11"}}]
        batch.append({"id": sid, "family": "gaps", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls11.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch11.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
