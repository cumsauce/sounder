"""Batch 25: the roll again, on the fractal surface (`octaves` 6) that
replaced the one-scale bumps heard as crackle in batch 24: a marble on fine
and coarse boards and a heavier steel ball, each dry and dressed. The page
also carries the batch 24 fine roll (b24_13, dry) as a reference.

  python3 tools/labels/gen25.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261122)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def roll(slide, rough, grain, mass=0.006, radius=0.008, press=0.06):
    return {"type": "impact", "name": "hit", "material": "wood", "size": 0.6, "noise": 0.6, "tilt": 4,
            "loss": 1.5, "object_mass": 2.0, "striker_mass": mass, "hardness": 1.0, "drive": press,
            "radius": radius, "slide": slide, "roughness": rough, "grain": grain, "at": 0.005}


DESIGNS = [
    ("marble_roll", "roll_fractal_fine", [
        {"type": "wander", "name": "sp", "rate": 1.5, "range": [0.3, 0.6]}, roll("sp", 2e-5, 0.003), out("hit")], 3.0),
    ("marble_roll", "roll_fractal_coarse", [
        {"type": "ramp", "name": "sp", "from": 0.7, "to": 0.15, "time": 2.8}, roll("sp", 6e-5, 0.006), out("hit")], 3.0),
    ("marble_roll", "roll_steel_ball", [
        {"type": "wander", "name": "sp", "rate": 1.0, "range": [0.25, 0.5]},
        roll("sp", 2e-5, 0.004, mass=0.03, radius=0.01, press=0.3), out("hit")], 3.0),
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
        sid = f"b25_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch25"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls25.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch25.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
