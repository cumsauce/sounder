"""Batch 26: the roll after the research. The crunch was the marble
hopping (88 landings in 2 s at roughness 2e-5): these rolls keep it on the
board (roughness 3e-6 to 8e-6), use the rolling filter (the ball bridges
dips narrower than itself), clean modes (noise bands pushed the energy over
2 kHz), and a slightly out-of-round ball (`wobble`, a pulse once a turn).
The batch 24 roll (b24_13) rides along as a reference.

  python3 tools/labels/gen26.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261123)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def roll(slide, rough, mass=0.006, radius=0.008, wobble=3e-6, material="wood", size=0.6, loss=1.5):
    return {"type": "impact", "name": "hit", "material": material, "size": size,
            "loss": loss, "object_mass": 2.0, "striker_mass": mass, "hardness": 1.0,
            "drive": "=" + str(mass * 9.81), "radius": radius, "slide": slide, "roughness": rough,
            "grain": 0.003, "wobble": wobble, "at": 0.005}


SPEED = {"type": "wander", "name": "sp", "rate": 1.0, "range": [0.3, 0.55]}
DESIGNS = [
    ("marble_roll", "marble_smooth", [SPEED, roll("sp", 3e-6), out("hit")], 3.0),
    ("marble_roll", "marble_grainy", [SPEED, roll("sp", 8e-6), out("hit")], 3.0),
    ("marble_roll", "steel_ball", [SPEED, roll("sp", 4e-6, mass=0.03, radius=0.01), out("hit")], 3.0),
    ("marble_roll", "marble_slowing", [
        {"type": "ramp", "name": "sp", "from": 0.7, "to": 0.1, "time": 2.8}, roll("sp", 4e-6), out("hit")], 3.0),
]


def dress(ops):
    ops = [dict(o) for o in ops]
    o = next(o for o in ops if o["type"] == "output")
    ops.insert(len(ops) - 1, {"type": "reverb", "name": "room_dress", "input": o["input"], "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
    o["input"] = "room_dress"
    return ops


def main(project):
    cues = []
    for group, design, ops, dur in DESIGNS:
        for dressed in (False, True):
            cues.append({"group": group, "design": design, "dressed": dressed,
                         "args": {"ops": dress(ops) if dressed else ops, "seed": 1, "duration": dur}})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"b26_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch26"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls26.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch26.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
