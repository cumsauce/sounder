"""Batch 6: the Farnell ports (rain, stream, pour, roar, fly, cicada,
helicopter, rocket, droid, transporter, torpedo, phaser_beam, phone_tone,
two_tone), mostly at their defaults, a few settings each. Every cue renders
dry and dressed (a small room, for listening only) under shuffled ids
s01.. ; batch6.json is the key.

  python3 tools/labels/gen6.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261119)

# (design name, node fields, duration s, output target)
DESIGNS = [
    ("rain_leaf", {"type": "rain", "surface": "leaf", "intensity": 30}, 4, {"lufs": -24}),
    ("rain_water", {"type": "rain", "surface": "water", "intensity": 8}, 4, {"lufs": -24}),
    ("rain_window", {"type": "rain", "surface": "window", "intensity": 20}, 4, {"lufs": -24}),
    ("rain_ground", {"type": "rain", "surface": "ground", "intensity": 60}, 4, {"lufs": -24}),
    ("stream", {"type": "stream"}, 5, {"lufs": -24}),
    ("stream_busy", {"type": "stream", "flow": 2, "layers": 4}, 5, {"lufs": -24}),
    ("pour", {"type": "pour", "fill_time": 5}, 6.5, {"lufs": -24}),
    ("roar", {"type": "roar"}, 1.6, {"peak_db": -3}),
    ("roar_big", {"type": "roar", "size": 2, "aggression": 0.8, "length": 1.5}, 2.2, {"peak_db": -3}),
    ("fly", {"type": "fly"}, 3, {"lufs": -24}),
    ("cicada_call", {"type": "cicada", "kind": "call"}, 4, {"lufs": -26}),
    ("cicada_buzz", {"type": "cicada", "kind": "buzz"}, 4, {"lufs": -26}),
    ("helicopter", {"type": "helicopter"}, 4, {"lufs": -22}),
    ("helicopter_far", {"type": "helicopter", "distance": 0.7}, 4, {"lufs": -24}),
    ("rocket", {"type": "rocket", "burn": 4}, 5, {"lufs": -22}),
    ("droid", {"type": "droid", "length": 3}, 3.5, {"lufs": -22}),
    ("transporter", {"type": "transporter", "length": 6}, 7, {"lufs": -24}),
    ("torpedo", {"type": "torpedo", "length": 4}, 4.5, {"peak_db": -3}),
    ("beam_kill", {"type": "phaser_beam", "length": 1.2}, 1.4, {"peak_db": -3}),
    ("phone_dial", {"type": "phone_tone", "kind": "dial"}, 3, {"lufs": -24}),
    ("phone_busy", {"type": "phone_tone", "kind": "busy"}, 3, {"lufs": -24}),
    ("phone_ringback", {"type": "phone_tone", "kind": "ringback"}, 5, {"lufs": -24}),
    ("phone_dtmf", {"type": "phone_tone", "kind": "dtmf", "digits": "5551234"}, 3, {"lufs": -24}),
    ("phone_rotary", {"type": "phone_tone", "kind": "pulse_dial", "digits": "37"}, 3, {"peak_db": -3}),
    ("two_tone", {"type": "two_tone"}, 3, {"lufs": -24}),
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
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch6"}}]
        batch.append({"id": sid, "family": "farnell", "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls6.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch6.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
