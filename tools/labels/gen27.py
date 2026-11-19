"""Batch 27: `rig`, several bodies stepped together. A hammer on a tin
sheet with a loose screw (0.2 mm of free play) or a tight one; a pan struck
with a spoon while a light or heavy lid rests on it; a ball dropped on a
drum skin, hard or soft. Each take dry and dressed.

  python3 tools/labels/gen27.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261124)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def rig(bodies, joints, **kw):
    return dict({"type": "rig", "name": "hit", "bodies": bodies, "joints": joints, "at": 0.005}, **kw)


def roof(gap):
    return rig([
        {"name": "hammer", "mass": 0.4, "velocity": -3.0},
        {"name": "sheet", "mass": 2.0, "fixed": True, "material": "metal", "size": 0.5,
         "thickness": 0.002, "loss": 0.4, "noise": 0.3, "noise_width": 0.1},
        {"name": "screw", "mass": 0.005, "material": "metal", "shape": "bar", "f0": 2500,
         "loss": 0.6, "level_db": -6}], [
        {"a": "hammer", "b": "sheet", "at_b": 0.4, "hardness": 0.9},
        {"kind": "spring", "a": "screw", "b": "sheet", "at_b": 0.8, "gap": gap,
         "stiffness": 2e5, "damping": 2}], gravity=0)


def pan(lid_mass):
    return rig([
        {"name": "spoon", "mass": 0.05, "velocity": -2.5},
        {"name": "pan", "mass": 1.5, "fixed": True, "material": "metal", "size": 0.25,
         "thickness": 0.002, "loss": 0.5, "noise": 0.4, "noise_width": 0.1},
        {"name": "lid", "mass": lid_mass, "material": "metal", "size": 0.24,
         "thickness": 0.001, "loss": 0.5, "noise": 0.4, "noise_width": 0.1, "level_db": -3}], [
        {"a": "spoon", "b": "pan", "at_b": 0.3, "hardness": 0.9, "gap": 0.002},
        {"a": "lid", "b": "pan", "at_a": 0.95, "at_b": 0.9, "bounce": 0.3}])


def drum(hard):
    return rig([
        {"name": "ball", "mass": 0.06},
        {"name": "skin", "mass": 0.3, "fixed": True, "material": "wood", "shape": "membrane",
         "f0": 110, "loss": 2.0, "noise": 0.3}], [
        {"a": "ball", "b": "skin", "at_b": 0.3, "gap": 0.3, "bounce": 0.6, "hardness": hard}])


DESIGNS = [
    ("tin_roof", "roof_loose_screw", [roof(0.0002), out("hit")], 2.0),
    ("tin_roof", "roof_tight_screw", [roof(0.0), out("hit")], 2.0),
    ("pan_lid", "pan_light_lid", [pan(0.2), out("hit")], 2.0),
    ("pan_lid", "pan_heavy_lid", [pan(0.8), out("hit")], 2.0),
    ("ball_drum", "drum_hard_ball", [drum(0.8), out("hit")], 2.5),
    ("ball_drum", "drum_soft_ball", [drum(0.2), out("hit")], 2.5),
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
        sid = f"b27_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch27"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls27.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch27.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
