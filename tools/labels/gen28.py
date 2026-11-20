"""Batch 28: the tin roof's loose screw again. In batch 27 its rattle was
"good detail but a bit strange": the screw was a clean 2.5 kHz bar mode.
Here it rings as noise bands and sits lower, in two versions, with the
tight screw as the reference. Each take dry and dressed.

  python3 tools/labels/gen28.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261125)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def rig(bodies, joints, **kw):
    return dict({"type": "rig", "name": "hit", "bodies": bodies, "joints": joints, "at": 0.005}, **kw)


def roof(gap, screw_noise=None, f0=2500, level=-6):
    screw = {"name": "screw", "mass": 0.005, "material": "metal", "shape": "bar", "f0": f0,
             "loss": 0.6, "level_db": level}
    if screw_noise:
        screw.update({"noise": screw_noise, "noise_width": 0.4})
    return rig([
        {"name": "hammer", "mass": 0.4, "velocity": -3.0},
        {"name": "sheet", "mass": 2.0, "fixed": True, "material": "metal", "size": 0.5,
         "thickness": 0.002, "loss": 0.4, "noise": 0.3, "noise_width": 0.1},
        screw], [
        {"a": "hammer", "b": "sheet", "at_b": 0.4, "hardness": 0.9},
        {"kind": "spring", "a": "screw", "b": "sheet", "at_b": 0.8, "gap": gap,
         "stiffness": 2e5, "damping": 2}], gravity=0)


DESIGNS = [
    ("tin_roof", "screw_noisy", [roof(0.0002, 0.7, level=-10), out("hit")], 2.0),
    ("tin_roof", "screw_noisy_low", [roof(0.0002, 0.8, f0=1500, level=-12), out("hit")], 2.0),
    ("tin_roof", "screw_tight_reference", [roof(0.0), out("hit")], 2.0),
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
        sid = f"b28_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch28"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls28.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch28.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
