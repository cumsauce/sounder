"""Batch 9: the foley ports (creature, surf, lava, cave, frog, leaves,
underwater) and the rain surfaces retuned after batch 8. Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch9.json is the key.

  python3 tools/labels/gen9.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261122)

# (design name, node fields, duration s, output target)
DESIGNS = [
    ("creature_roar_big", {"type": "creature", "call": "roar", "size": 0.9, "aggression": 0.8, "length": 2}, 2.5, {"peak_db": -3}),
    ("creature_growl", {"type": "creature", "call": "growl", "size": 0.7, "length": 1.6}, 2, {"peak_db": -3}),
    ("creature_snarl", {"type": "creature", "call": "snarl", "size": 0.5, "length": 2}, 2.5, {"peak_db": -3}),
    ("creature_whine", {"type": "creature", "call": "whine", "size": 0.4, "length": 1.5}, 2, {"peak_db": -3}),
    ("creature_roar_small_rough", {"type": "creature", "call": "roar", "size": 0.3, "roughness": 0.9, "length": 1.2}, 1.5, {"peak_db": -3}),
    ("creature_growl_huge", {"type": "creature", "call": "growl", "size": 0.95, "aggression": 1, "roughness": 1, "length": 2.5}, 3, {"peak_db": -3}),
    ("surf_sand", {"type": "surf", "shore": "sand", "wave_height": 1, "period": 8}, 20, {"lufs": -26}),
    ("surf_shingle", {"type": "surf", "shore": "shingle", "wave_height": 1, "period": 8}, 20, {"lufs": -26}),
    ("surf_storm", {"type": "surf", "shore": "shingle", "wave_height": 2.5, "period": 3}, 15, {"lufs": -24}),
    ("lava", {"type": "lava", "size": 20}, 6, {"lufs": -24}),
    ("lava_big", {"type": "lava", "size": 150, "activity": 3}, 6, {"lufs": -24}),
    ("cave_wet", {"type": "cave", "drips": 1, "depth": 0}, 5, {"peak_db": -3}),
    ("cave_sparse", {"type": "cave", "drips": 0.3}, 8, {"peak_db": -3}),
    ("frog_small", {"type": "frog", "size": 3, "rate": 1}, 6, {"lufs": -24}),
    ("frog_chorus", {"type": "frog", "size": 6, "rate": 0.5, "chorus": 6}, 10, {"lufs": -24}),
    ("frog_bull", {"type": "frog", "size": 15, "rate": 0.3}, 8, {"lufs": -24}),
    ("leaves_trees", {"type": "leaves", "wind": 8, "foliage": "leaves"}, 5, {"lufs": -26}),
    ("leaves_grass", {"type": "leaves", "wind": 8, "foliage": "grass"}, 5, {"lufs": -26}),
    ("leaves_dry", {"type": "leaves", "wind": 12, "foliage": "dry", "gust": 0.5}, 5, {"lufs": -26}),
    ("underwater", {"type": "underwater", "depth": 4, "bubbles": 3, "pressure": 0.3}, 6, {"lufs": -26}),
    ("rain_stone_v2", {"type": "rain", "surface": "stone"}, 5, {"lufs": -24}),
    ("rain_glass_v2", {"type": "rain", "surface": "glass"}, 5, {"lufs": -24}),
    ("rain_metal_roof_v2", {"type": "rain", "surface": "soft", "roof": "metal", "roof_height": 2}, 5, {"lufs": -24}),
    ("rain_water_v2", {"type": "rain", "surface": "water"}, 5, {"lufs": -24}),
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
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch9"}}]
        batch.append({"id": sid, "family": "foley", "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls9.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch9.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
