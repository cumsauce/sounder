"""Second Farnell set: engine (ch. 45), gunshot (ch. 53), explosion (ch. 54)
as sounder graphs, each rendered beside the current node. Usage:
python3 gen2.py [seed]"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))


def engine_book():
    """Four cylinders (phase offsets 0 / .25 / .5 / .75, each a 1/(1+k x^2)
    pulse, jittered by slow noise), three transmission overtones, through a
    warping exhaust waveguide (two cross-fed delays whose times the engine
    signal modulates), hp 200 twice, tanh."""
    return [
        {"type": "ramp", "name": "rpm", "from": 1200, "to": 4800, "time": 2.0, "curve": "exp"},
        # firing phase: 4 cylinders, 4-stroke = rpm / 60 * 2 Hz per crank cycle / 4 pulses
        {"type": "expr", "name": "ph", "inputs": {"r": "rpm"}, "state": {"p": 0},
         "out": "p", "update": {"p": "fract(p + r / 60 / 2 / sr)"}},
        {"type": "noise", "name": "jn", "color": "white"},
        {"type": "filter", "name": "jl", "input": "jn", "mode": "lp", "freq": 20, "slope": 24},
        {"type": "gain", "name": "jit", "input": "jl", "gain": 6},
        {"type": "expr", "name": "cyl", "inputs": {"p": "ph", "j": "jit", "r": "rpm"},
         "out": ("(1 / (1 + 300 * pow(fract(p + 0.00 + 0.010 * j) - 0.5, 2)) "
                 "+ 0.9 / (1 + 300 * pow(fract(p + 0.25 - 0.012 * j) - 0.5, 2)) "
                 "+ 1.1 / (1 + 300 * pow(fract(p + 0.50 + 0.008 * j) - 0.5, 2)) "
                 "+ 0.95 / (1 + 300 * pow(fract(p + 0.75 - 0.009 * j) - 0.5, 2))) * 0.5 - 0.1")},
        # transmission overtones x16, x4, x8 of the crank phase, delayed
        {"type": "expr", "name": "ot", "inputs": {"p": "ph"},
         "out": "0.12 * cos(2 * pi * fract(p * 16 + 0.3)) + 0.2 * (1 / (1 + 40 * pow(fract(p * 4 + 0.1) - 0.5, 2))) + 0.1 * cos(2 * pi * fract(p * 8 + 0.7))"},
        # combustion noise riding the cylinder pulses (the book's noise-modulated pulses)
        {"type": "filter", "name": "cn", "input": "jn", "mode": "bp", "freq": 1800, "q": 0.7},
        {"type": "math", "name": "cnp", "op": "mul", "a": "cn", "b": "cyl"},
        {"type": "mix", "name": "src", "inputs": [{"input": "cyl", "gain": 1.0}, {"input": "ot", "gain": 0.6}, {"input": "cnp", "gain": 0.5}]},
        {"type": "onepole", "name": "src_dc", "input": "src", "mode": "hp", "freq": 30},
        # warping exhaust: delay times wobble with the engine signal
        {"type": "math", "name": "w1", "op": "mul", "a": "src_dc", "b": 0.0015},
        {"type": "math", "name": "t1", "op": "add", "a": "w1", "b": 0.0061},
        {"type": "math", "name": "w2", "op": "mul", "a": "src_dc", "b": -0.0012},
        {"type": "math", "name": "t2", "op": "add", "a": "w2", "b": 0.0093},
        {"type": "delay", "name": "e1", "input": "src_dc", "time": "t1", "max": 0.02, "feedback": 0.55},
        {"type": "delay", "name": "e2", "input": "src_dc", "time": "t2", "max": 0.02, "feedback": 0.5},
        {"type": "mix", "name": "pipe", "inputs": ["src_dc", {"input": "e1", "gain": 0.8}, {"input": "e2", "gain": 0.8}]},
        {"type": "filter", "name": "hp", "input": "pipe", "mode": "hp", "freq": 120, "slope": 24},
        {"type": "shape", "name": "sat", "input": "hp", "mode": "tanh", "drive": 2.5, "oversample": 4},
        {"type": "filter", "name": "lp", "input": "sat", "mode": "lp", "freq": 5000},
        {"type": "output", "name": "out", "input": "lp", "lufs": -20},
    ]


def gunshot_book():
    """Shell chirp (64^x wrapped cos, 10 kHz -> a few hundred Hz in 60 ms),
    barrel wave (4 cycles, 100-200 Hz) 1.5 ms later, a noise burst (three
    squarings, 200 ms), through an 8-band body (Q 1) and tanh; an N-wave
    crack with its ground reflection."""
    bands = [2225, 647, 1278, 1329, 2615, 500, 1682, 2243]
    nodes = [
        {"type": "expr", "name": "shell", "inputs": {},
         "out": "t < 0.003 ? 0 : (t > 0.063 ? 0 : cos(2 * pi * (fract(pow(64, 1.05 * (1 - (t - 0.003) / 0.06))) - 0.25)))"},
        {"type": "expr", "name": "barrel", "inputs": {},
         "out": "t < 0.0045 ? 0 : (t > 0.0345 ? 0 : (1 - (t - 0.0045) / 0.03) * cos(2 * pi * (fract(pow(2, 2 * (1 - (t - 0.0045) / 0.03))) - 0.25)))"},
        {"type": "noise", "name": "n", "color": "white"},
        {"type": "expr", "name": "ne", "inputs": {},
         "out": "t < 0.003 ? 0 : pow(max(1 - (t - 0.003) / 0.2, 0), 8)"},
        {"type": "math", "name": "nb", "op": "mul", "a": "n", "b": "ne"},
        {"type": "mix", "name": "exc", "inputs": [{"input": "shell", "gain": 0.6}, {"input": "barrel", "gain": 1.0}, {"input": "nb", "gain": 0.8}]},
    ]
    for i, f in enumerate(bands):
        nodes.append({"type": "filter", "name": f"b{i}", "input": "exc", "mode": "bp", "freq": f, "q": 1})
    nodes += [
        {"type": "mix", "name": "body", "inputs": [f"b{i}" for i in range(len(bands))]},
        {"type": "gain", "name": "drv", "input": "body", "gain": 2.5},
        {"type": "shape", "name": "sat", "input": "drv", "mode": "tanh", "oversample": 4},
        # N-wave crack (+1 in 1 ms, -1 1 ms later) and its ground reflection
        {"type": "env", "name": "nw", "points": [[0, 0], [0.002, 0], [0.0025, 1], [0.0035, -1], [0.004, 0]]},
        {"type": "delay", "name": "nwr", "input": "nw", "time": 0.0025},
        {"type": "onepole", "name": "nwl", "input": "nwr", "mode": "lp", "freq": 100},
        {"type": "mix", "name": "shock", "inputs": ["nw", {"input": "nwl", "gain": 8}]},
        {"type": "mix", "name": "all", "inputs": [{"input": "sat", "gain": 0.8}, {"input": "shock", "gain": 0.35}]},
        {"type": "onepole", "name": "dc", "input": "all", "mode": "hp", "freq": 20},
        {"type": "output", "name": "out", "input": "dc", "peak_db": -1, "fade_in": 0.0005},
    ]
    return nodes


def explosion_book():
    """Shockwave chirp (5 cycles 500 Hz -> 0 in 10 ms), three harsh noise
    blasts (30 ms, swept from 7 kHz, clipped), and an 8 s fireball rumble
    (noise low-passed near 70 Hz, swept 40 -> 140 Hz and back, quartic fall)."""
    nodes = [
        {"type": "expr", "name": "shock", "inputs": {},
         "out": "t < 0.005 ? 0 : (t > 0.015 ? 0 : cos(2 * pi * fract(5 * pow(1 - (t - 0.005) / 0.01, 2))))"},
        {"type": "onepole", "name": "shock_dc", "input": "shock", "mode": "hp", "freq": 5},
        {"type": "noise", "name": "n", "color": "white"},
    ]
    blasts = []
    for k, (at, bright) in enumerate([(0.035, 1.0), (0.072, 0.7), (0.093, 0.5)]):
        p = f"bl{k}_"
        f0 = 7000 * bright * bright
        nodes += [
            {"type": "ramp", "name": p + "f", "from": f0, "to": 200, "time": 0.03, "curve": "exp"},
            {"type": "filter", "name": p + "lp", "input": "n", "mode": "lp", "freq": p + "f", "q": 1.5 + 3.5 * (1 - bright)},
            {"type": "gain", "name": p + "g", "input": p + "lp", "gain": 50},
            {"type": "shape", "name": p + "c", "input": p + "g", "mode": "clip"},
            {"type": "env", "name": p + "e", "points": [[0, 0], [at, 0], [at + 0.001, 1], [at + 0.03, 0]]},
            {"type": "math", "name": p + "x", "op": "mul", "a": p + "c", "b": p + "e"},
        ]
        blasts.append({"input": p + "x", "gain": bright})
    nodes += [
        {"type": "mix", "name": "blast_raw", "inputs": blasts},
        {"type": "filter", "name": "blast", "input": "blast_raw", "mode": "bp", "freq": 3000, "q": 0.5},
        # fireball
        {"type": "env", "name": "fb_f", "points": [[0, 40], [0.1, 40], [0.6, 140], [6.0, 40]], "curve": "smooth"},
        {"type": "filter", "name": "fb_bp", "input": "n", "mode": "bp", "freq": 70, "q": 1},
        {"type": "filter", "name": "fb_lp", "input": "fb_bp", "mode": "lp", "freq": "fb_f", "slope": 24},
        {"type": "expr", "name": "fb_e", "inputs": {},
         "out": "t < 0.1 ? 0 : (t < 0.2 ? (t - 0.1) / 0.1 : pow(max(1 - (t - 0.2) / 6.0, 0), 4))"},
        {"type": "math", "name": "fb_x", "op": "mul", "a": "fb_lp", "b": "fb_e"},
        {"type": "gain", "name": "fb", "input": "fb_x", "gain": 10},
        {"type": "onepole", "name": "fb_dc", "input": "fb", "mode": "hp", "freq": 15},
        {"type": "mix", "name": "all", "inputs": [{"input": "shock_dc", "gain": 0.8}, {"input": "blast", "gain": 4.0}, {"input": "fb_dc", "gain": 1.0}]},
        {"type": "shape", "name": "sat", "input": "all", "mode": "tanh", "drive": 1.5, "oversample": 4},
        {"type": "output", "name": "out", "input": "sat", "peak_db": -1},
    ]
    return nodes


SOUNDS = {
    "fz_engine_book": (engine_book(), 2.5, "engine_set"),
    "fz_engine_node": ([{"type": "ramp", "name": "rpm", "from": 1200, "to": 4800, "time": 2.0, "curve": "exp"},
                        {"type": "engine", "name": "e", "rpm": "rpm", "cylinders": 4, "load": 0.6},
                        {"type": "output", "name": "out", "input": "e", "lufs": -20}], 2.5, "engine_set"),
    "fz_gun_book": (gunshot_book(), 1.0, None),
    "fz_gun_node": ([{"type": "gunshot", "name": "g", "caliber": 9},
                     {"type": "output", "name": "out", "input": "g", "peak_db": -1}], 1.0, None),
    "fz_boom_book": (explosion_book(), 6.0, None),
    "fz_boom_node": ([{"type": "explosion", "name": "x", "size": 2, "distance": 50},
                      {"type": "output", "name": "out", "input": "x", "peak_db": -1}], 6.0, None),
}

KEEP = ("length", "level", "time", "energy shares", "pitch", "events")


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    only = sys.argv[2:]
    calls = []
    for name, (ops, dur, ref) in SOUNDS.items():
        if only and not any(o in name for o in only):
            continue
        calls += [{"name": "sound_ops", "args": {"sound": name, "ops": ops, "seed": seed, "duration": dur}},
                  {"name": "export", "args": {"doc": name, "format": "wav"}},
                  {"name": "listen", "args": {"doc": name}}]
        if ref:
            calls.append({"name": "verify", "args": {"doc": name, "expect": {"reference_set": {"set": ref}}}})
    path = os.path.join(HERE, "calls.json")
    json.dump(calls, open(path, "w"))
    p = subprocess.run([sys.executable, "mcp_driver.py", "--client", "farnell", "--calls", path],
                       cwd=ROOT, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{"):
            j = json.loads(l)
            txt = " ".join(map(str, j.get("text", [])))
            keep = [s for s in txt.splitlines() if s.split(":")[0].strip() in KEEP
                    or "reference_set" in s or s.startswith("sound") or "error" in s.lower()]
            print(("OK  " if j.get("ok") else "ERR ") + ("\n    ".join(keep) if keep else txt[:400]))


if __name__ == "__main__":
    main()
