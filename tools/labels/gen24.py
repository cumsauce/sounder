"""Batch 24: `impact` sliding. A marble rolling on a board (`slide` over a
rough profile), a metal box scraped across stone and a wooden door creaking
(`friction`), a bowed metal bar (friction near the Stribeck speed), and the
bend again on a bright, noisy sheet (the noise bands now bend). Each take dry
and dressed.

  python3 tools/labels/gen24.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261121)


def out(src, peak=-3):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def roll(slide, rough, grain):
    return {"type": "impact", "name": "hit", "material": "wood", "size": 0.6, "noise": 0.6, "tilt": 4,
            "loss": 1.5, "object_mass": 2.0, "striker_mass": 0.006, "hardness": 1.0, "drive": 0.06,
            "radius": 0.008, "slide": slide, "roughness": rough, "grain": grain, "at": 0.005}


SCRAPE = {"type": "impact", "name": "hit", "material": "metal", "size": 0.4, "thickness": 0.0015,
          "noise": 0.4, "noise_width": 0.1, "loss": 0.6, "object_mass": 3.0, "height": 0.001,
          "floor": "stone", "slide": "pull", "roughness": 0.0001, "grain": 0.003,
          "friction": [0.6, 0.4], "stribeck": 0.1, "at": 0.005}
CREAK = {"type": "impact", "name": "hit", "material": "wood", "size": 0.8, "noise": 0.3, "tilt": 2,
         "loss": 1.0, "object_mass": 20.0, "striker_mass": 0.5, "hardness": 0.6, "drive": 30.0,
         "slide": "swing", "friction": [0.8, 0.3], "stribeck": 0.02, "at": 0.005}
BOW = {"type": "impact", "name": "hit", "material": "metal", "shape": "bar", "f0": 440, "loss": 0.5,
       "object_mass": 0.3, "striker_mass": 0.05, "hardness": 0.5, "drive": "press", "slide": 0.03,
       "friction": [0.8, 0.3], "stribeck": 0.05, "at": 0.005}
SHEET = {"type": "impact", "name": "hit", "material": "metal", "size": 0.3, "thickness": 0.003,
         "noise": 0.6, "noise_width": 0.1, "loss": 0.4, "object_mass": 2.0, "striker_mass": 0.5,
         "hardness": 0.6, "speed": 3.0, "pitch": "bend", "at": 0.005}

DESIGNS = [
    ("marble_roll", "roll_fine", [
        {"type": "wander", "name": "sp", "rate": 1.5, "range": [0.3, 0.6]}, roll("sp", 2e-5, 0.003), out("hit")], 3.0),
    ("marble_roll", "roll_coarse", [
        {"type": "ramp", "name": "sp", "from": 0.7, "to": 0.15, "time": 2.8}, roll("sp", 6e-5, 0.006), out("hit")], 3.0),
    ("box_scrape", "scrape_pull", [
        {"type": "ramp", "name": "pull", "from": 0.6, "to": 0.05, "time": 1.4, "curve": "linear"}, SCRAPE, out("hit")], 1.6),
    ("box_scrape", "scrape_jerky", [
        {"type": "wander", "name": "pull", "rate": 6, "range": [0.0, 0.6]}, SCRAPE, out("hit")], 1.6),
    ("door_creak", "creak_slow", [
        {"type": "ramp", "name": "swing", "from": 0.004, "to": 0.03, "time": 2.0, "curve": "linear"}, CREAK, out("hit")], 2.2),
    ("door_creak", "creak_wander", [
        {"type": "wander", "name": "swing", "rate": 3, "range": [0.003, 0.03]}, CREAK, out("hit")], 2.2),
    ("bowed_bar", "bow_steady", [
        {"type": "ramp", "name": "press", "from": 0.5, "to": 3.0, "time": 0.3, "curve": "linear"}, BOW, out("hit")], 2.5),
    ("bowed_bar", "bow_hard", [
        {"type": "ramp", "name": "press", "from": 1.0, "to": 8.0, "time": 0.3, "curve": "linear"}, BOW, out("hit")], 2.5),
    ("sheet_bend", "sheet_wobble", [
        {"type": "wander", "name": "bend", "rate": 3, "range": [0.9, 1.1]}, SHEET, out("hit")], 3.0),
    ("sheet_bend", "sheet_sag", [
        {"type": "ramp", "name": "bend", "from": 1.0, "to": 0.75, "time": 2.0}, SHEET, out("hit")], 3.0),
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
        sid = f"b24_{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch24"}})
        batch.append({"id": sid, "family": "impact", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": c["args"]})
    cf = os.path.join(project, "calls24.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch24.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
