"""Generate a varied batch of plausible foley sounds for blind labelling.
Each sound is a sounder document; renders and exports through a throwaway
server in this folder's project. Writes batch.json (id -> family, doc)."""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
PROJ = os.path.join(HERE, "proj")
REPO = "/Users/doomy/Code/sounder"
rng = random.Random(20261117)
u = rng.uniform
pick = rng.choice


def burst(t=0.01, dec=None, color="pink"):
    dec = dec or u(0.004, 0.02)
    return [
        {"type": "noise", "name": "n", "color": color, "seed": rng.randint(1, 99)},
        {"type": "decay", "name": "e", "time": dec, "at": t},
        {"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"},
    ]


def struck(material):
    noise = {"wood": u(0.3, 0.8), "metal": u(0.2, 0.85), "stone": u(0.4, 0.9), "glass": u(0.0, 0.4)}[material]
    shape = pick({"wood": ["plate", "bar"], "metal": ["plate", "bar", "tube", "membrane", "shell"],
                  "stone": ["plate", "bar"], "glass": ["plate", "shell", "tube"]}[material])
    f0 = {"wood": u(90, 500), "metal": u(100, 900), "stone": u(150, 800), "glass": u(500, 2500)}[material]
    loss = {"wood": u(1.2, 4), "metal": u(0.3, 1.5), "stone": u(2, 6), "glass": u(0.3, 1.5)}[material]
    m = {"type": "modal", "name": "body", "excite": "burst", "material": material, "shape": shape,
         "f0": round(f0, 1), "loss": round(loss, 2), "noise": round(noise, 2), "tilt": round(u(-2, 6), 1)}
    if material == "metal" and rng.random() < 0.5:
        m["strike_glide"] = round(u(15, 60))
    return burst() + [m], "body"


def footstep():
    surf = pick(["concrete", "gravel", "grass", "wood"])
    n = {"type": "footstep", "name": "f", "surface": surf, "weight": round(u(0.15, 0.7), 2),
         "pace": pick(["walk", "run", "sneak"]), "shoe": pick(["sneaker", "hard", "boot", "bare"]), "at": 0.01}
    if surf == "wood":
        n.update({"noise": round(u(0.3, 0.6), 2), "ring": round(u(0.5, 1.5), 2)})
    return [n], "f"


def click():
    return [{"type": "click", "name": "c", "kind": pick(["latch", "switch", "button", "lock", "clasp"]),
             "size": round(u(0.1, 0.9), 2), "hardness": round(u(0.3, 1.0), 2), "at": 0.01,
             "noise": round(u(0.3, 0.9), 2)}], "c"


def crumple():
    return [{"type": "crumple", "name": "c", "density": round(u(15, 160)), "size": round(u(0.1, 0.9), 2),
             "hardness": round(u(0.1, 0.95), 2), "seed": rng.randint(1, 99)}], "c"


def scrape():
    speed = [{"type": "env", "name": "sp", "points": [[0, 0], [0.05, u(0.4, 2)], [u(0.3, 0.9), u(0.2, 1.5)], [1.0, 0]], "curve": "smooth"}]
    mat = pick(["wood", "metal", "stone"])
    return speed + [
        {"type": "friction", "name": "fr", "speed": "sp", "pressure": round(u(0.3, 1), 2),
         "roughness": round(u(0.1, 0.9), 2), "grain": round(u(80, 600))},
        {"type": "modal", "name": "body", "excite": "fr", "material": mat, "f0": round(u(120, 700)),
         "loss": round(u(1, 4), 2), "noise": round(u(0.3, 0.8), 2), "gain_db": -12},
    ], "body"


def rolling():
    return [
        {"type": "env", "name": "sp", "points": [[0, 0], [0.1, u(0.5, 3)], [1.1, u(0.2, 1.5)], [1.3, 0]], "curve": "smooth"},
        {"type": "rolling", "name": "r", "speed": "sp", "roughness": round(u(0.2, 0.9), 2), "size": round(u(0.001, 0.01), 4)},
        {"type": "gain", "name": "rg", "input": "r", "db": -18},
        {"type": "modal", "name": "body", "excite": "rg", "material": pick(["wood", "metal", "stone"]),
         "f0": round(u(150, 900)), "loss": round(u(1, 3), 2), "noise": round(u(0.3, 0.8), 2)},
    ], "body"


def knocks():
    n = rng.randint(2, 4)
    gap = u(0.08, 0.3)
    nodes, _ = struck(pick(["wood", "wood", "metal", "stone"]))
    nodes[1]["at"] = 0.01
    times = [round(0.01 + i * gap * u(0.85, 1.15), 3) for i in range(n)]
    nodes[0:3] = [
        {"type": "noise", "name": "n", "color": "pink", "seed": rng.randint(1, 99)},
        {"type": "pattern", "name": "p", "times": times},
        {"type": "trigger_env", "name": "e", "events": "p", "attack": 0.0005, "decay": u(0.005, 0.02)},
        {"type": "math", "name": "burst", "op": "mul", "a": "n", "b": "e"},
    ]
    return nodes, "body"


def layered():
    nodes, body = struck(pick(["wood", "metal", "stone"]))
    nodes += [
        {"type": "noise", "name": "wn", "color": "white", "seed": rng.randint(1, 99)},
        {"type": "decay", "name": "ce", "time": u(0.006, 0.03), "at": 0.01},
        {"type": "math", "name": "craw", "op": "mul", "a": "wn", "b": "ce"},
        {"type": "filter", "name": "crack", "input": "craw", "mode": "bp", "freq": round(u(1500, 5000)), "q": 0.6},
        {"type": "noise", "name": "bn", "color": "brown", "seed": rng.randint(1, 99)},
        {"type": "decay", "name": "te", "time": u(0.15, 0.6), "at": 0.01},
        {"type": "math", "name": "traw", "op": "mul", "a": "bn", "b": "te"},
        {"type": "filter", "name": "thud", "input": "traw", "mode": "lp", "freq": round(u(120, 260)), "slope": 24},
        {"type": "mix", "name": "dry", "inputs": [body, {"input": "crack", "gain": round(u(0.1, 0.6), 2)},
                                                  {"input": "thud", "gain": round(u(0.3, 1.5), 2)}], "to": "mono"},
        {"type": "finish", "name": "fin", "input": "dry", "punch": round(u(0, 0.6), 2), "warmth": round(u(0, 0.6), 2)},
    ]
    return nodes, "fin"


FAMILIES = [("wood hit", lambda: struck("wood")), ("metal hit", lambda: struck("metal")),
            ("stone hit", lambda: struck("stone")), ("glass hit", lambda: struck("glass")),
            ("footstep", footstep), ("click", click), ("crumple", crumple), ("scrape", scrape),
            ("rolling", rolling), ("knocks", knocks), ("layered hit", layered)]


def main(n=40):
    os.makedirs(os.path.join(PROJ, ".sounder"), exist_ok=True)
    batch, calls = [], []
    order = [FAMILIES[i % len(FAMILIES)] for i in range(n)]
    rng.shuffle(order)
    for i, (fam, make) in enumerate(order):
        sid = f"s{i + 1:02d}"
        nodes, out = make()
        long_ = fam in ("scrape", "rolling", "crumple")
        nodes.append({"type": "output", "name": "out", "input": out, "lufs": -26, "fade_in": 0.0005})
        args = {"sound": sid, "ops": nodes, "seed": rng.randint(1, 999)}
        if long_:
            args["duration"] = 1.4
        calls.append({"name": "sound_ops", "args": args})
        calls.append({"name": "export", "args": {"doc": sid}})
        batch.append({"id": sid, "family": fam, "doc": args})
    json.dump(calls, open(os.path.join(HERE, "calls.json"), "w"))
    json.dump(batch, open(os.path.join(HERE, "batch.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=PROJ, FRIENDSLOP_DATA_DIR="")
    p = subprocess.run([sys.executable, "mcp_driver.py", "--client", "label", "--calls", os.path.join(HERE, "calls.json")],
                       cwd=HERE, env=env, capture_output=True, text=True)
    bad = 0
    for l in p.stdout.splitlines():
        if l.startswith("{"):
            d = json.loads(l)
            if d.get("ok") is False:
                bad += 1
                print("FAIL", " ".join(map(str, d.get("text", [])))[:200])
    print("done; failures:", bad)


if __name__ == "__main__":
    main()
