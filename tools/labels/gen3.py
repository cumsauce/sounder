"""Batch 2: varied sounds (textures, whooshes, machines, water, UI, voices,
blasts, strings) for blind labelling. Same pipeline as gen.py."""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
rng = random.Random(20261119)
u = rng.uniform
pick = rng.choice
sd = lambda: rng.randint(1, 999)


def wind():
    return [{"type": "wander", "name": "sp", "rate": u(0.2, 1), "range": [u(3, 8), u(10, 25)], "seed": sd()},
            {"type": "wind", "name": "w", "speed": "sp", "gust": round(u(0.1, 0.8), 2), "whistle": round(u(0, 0.6), 2), "seed": sd()}], "w", 2.5


def water():
    return [{"type": "wander", "name": "fl", "rate": u(0.3, 1.5), "range": [u(0.2, 0.4), u(0.5, 0.9)], "seed": sd()},
            {"type": "water", "name": "w", "flow": "fl", "density": round(u(80, 800)), "depth": round(u(0.2, 0.9), 2), "tone": round(u(0.3, 0.7), 2), "seed": sd()}], "w", 2.5


def splash():
    return [{"type": "splash", "name": "s", "size": round(u(0.01, 0.6), 3), "tail": round(u(0.5, 2), 2), "seed": sd()}], "s", 2.0


def fire():
    return [{"type": "fire", "name": "f", "size": round(u(0.1, 3), 2), "intensity": round(u(0.2, 0.9), 2), "crackle": round(u(0.1, 0.9), 2), "seed": sd()}], "f", 2.5


def cloth():
    return [{"type": "wander", "name": "m", "rate": u(1, 4), "range": [0, u(0.8, 2.5)], "seed": sd()},
            {"type": "cloth", "name": "c", "motion": "m", "weight": round(u(0.1, 0.9), 2), "seed": sd()}], "c", 2.0


def whoosh():
    t = u(0.3, 0.9)
    return [{"type": "noise", "name": "n", "color": pick(["pink", "white", "brown"]), "seed": sd()},
            {"type": "env", "name": "e", "points": [[0, 0], [t * u(0.5, 0.8), 1], [t, 0]], "curve": "smooth"},
            {"type": "ramp", "name": "f", "from": round(u(200, 800)), "to": round(u(1500, 6000)), "time": t},
            {"type": "filter", "name": "bp", "input": "n", "mode": "bp", "freq": "f", "q": round(u(0.7, 3), 2)},
            {"type": "math", "name": "w", "op": "mul", "a": "bp", "b": "e"}], "w", 1.0


def engine():
    t = 2.0
    return [{"type": "ramp", "name": "r", "from": round(u(700, 2000)), "to": round(u(2000, 6000)), "time": t, "curve": "exp"},
            {"type": "engine", "name": "e", "rpm": "r", "cylinders": pick([1, 2, 4, 6, 8]), "load": round(u(0.2, 0.9), 2), "exhaust_length": round(u(0.5, 2.5), 2), "seed": sd()}], "e", 2.0


def motor():
    return [{"type": "ramp", "name": "r", "from": 0, "to": round(u(2000, 12000)), "time": u(0.5, 1.5), "curve": "linear"},
            {"type": "motor", "name": "m", "rpm": "r", "poles": pick([2, 4, 6]), "gear_teeth": pick([0, 12, 30]), "whine": round(u(0.1, 0.7), 2), "load": round(u(0.1, 0.6), 2), "seed": sd()}], "m", 2.0


def ui():
    a = pick([523, 587, 659, 784, 880, 988, 1175])
    b = a * pick([1.0, 1.25, 1.5, 2.0, 0.75])
    shape = pick(["sine", "tri", "square"])
    return [{"type": "osc", "name": "o1", "shape": shape, "freq": a, "amp": 0.5},
            {"type": "env", "name": "e1", "points": [[0, 0], [0.01, 0], [0.013, 1], [u(0.08, 0.2), 0]]},
            {"type": "math", "name": "n1", "op": "mul", "a": "o1", "b": "e1"},
            {"type": "osc", "name": "o2", "shape": shape, "freq": round(b, 1), "amp": 0.5},
            {"type": "env", "name": "e2", "points": [[0, 0], [0.08, 0], [0.083, 1], [u(0.2, 0.45), 0]]},
            {"type": "math", "name": "n2", "op": "mul", "a": "o2", "b": "e2"},
            {"type": "mix", "name": "m", "inputs": ["n1", "n2"]}], "m", 0.7


def spark():
    return [{"type": "spark", "name": "s", "rate": round(u(2, 40), 1), "body": round(u(1500, 6000)), "seed": sd()}], "s", 2.0


def drips():
    return [{"type": "poisson", "name": "p", "rate": u(2, 8), "seed": sd()},
            {"type": "voices", "name": "v", "events": "p", "length": 0.15, "max": 64, "map": {"r": f"{u(1, 2):.2f} + $rand * {u(1, 4):.2f}"},
             "patch": [{"type": "bubble", "name": "b", "radius_mm": "$r"}]}], "v", 2.5


def creature():
    # Fitted to refs/creature_set (three CC0 growls): low f0, a long tract
    # (scale under 1), heavy fry and roughness.
    f0 = u(50, 90)
    return [{"type": "ramp", "name": "p", "from": round(f0 * u(1.1, 1.4)), "to": round(f0), "time": u(0.6, 1.3)},
            {"type": "glottis", "name": "g", "f0": "p", "tension": round(u(0.6, 0.9), 2), "breath": round(u(0.2, 0.6), 2), "roughness": round(u(0.7, 1.0), 2), "fry": round(u(0.35, 0.7), 2), "subharmonic": round(u(0.2, 0.6), 2)},
            {"type": "env", "name": "e", "points": [[0, 0], [u(0.05, 0.15), 1], [u(0.9, 1.3), 0.8], [1.4, 0]], "curve": "smooth"},
            {"type": "tract", "name": "t", "input": "g", "vowel": pick(["u", "o", "a"]), "scale": round(u(0.5, 0.7), 2), "growl": round(u(0.5, 0.9), 2)},
            {"type": "math", "name": "c", "op": "mul", "a": "t", "b": "e"}], "c", 1.4


def blast():
    if rng.random() < 0.5:
        return [{"type": "explosion", "name": "x", "size": round(u(0.3, 8), 2), "distance": round(u(5, 400)), "seed": sd()}], "x", 2.5
    return [{"type": "gunshot", "name": "x", "caliber": pick([5.56, 7.62, 9, 12, 45]), "barrel": round(u(0.1, 0.8), 2), "distance": round(u(0, 150)), "suppressed": rng.random() < 0.3, "seed": sd()}], "x", 1.5


def string():
    ex = pick(["pluck", "strike", "bow"])
    n = [{"type": "string", "name": "s", "freq": round(u(80, 700), 1), "excite": ex, "damping": round(u(0.1, 0.7), 2), "stiffness": round(u(0, 0.3), 2), "seed": sd()}]
    if ex == "bow":
        n[0]["bow_pressure"] = round(u(0.3, 0.8), 2)
    return n, "s", 1.5


def shatter():
    return [{"type": "shatter", "name": "b", "pieces": rng.randint(10, 60), "spread": round(u(0.2, 0.8), 2), "at": 0.02, "seed": sd()},
            {"type": "voices", "name": "v", "events": "b", "length": 0.3, "max": 80, "map": {"hz": "6000 - $value * 4500"},
             "patch": [{"type": "impulse", "name": "t", "at": [0.0]}, {"type": "gain", "name": "g", "input": "t", "gain": "$value"},
                       {"type": "modal", "name": "m", "excite": "g", "material": pick(["glass", "metal", "stone"]), "f0": "$hz", "modes": 16, "noise": round(u(0, 0.4), 2)}]}], "v", 1.5


FAMILIES = [("wind", wind), ("water", water), ("splash", splash), ("fire", fire), ("cloth", cloth), ("whoosh", whoosh),
            ("engine", engine), ("motor", motor), ("ui", ui), ("spark", spark), ("drips", drips), ("creature", creature),
            ("blast", blast), ("string", string), ("shatter", shatter)]


def main(n=40):
    batch, calls = [], []
    order = [FAMILIES[i % len(FAMILIES)] for i in range(n)]
    rng.shuffle(order)
    for i, (fam, make) in enumerate(order):
        sid = f"c{i + 1:02d}"
        nodes, out, dur = make()
        tgt = {"peak_db": -3} if fam in ("splash", "blast", "spark") else {"lufs": -24}
        nodes.append({"type": "output", "name": "out", "input": out, "fade_in": 0.003, **tgt})
        args = {"sound": sid, "ops": nodes, "seed": sd(), "duration": dur}
        calls += [{"name": "sound_ops", "args": args}, {"name": "export", "args": {"doc": sid, "format": "wav"}}]
        batch.append({"id": sid, "family": fam, "doc": args})
    json.dump(calls, open(os.path.join(HERE, "calls3.json"), "w"))
    json.dump(batch, open(os.path.join(HERE, "batch3.json"), "w"), indent=1)
    p = subprocess.run([sys.executable, "mcp_driver.py", "--client", "label3", "--calls", os.path.join(HERE, "calls3.json")],
                       cwd=HERE, capture_output=True, text=True)
    bad = [" ".join(map(str, json.loads(l).get("text", [])))[:200] for l in p.stdout.splitlines()
           if l.startswith("{") and json.loads(l).get("ok") is False]
    print("failures:", len(bad)); [print(" ", b) for b in bad]


if __name__ == "__main__":
    main()
