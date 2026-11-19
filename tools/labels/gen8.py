"""Batch 8: the physical `rain` (after the foley project), every surface,
three rates, two roofs (batches 6 and 7: the Farnell rain read as static). Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch8.json is the key.

  python3 tools/labels/gen8.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261121)

# (design name, node fields, duration s, output target)
DESIGNS = [
    ("rain_water", {"type": "rain", "surface": "water"}, 5, {"lufs": -24}),
    ("rain_stone", {"type": "rain", "surface": "stone"}, 5, {"lufs": -24}),
    ("rain_leaf", {"type": "rain", "surface": "leaf"}, 5, {"lufs": -24}),
    ("rain_wood", {"type": "rain", "surface": "wood"}, 5, {"lufs": -24}),
    ("rain_soft", {"type": "rain", "surface": "soft"}, 5, {"lufs": -24}),
    ("rain_glass", {"type": "rain", "surface": "glass"}, 5, {"lufs": -24}),
    ("rain_metal", {"type": "rain", "surface": "metal"}, 5, {"lufs": -24}),
    ("rain_stone_light", {"type": "rain", "surface": "stone", "rate": 2}, 5, {"lufs": -26}),
    ("rain_stone_downpour", {"type": "rain", "surface": "stone", "rate": 50}, 5, {"lufs": -22}),
    ("rain_water_downpour", {"type": "rain", "surface": "water", "rate": 50}, 5, {"lufs": -22}),
    ("rain_under_metal_roof", {"type": "rain", "surface": "soft", "roof": "metal", "roof_height": 2}, 5, {"lufs": -24}),
    ("rain_under_wood_roof", {"type": "rain", "surface": "soft", "roof": "wood", "roof_height": 3}, 5, {"lufs": -24}),
]


def main(project):
    cues = []
    for name, node, dur, target in DESIGNS:
        for dress in (False, True):
            ops = [dict(node, name="src")]
            last = "src"
            if dress:
                ops.append({"type": "reverb", "name": "room", "input": "src", "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
                last = "room"
            ops.append(dict({"type": "output", "name": "out", "input": last, "fade_in": 0.002}, **target))
            cues.append({"design": name, "dressed": dress, "ops": ops, "duration": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"s{i + 1:02d}"
        args = {"sound": sid, "ops": c["ops"], "duration": c["duration"], "seed": 1}
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch8"}}]
        batch.append({"id": sid, "family": "rain", "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls8.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch8.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
