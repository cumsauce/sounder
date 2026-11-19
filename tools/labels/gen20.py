"""Batch 20: tester agents' sounds for briefs with no ready-made node (sword
draw, vending machine can, sci-fi door, ice into a glass, zipper). Each
take renders dry and dressed (a small room before the output).

  python3 tools/labels/gen20.py <project dir> <testers dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261203)
BRIEFS = ("sword_draw", "vending_can", "scifi_door", "ice_glass", "zipper")


def dress(ops):
    ops = [dict(o) for o in ops]
    out = next(o for o in ops if o["type"] == "output")
    ops.insert(len(ops) - 1, {"type": "reverb", "name": "room_dress", "input": out["input"], "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18})
    out["input"] = "room_dress"
    return ops


def main(project, testers):
    cues = []
    for f in sorted(os.listdir(testers)):
        if not any(f == f"{b}_{t}.json" for b in BRIEFS for t in "ab"):
            continue
        name = f[:-5]
        d = json.load(open(os.path.join(testers, f)))
        group = name.rsplit("_", 1)[0]
        for dressed in (False, True):
            args = {"ops": dress(d["ops"]) if dressed else d["ops"], "seed": d.get("seed", 1)}
            if d.get("duration"):
                args["duration"] = d["duration"]
            if d.get("params"):
                args["params"] = d["params"]
            if d.get("macros"):
                args["macros"] = d["macros"]
            cues.append({"group": group, "design": name, "dressed": dressed, "args": args, "kind": d.get("kind", "oneshot")})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"s{i + 1:02d}"
        calls.append({"name": "new_sound", "args": {"name": sid, "kind": c["kind"]}})
        calls.append({"name": "sound_ops", "args": dict(c["args"], doc=sid)})
        calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch20"}})
        batch.append({"id": sid, "family": "testers", "group": c["group"], "design": c["design"], "dressed": c["dressed"], "doc": dict(c["args"], ops=c["args"]["ops"])})
    cf = os.path.join(project, "calls20.json")
    json.dump(calls, open(cf, "w"))
    for b in batch:
        b["doc"]["ops"] = b["doc"]["ops"]
    json.dump(batch, open(os.path.join(HERE, "batch20.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
