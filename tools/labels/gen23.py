"""Batch 23: `impact`'s one-way inputs. A loose lid shaken (`drive` a sine or
a wander), a marble rolling on a board (a pulsed `drive`), a metal sign in
the wind that is also hit (`excite` noise), a struck steel sheet that bends
(`pitch` a wander or a sag), and the pipe drop with a force-driven crack, with
and without chips from `on_threshold` on `force`. Each take dry and dressed.

  python3 tools/labels/gen23.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261120)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


LID = {"type": "impact", "name": "hit", "material": "metal", "size": 0.25, "thickness": 0.0015,
       "noise": 0.4, "noise_width": 0.1, "loss": 0.5, "object_mass": 0.3, "striker_mass": 0.03,
       "hardness": 0.9, "height": 0.001, "drive": "push", "scatter": 0.5}
ROLL = {"type": "impact", "name": "hit", "material": "wood", "size": 0.6, "noise": 0.6, "tilt": 4,
        "loss": 1.5, "object_mass": 2.0, "striker_mass": 0.005, "hardness": 1.0, "height": 0.002,
        "radius": 0.008, "drive": "push", "scatter": 0.5}
SIGN = {"type": "impact", "name": "hit", "material": "metal", "size": 0.6, "thickness": 0.002,
        "noise": 0.4, "noise_width": 0.1, "loss": 0.3, "object_mass": 3.0, "striker_mass": 0.2,
        "hardness": 0.8, "speed": 2.0, "excite": "wind", "at": 0.005}
SHEET = {"type": "impact", "name": "hit", "material": "metal", "size": 0.8, "thickness": 0.001,
         "noise": 0.3, "noise_width": 0.1, "loss": 0.3, "object_mass": 4.0, "striker_mass": 0.5,
         "hardness": 0.6, "speed": 3.0, "pitch": "bend", "at": 0.005}
PIPE = {"type": "impact", "name": "hit", "material": "metal", "shape": "tube", "f0": 160, "loss": 1.0,
        "noise": 0.4, "noise_width": 0.1, "object_mass": 2.0, "height": 0.4, "bounce": 0.35,
        "scatter": 0.4, "floor": "stone"}
CRACK = [{"type": "noise", "name": "wn", "color": "white"},
         {"type": "math", "name": "crk", "op": "mul", "a": "wn", "b": "hit.force"},
         {"type": "filter", "name": "crk_f", "input": "crk", "mode": "hp", "freq": 2500},
         {"type": "gain", "name": "crk_g", "input": "crk_f", "db": -14}]
CHIPS = [{"type": "on_threshold", "name": "lands", "input": "hit.force", "level": 0.2, "hold": 0.01},
         {"type": "voices", "name": "chips", "events": "lands", "length": 0.2, "max": 64,
          "map": {"f": "1800 + $rand * 1600"}, "patch": [
              {"type": "contact", "name": "c", "speed": 1, "mass": 0.01, "material": "stone"},
              {"type": "modal", "name": "chip", "excite": "c", "material": "stone", "f0": "$f", "noise": 0.6}]},
         {"type": "gain", "name": "chips_g", "input": "chips", "db": 12}]

DESIGNS = [
    ("lid_rattle", "lid_sine", [
        {"type": "osc", "name": "sh", "shape": "sine", "freq": 14},
        {"type": "ramp", "name": "sw", "from": 0.05, "to": 0.7, "time": 1.2, "curve": "linear"},
        {"type": "math", "name": "push", "op": "mul", "a": "sh", "b": "sw"}, LID, out("hit")], 2.5),
    ("lid_rattle", "lid_wander", [
        {"type": "wander", "name": "push", "rate": 25, "range": [-0.5, 0.8]}, LID, out("hit")], 2.5),
    ("marble_roll", "roll_60", [
        {"type": "wander", "name": "push", "rate": 60, "range": [-0.08, 0.1]}, ROLL, out("hit")], 2.5),
    ("marble_roll", "roll_120", [
        {"type": "wander", "name": "push", "rate": 120, "range": [-0.06, 0.08]}, ROLL, out("hit")], 2.5),
    ("sign_wind", "sign_breeze", [
        {"type": "noise", "name": "pn", "color": "pink"},
        {"type": "wander", "name": "gust", "rate": 0.8, "range": [0.0002, 0.0012]},
        {"type": "math", "name": "wind", "op": "mul", "a": "pn", "b": "gust"}, SIGN, out("hit")], 4.0),
    ("sign_wind", "sign_gale", [
        {"type": "noise", "name": "pn", "color": "pink"},
        {"type": "wander", "name": "gust", "rate": 1.5, "range": [0.0008, 0.004]},
        {"type": "math", "name": "wind", "op": "mul", "a": "pn", "b": "gust"}, SIGN, out("hit")], 4.0),
    ("sheet_bend", "sheet_wobble", [
        {"type": "wander", "name": "bend", "rate": 3, "range": [0.94, 1.06]}, SHEET, out("hit")], 3.0),
    ("sheet_bend", "sheet_sag", [
        {"type": "ramp", "name": "bend", "from": 1.0, "to": 0.8, "time": 2.0}, SHEET, out("hit")], 3.0),
    ("pipe_layers", "pipe_crack", [PIPE] + CRACK + [
        {"type": "mix", "name": "dry", "inputs": ["hit", "crk_g"]}, out("dry")], 3.5),
    ("pipe_layers", "pipe_crack_chips", [PIPE] + CRACK + CHIPS + [
        {"type": "mix", "name": "dry", "inputs": ["hit", "crk_g", "chips_g"]}, out("dry")], 3.5),
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
        sid = f"b23_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch23"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls23.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch23.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
