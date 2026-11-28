"""Batch 32: the weapon kit on `gunshot`. Per group, the shot before the kit
(Farnell's shot: punch 0, drive 0, no space, dressed with a room as in the
earlier listens), the kit dry and in spaces, and one CC0 recording from
refs/gun_set as a blind take (the recordings are for comparison only).
Automatic fire: the earlier voices burst vs the `events` port.

  python3 tools/labels/gen32.py <project dir>
"""
import json, os, random, shutil, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261128)


def out(src, peak=-1):
    return {"type": "output", "name": "out", "input": src, "fade_in": 0.0005, "peak_db": peak}


def shot(**kw):
    return dict({"type": "gunshot", "name": "shot", "seed": 1}, **kw)


def room(src):
    return {"type": "reverb", "name": "room_dress", "input": src, "model": "room", "size": 0.3, "decay": 0.7, "mix": 0.18}


OLD = {"punch": 0, "drive": 0}
BURST = {"type": "pattern", "name": "rof", "every": 0.08, "count": 10, "jitter": 0.002, "seed": 2}

DESIGNS = [
    ("gun_pistol", "old_dry", [shot(caliber=9, barrel=0.1, **OLD), out("shot")], 1.5),
    ("gun_pistol", "old_room", [shot(caliber=9, barrel=0.1, **OLD), room("shot"), out("room_dress")], 1.5),
    ("gun_pistol", "kit_dry", [shot(weapon="pistol"), out("shot")], 1.5),
    ("gun_pistol", "kit_open", [shot(weapon="pistol", space="open"), out("shot")], 1.5),
    ("gun_pistol", "kit_urban", [shot(weapon="pistol", space="urban"), out("shot")], 2.0),
    ("gun_pistol", "ref_427598", None, 0),
    ("gun_rifle", "old_room", [shot(caliber=7.62, barrel=0.5, **OLD), room("shot"), out("room_dress")], 1.5),
    ("gun_rifle", "kit_dry", [shot(weapon="rifle"), out("shot")], 1.5),
    ("gun_rifle", "kit_open", [shot(weapon="rifle", space="open"), out("shot")], 1.5),
    ("gun_rifle", "kit_forest", [shot(weapon="rifle", space="forest"), out("shot")], 2.0),
    ("gun_rifle", "kit_canyon", [shot(weapon="rifle", space="canyon"), out("shot")], 4.0),
    ("gun_rifle", "ref_427594", None, 0),
    ("gun_shotgun", "kit_dry", [shot(weapon="shotgun"), out("shot")], 1.5),
    ("gun_shotgun", "kit_open", [shot(weapon="shotgun", space="open"), out("shot")], 1.5),
    ("gun_shotgun", "kit_room", [shot(weapon="shotgun", space="room"), out("shot")], 1.5),
    ("gun_shotgun", "ref_213925", None, 0),
    ("gun_indoor", "kit_room", [shot(weapon="pistol", space="room"), out("shot")], 1.5),
    ("gun_indoor", "kit_hall", [shot(weapon="pistol", space="hall"), out("shot")], 2.5),
    ("gun_indoor", "kit_hangar", [shot(weapon="pistol", space="hangar"), out("shot")], 3.5),
    ("gun_auto", "old_voices_room", [BURST, {"type": "voices", "name": "shots", "events": "rof", "length": 0.4, "max": 16,
        "patch": [shot(caliber=9, barrel=0.2, **OLD)]}, room("shots"), out("room_dress")], 1.8),
    ("gun_auto", "kit_events_dry", [BURST, shot(weapon="smg", events="rof"), out("shot")], 1.8),
    ("gun_auto", "kit_events_urban", [BURST, shot(weapon="smg", events="rof", space="urban"), out("shot")], 2.5),
    ("gun_auto", "kit_events_open_lmg", [dict(BURST, every=0.1), shot(weapon="lmg", events="rof", space="open"), out("shot")], 2.5),
]



def main(project):
    cues = []
    for group, design, ops, dur in DESIGNS:
        cues.append({"group": group, "design": design, "ops": ops, "dur": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    audio = os.path.join(project, "batch32_audio")
    os.makedirs(audio, exist_ok=True)
    for i, c in enumerate(cues):
        sid = f"b32_{i + 1:02d}"
        entry = {"id": sid, "family": "gunshot", "group": c["group"], "design": c["design"], "dressed": c["design"] != "kit_dry" and c["design"] != "old_dry"}
        if c["ops"] is None:
            src = os.path.join(REPO, "refs", "gun_set", c["design"][4:] + "_cut.wav")
            shutil.copy(src, os.path.join(audio, sid + ".wav"))
            entry["doc"] = {"ops": [{"type": "recording", "file": "refs/gun_set/" + c["design"][4:] + "_cut.wav"}]}
        else:
            args = {"ops": c["ops"], "seed": 1, "duration": c["dur"]}
            calls.append({"name": "new_sound", "args": {"name": sid}})
            calls.append({"name": "sound_ops", "args": dict(args, doc=sid)})
            calls.append({"name": "export", "args": {"doc": sid, "format": "wav", "path": os.path.join(audio, sid + ".wav")}})
            entry["doc"] = args
        batch.append(entry)
    cf = os.path.join(project, "calls32.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch32.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off", FRIENDSLOP_DATA_DIR=os.path.join(project, ".sounder"))
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:400])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
