"""Batch 29: rig waveguide bodies and the air joint. A ball on a drum skin
over a shell tube (vs the approved part `rig_ball_on_drum`), a dropped
metal pipe with its air column coupled (vs the approved `impact_pipe_drop`),
and a string struck on a wooden body (vs a plain `string` node). Each take
dry and dressed.

  python3 tools/labels/gen29.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261126)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def rig(bodies, joints, **kw):
    return dict({"type": "rig", "name": "hit", "bodies": bodies, "joints": joints, "at": 0.005}, **kw)


def part(name, **kw):
    return dict({"type": "sound", "name": "hit", "sound": name}, **kw)


def drum(strength):
    return rig([
        {"name": "ball", "mass": 0.06},
        {"name": "skin", "mass": 0.3, "fixed": True, "material": "wood", "shape": "membrane",
         "f0": 110, "loss": 2.0, "noise": 0.3},
        {"name": "shell", "mass": 0.1, "waveguide": {"kind": "tube", "length": 0.4,
         "ends": "closed", "radius": 0.15, "decay": 0.35, "damping": 0.4}}], [
        {"a": "ball", "b": "skin", "at_b": 0.3, "gap": 0.3, "bounce": 0.6, "hardness": 0.2},
        {"kind": "air", "a": "skin", "b": "shell", "at_a": 0.3, "strength": strength}])


def pipe(strength):
    return rig([
        {"name": "pipe", "mass": 2.0, "material": "metal", "shape": "tube", "f0": 160,
         "loss": 1.0, "noise": 0.4, "noise_width": 0.1},
        {"name": "air", "mass": 0.1, "waveguide": {"kind": "tube", "length": 1.0,
         "ends": "open", "radius": 0.02, "decay": 0.6}}], [
        {"a": "pipe", "b": "ground", "gap": 0.4, "bounce": 0.35, "hardness": 0.9},
        {"kind": "air", "a": "pipe", "b": "air", "at_a": 0.3, "strength": strength}])


def guitar(f0, body_db):
    return rig([
        {"name": "pick", "mass": 0.002, "velocity": -1.5},
        {"name": "str", "mass": 0.002, "waveguide": {"f0": f0, "decay": 2.5, "damping": 0.3}},
        {"name": "box", "mass": 0.5, "fixed": True, "material": "wood", "size": 0.35,
         "thickness": 0.003, "loss": 1.5, "noise": 0.6, "tilt": 4, "level_db": body_db}], [
        {"a": "pick", "b": "str", "at_b": 0.15, "hardness": 0.8, "bounce": 0.2},
        {"kind": "spring", "a": "box", "b": "str", "at_a": 0.3, "at_b": 0.98,
         "stiffness": 2e4, "damping": 0.5}], gravity=0)


DESIGNS = [
    ("ball_drum_shell", "shell_air_2", [drum(2.0), out("hit")], 2.0),
    ("ball_drum_shell", "shell_air_6", [drum(6.0), out("hit")], 2.0),
    ("ball_drum_shell", "part_rig_ball_on_drum", [part("rig_ball_on_drum"), out("hit")], 2.0),
    ("pipe_air", "air_3", [pipe(3.0), out("hit")], 2.5),
    ("pipe_air", "air_8", [pipe(8.0), out("hit")], 2.5),
    ("pipe_air", "part_impact_pipe_drop", [part("impact_pipe_drop"), out("hit")], 2.5),
    ("string_body", "rig_string_box", [guitar(196, 0), out("hit")], 2.5),
    ("string_body", "rig_string_box_quiet_body", [guitar(196, -8), out("hit")], 2.5),
    ("string_body", "plain_string_node", [{"type": "string", "name": "hit", "freq": 196,
      "excite": "pluck", "position": 0.15, "damping": 0.3}, out("hit")], 2.5),
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
        sid = f"b29_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch29"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls29.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch29.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
