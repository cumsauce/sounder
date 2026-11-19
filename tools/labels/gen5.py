"""Batch 5: stone, built on the slab from batch 4.

In batch 4 only the heavy slab read as stone ("hitting a rock"); small,
bright, dead hits read as mouse and keyboard clicks. So every design here
keeps the slab's weight (brown noise low-passed under 250 Hz) and grit
(crackle noise, band-passed) under the hit, and varies the size, how much
struck-stone body (`modal` stone) sits on top, and the debris after it.
Each design renders dry and dressed (a small room, for listening only)
under shuffled ids s01.. ; batch5.json is the key.

  python3 tools/labels/gen5.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261121)

SRC = [{"type": "noise", "name": "wn", "color": "white"}, {"type": "noise", "name": "bn", "color": "brown"},
       {"type": "noise", "name": "gn", "color": "crackle"}]


def env_noise(name, src, at, t, mode, freq, db, q=0.8, slope=None):
    f = {"type": "filter", "name": f"{name}_f", "input": f"{name}_x", "mode": mode, "freq": freq}
    if q and mode == "bp":
        f["q"] = q
    if slope:
        f["slope"] = slope
    return [{"type": "decay", "name": f"{name}_e", "time": t, "at": at},
            {"type": "math", "name": f"{name}_x", "op": "mul", "a": src, "b": f"{name}_e"}, f,
            {"type": "gain", "name": name, "input": f"{name}_f", "db": db}]


def stone_hit(p, at, scale_db, k):
    """One stone impact: weight, crack, dead body, grit, and optional
    struck-stone modes. `p` holds the design's numbers."""
    n = env_noise(f"w{k}", "bn", at, p["weight_t"], "lp", p["weight_hz"], p["weight_db"] + scale_db, slope=24)
    n += env_noise(f"c{k}", "wn", at, p["crack_t"], "hp", p["crack_hz"], p["crack_db"] + scale_db)
    n += env_noise(f"b{k}", "wn", at, p["body_t"], "bp", p["body_hz"], p["body_db"] + scale_db, q=0.8)
    n += env_noise(f"g{k}", "gn", at, p["grit_t"], "bp", p["grit_hz"], p["grit_db"] + scale_db, q=0.7)
    parts = [f"w{k}", f"c{k}", f"b{k}", f"g{k}"]
    if p.get("modal_db") is not None:
        n += [{"type": "decay", "name": f"m{k}_e", "time": 0.004, "at": at},
              {"type": "math", "name": f"m{k}_x", "op": "mul", "a": "wn", "b": f"m{k}_e"},
              {"type": "modal", "name": f"m{k}_r", "excite": f"m{k}_x", "material": "stone", "f0": p["modal_f0"],
               "loss": p["modal_loss"], "noise": 0.5, "noise_width": 0.2, "modes": 48},
              {"type": "gain", "name": f"m{k}", "input": f"m{k}_r", "db": p["modal_db"] + scale_db}]
        parts.append(f"m{k}")
    return n, parts


def chips(count, start, spread, db):
    n, names = [], []
    t = start
    for i in range(count):
        t += rng.uniform(0.3, 1.0) * spread / count
        n += env_noise(f"ch{i}", "wn", round(t, 3), rng.uniform(0.002, 0.005), "bp", rng.uniform(1500, 4000), db - rng.uniform(0, 8), q=1.2)
        names.append(f"ch{i}")
    return n, names


BASE = dict(weight_t=0.3, weight_hz=180, weight_db=0, crack_t=0.01, crack_hz=1800, crack_db=-6,
            body_t=0.12, body_hz=450, body_db=4, grit_t=0.12, grit_hz=2500, grit_db=0)


def design(p, hits, chip=None):
    n, ins = list(SRC), []
    for k, (t, db) in enumerate(hits):
        h, names = stone_hit(p, t, db, k)
        n += h
        ins += names
    if chip:
        c, names = chips(*chip)
        n += c
        ins += names
    n.append({"type": "mix", "name": "dry", "inputs": ins, "to": "mono"})
    return n


def slab():
    """The batch 4 slab again, as the anchor (heard as "hitting a rock")."""
    return design(dict(BASE, weight_hz=140, weight_t=0.35, body_hz=420, body_t=0.14, crack_t=0.012), [(0.005, 0)], (5, 0.09, 0.3, -18)), 1.2


def rock_drop():
    """A fist-sized rock dropped on concrete: the slab's layers smaller and
    brighter, a little stone body on top, one small bounce."""
    p = dict(BASE, weight_t=0.15, weight_hz=230, weight_db=-4, body_hz=700, body_t=0.07, crack_db=-4, grit_t=0.08,
             modal_f0=900, modal_loss=2.0, modal_db=-8)
    return design(p, [(0.005, 0), (0.13, -12)], (3, 0.15, 0.2, -20)), 1.0


def rock_drop_dead():
    """The same rock with no stone body: only noise layers."""
    p = dict(BASE, weight_t=0.15, weight_hz=230, weight_db=-4, body_hz=700, body_t=0.07, crack_db=-4, grit_t=0.08)
    return design(p, [(0.005, 0), (0.13, -12)], (3, 0.15, 0.2, -20)), 1.0


def brick_drop():
    """A brick dropped flat on concrete: dense, a short boxy body, chips."""
    p = dict(BASE, weight_t=0.22, weight_hz=200, weight_db=-2, body_hz=800, body_t=0.06, crack_db=-5,
             modal_f0=700, modal_loss=2.5, modal_db=-6)
    return design(p, [(0.005, 0)], (6, 0.04, 0.35, -16)), 1.0


def boulder_drop():
    """A boulder dropped onto rock: long low weight, heavy grit, debris."""
    p = dict(BASE, weight_t=0.6, weight_hz=110, weight_db=2, body_hz=320, body_t=0.2, body_db=3, crack_t=0.015,
             crack_hz=1500, crack_db=-8, grit_t=0.25, grit_hz=2000, grit_db=2)
    return design(p, [(0.005, 0)], (12, 0.12, 0.8, -14)), 1.8


def hammer_on_stone():
    """A hammer striking a stone block: a hard crack, some stone ring, the
    block's weight under it."""
    p = dict(BASE, weight_t=0.12, weight_hz=220, weight_db=-6, body_hz=900, body_t=0.05, body_db=0, crack_t=0.006,
             crack_hz=2500, crack_db=-2, grit_t=0.05, grit_db=-6, modal_f0=1800, modal_loss=1.2, modal_db=-4)
    return design(p, [(0.005, 0)], (2, 0.03, 0.15, -22)), 0.8


def rocks_knocked():
    """Two large rocks knocked together twice."""
    p = dict(BASE, weight_t=0.12, weight_hz=250, weight_db=-6, body_hz=650, body_t=0.06, crack_db=-3, grit_t=0.06,
             grit_db=-3, modal_f0=750, modal_loss=1.8, modal_db=-6)
    return design(p, [(0.005, 0), (0.45, -2)]), 1.0


DESIGNS = [("slab", slab), ("rock_drop", rock_drop), ("rock_drop_dead", rock_drop_dead), ("brick_drop", brick_drop),
           ("boulder_drop", boulder_drop), ("hammer_on_stone", hammer_on_stone), ("rocks_knocked", rocks_knocked)]


def main(project):
    cues = []
    for name, make in DESIGNS:
        nodes, dur = make()
        for dress in (False, True):
            ops = [dict(x) for x in nodes]
            last = "dry"
            if dress:
                ops.append({"type": "reverb", "name": "room", "input": "dry", "model": "room", "size": 0.25, "decay": 0.6, "mix": 0.18})
                last = "room"
            ops.append({"type": "output", "name": "out", "input": last, "peak_db": -3, "fade_in": 0.0005})
            cues.append({"design": name, "dressed": dress, "ops": ops, "duration": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"s{i + 1:02d}"
        args = {"sound": sid, "ops": c["ops"], "duration": c["duration"], "seed": 1}
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch5"}}]
        batch.append({"id": sid, "family": "stone", "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls5.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch5.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project, SOUNDER_HEARING="off")
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{") and json.loads(l).get("ok") is False:
            print("FAIL", " ".join(map(str, json.loads(l).get("text", [])))[:300])
    print(len(batch), "cues")


if __name__ == "__main__":
    main(sys.argv[1])
