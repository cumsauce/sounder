"""Batch 22: the coupled `impact` node against the approved builds. Three
objects (a metal pipe dropped, a coin dropped, a wooden block dropped), each
as `impact` alone, `impact` with a noise crack driven by its `force` port, and
the current best build (an approved part, or a bounce -> voices coin). Every
take renders dry and dressed (a small room before the output).

  python3 tools/labels/gen22.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261119)

PIPE = {"type": "impact", "name": "hit", "material": "metal", "shape": "tube", "f0": 160,
        "loss": 0.4, "noise": 0.4, "noise_width": 0.1, "object_mass": 2.0, "height": 0.4,
        "bounce": 0.35, "scatter": 0.4, "floor": "stone"}
COIN = {"type": "impact", "name": "hit", "material": "metal", "shape": "plate", "size": 0.025,
        "thickness": 0.0015, "loss": 0.5, "noise": 0.4, "noise_width": 0.1,
        "object_mass": 0.007, "height": 0.3, "bounce": 0.6, "scatter": 0.4, "floor": "stone"}
BLOCK = {"type": "impact", "name": "hit", "material": "wood", "shape": "bar", "size": 0.3,
         "loss": 2.0, "noise": 0.6, "tilt": 4, "object_mass": 0.8, "height": 0.4,
         "bounce": 0.3, "scatter": 0.4, "floor": "stone"}


def alone(node):
    return [node, {"type": "output", "name": "out", "input": "hit", "fade_in": 0.0005, "peak_db": -3}]


def cracked(node, crack_db, hp):
    return [node,
            {"type": "noise", "name": "wn", "color": "white"},
            {"type": "math", "name": "crk", "op": "mul", "a": "wn", "b": "hit.force"},
            {"type": "filter", "name": "crk_f", "input": "crk", "mode": "hp", "freq": hp},
            {"type": "gain", "name": "crk_g", "input": "crk_f", "db": crack_db},
            {"type": "mix", "name": "dry", "inputs": ["hit", "crk_g"]},
            {"type": "finish", "name": "bus", "input": "dry", "punch": 0.3, "glue": 0.2},
            {"type": "output", "name": "out", "input": "bus", "fade_in": 0.0005, "peak_db": -3}]


def part(name):
    return [{"type": "sound", "name": "src", "sound": name},
            {"type": "output", "name": "out", "input": "src", "fade_in": 0.0005, "peak_db": -3}]


COIN_BUILD = [
    {"type": "bounce", "name": "b", "first": 0.24, "restitution": 0.6, "start": 0.005, "jitter": 0.1},
    {"type": "voices", "name": "src", "events": "b", "length": 1.0, "max": 32, "patch": [
        {"type": "impulse", "name": "tap", "at": [0.0]},
        {"type": "gain", "name": "strike", "input": "tap", "gain": "$value"},
        {"type": "modal", "name": "coin", "excite": "strike", "material": "metal", "size": 0.025,
         "thickness": 0.0015, "loss": 0.5, "noise": 0.4, "noise_width": 0.1}]},
    {"type": "output", "name": "out", "input": "src", "fade_in": 0.0005, "peak_db": -3}]

DESIGNS = [
    ("pipe_drop", "pipe_impact", alone(PIPE), 3.0),
    ("pipe_drop", "pipe_impact_crack", cracked(PIPE, -14, 2500), 3.0),
    ("pipe_drop", "pipe_part", part("large_pipe_drop"), 3.0),
    ("coin_drop", "coin_impact", alone(COIN), 2.0),
    ("coin_drop", "coin_impact_crack", cracked(COIN, -16, 4000), 2.0),
    ("coin_drop", "coin_bounce_voices", COIN_BUILD, 2.0),
    ("block_drop", "block_impact", alone(BLOCK), 1.5),
    ("block_drop", "block_impact_crack", cracked(BLOCK, -10, 2000), 1.5),
    ("block_drop", "block_crate_part", part("wooden_crate_drop"), 1.5),
]


def dress(ops):
    ops = [dict(o) for o in ops]
    out = next(o for o in ops if o["type"] == "output")
    ops.insert(len(ops) - 1, {"type": "reverb", "name": "room_dress", "input": out["input"], "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
    out["input"] = "room_dress"
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
        sid = f"b22_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch22"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls22.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch22.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
