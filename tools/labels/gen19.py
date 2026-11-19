"""Batch 19: the layer splits heard alone: rain drops and rain hiss, a
creature's voice and its breath. Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch9.json is the key.

  python3 tools/labels/gen9.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261202)

DESIGNS = []
for surf in ("stone", "soft", "wood"):
    DESIGNS.append((f"rain_drops_{surf}", "rain_drops", {"type": "rain", "surface": surf, "layer": "drops"}, 5, {"lufs": -26}))
for surf in ("stone", "soft", "water"):
    DESIGNS.append((f"rain_hiss_{surf}", "rain_hiss", {"type": "rain", "surface": surf, "layer": "hiss"}, 5, {"lufs": -26}))
calls = [("roar", {"size": 0.9, "aggression": 0.8, "length": 2}), ("growl", {"size": 0.95, "aggression": 1, "roughness": 1, "length": 2.5}), ("snarl", {"size": 0.5, "length": 2})]
for layer in ("voice", "breath"):
    for call, f in calls:
        DESIGNS.append((f"creature_{call}_{layer}", "creature_" + layer, dict({"type": "creature", "call": call, "layer": layer}, **f), 3, {"peak_db": -3}))


def main(project):
    cues = []
    for name, group, node, dur, target in DESIGNS:
        for dress in (False, True):
            ops = [dict(node, name="src")]
            last = "src"
            if dress:
                ops.append({"type": "reverb", "name": "room", "input": "src", "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
                last = "room"
            ops.append(dict({"type": "output", "name": "out", "input": last, "fade_in": 0.002}, **target))
            cues.append({"group": group, "design": name, "dressed": dress, "ops": ops, "duration": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"s{i + 1:02d}"
        args = {"sound": sid, "ops": c["ops"], "duration": c["duration"], "seed": 1}
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch19"}}]
        batch.append({"id": sid, "family": "layers", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls19.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch19.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
