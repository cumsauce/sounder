"""Third Farnell set: motor (ch. 44), fan / ventilation (ch. 46), gravel
footsteps (ch. 49), clock (ch. 43), each rendered beside the current node
where one exists. Usage: python3 gen3.py [seed] [name filters...]"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))


def motor_book(T=4.0):
    """Speed envelope (fast 1-(1-x)^6 rise, linear fall); rotor = noisy
    clicking spikes (bp 4 kHz noise plus DC, times the quartic decay of each
    phasor cycle); stator = a 1/(x^2+1) pulse at 4x; FM body resonance
    (a fixed 178 Hz carrier phase-modulated by the motor, hp 180 twice)."""
    return [
        {"type": "noise", "name": "n", "color": "white"},
        {"type": "filter", "name": "nb", "input": "n", "mode": "bp", "freq": 4000, "q": 1},
        {"type": "expr", "name": "m", "inputs": {"nb": "nb"}, "state": {"ph": 0},
         "out": (f"(t < {T} ? 1 : 0) * (2 * t / {T} < 1 ? 1 - pow(1 - 2 * t / {T}, 6) : 2 - 2 * t / {T}) * ("
                 "0.1 * (2.0 * nb + 0.2) * pow(1 - ph, 4) "
                 "+ 0.3 * (1 / (1 + 16 * pow(cos(2 * pi * fract(2 * ph)), 2)) - 0.3))"),
         "update": {"ph": f"fract(ph + 110 * (2 * t / {T} < 1 ? 1 - pow(1 - 2 * t / {T}, 6) : max(2 - 2 * t / {T}, 0)) / sr)"}},
        {"type": "expr", "name": "fm", "inputs": {"m": "m"}, "out": "cos(2 * pi * 178 * t + 6 * m)"},
        {"type": "filter", "name": "fmh", "input": "fm", "mode": "hp", "freq": 180, "slope": 24},
        {"type": "expr", "name": "fe", "inputs": {"m": "m"}, "state": {"y": 0}, "out": "y",
         "update": {"y": "y + 0.002 * (abs(m) - y)"}},
        {"type": "math", "name": "fmb", "op": "mul", "a": "fmh", "b": "fe"},
        {"type": "mix", "name": "all", "inputs": ["m", {"input": "fmb", "gain": 0.5}]},
        {"type": "onepole", "name": "dc", "input": "all", "mode": "hp", "freq": 20},
        {"type": "output", "name": "out", "input": "dc", "lufs": -22},
    ]


def fan_book():
    """Ventilation: a fan (24 pulses/s from a 12 Hz osc, 1/(1+(5 cos)^2),
    noise bp 700 Hz modulated by the pulse) spun up by a motor hum, through
    a pipe (two feedback delays, 22 ms x0.3 and 70 ms x0.499)."""
    return [
        {"type": "ramp", "name": "sp", "from": 0.05, "to": 1, "time": 1.2, "curve": "exp"},
        {"type": "expr", "name": "ph", "inputs": {"s": "sp"}, "state": {"p": 0}, "out": "p",
         "update": {"p": "fract(p + 30 * s / sr)"}},
        {"type": "expr", "name": "pulse", "inputs": {"p": "ph"},
         "out": "1 / (1 + pow(4 * cos(2 * pi * p), 2))"},
        {"type": "noise", "name": "n", "color": "white"},
        {"type": "filter", "name": "nb", "input": "n", "mode": "bp", "freq": 1500, "q": 1},
        {"type": "math", "name": "nm", "op": "mul", "a": "nb", "b": "pulse"},
        {"type": "expr", "name": "hum", "inputs": {"p": "ph", "s": "sp"},
         "out": "s * (0.05 * sin(2 * pi * fract(8 * p)) + 0.03 * pow(1 - fract(8 * p), 4))"},
        {"type": "mix", "name": "fan", "inputs": [{"input": "pulse", "gain": 0.1}, {"input": "nm", "gain": 0.4}, {"input": "hum", "gain": 0.2}]},
        {"type": "math", "name": "fan_s", "op": "mul", "a": "fan", "b": "sp"},
        {"type": "onepole", "name": "fdc", "input": "fan_s", "mode": "hp", "freq": 20},
        {"type": "delay", "name": "pa", "input": "fdc", "time": 0.009, "feedback": 0.3},
        {"type": "delay", "name": "pb", "input": "fdc", "time": 0.023, "feedback": 0.4},
        {"type": "mix", "name": "all", "inputs": ["fdc", {"input": "pa", "gain": 0.5}, {"input": "pb", "gain": 0.5}]},
        {"type": "output", "name": "out", "input": "all", "lufs": -24},
    ]


def gravel_book():
    """Two steps. Each step's ground-reaction force is three polynomial
    pulses (heel, ball, toe roll) 1.5 n u (1-u)(1-u^2); gravel = spikes from
    the ratio of two low-passed noises, high-passed and squared, through a
    band-pass whose centre a 50 Hz noise plus the force moves (500 Hz-10
    kHz), times the force."""
    def grf(t0, d):
        x = f"min(max((t - {t0}) / {d}, 0), 1)"
        poly = lambda u: f"(1.5 * 3.333 * ({u}) * (1 - ({u})) * (1 - ({u}) * ({u})))"
        heel = poly(f"min({x} * 3, 1)")
        ball = poly(f"min(max({x} - 0.125, 0) * 1.333, 1)")
        roll = poly(f"min(max({x} - 0.667, 0) * 3, 1)")
        return f"({heel} + 0.8 * {ball} + 0.5 * {roll})"
    return [
        {"type": "expr", "name": "grf", "inputs": {},
         "out": f"min(({grf(0.02, 0.5)} + {grf(0.62, 0.45)}) * 0.6, 1)"},
        {"type": "noise", "name": "n1", "color": "white"},
        {"type": "noise", "name": "n2", "color": "white", "seed": 7},
        {"type": "onepole", "name": "a", "input": "n1", "mode": "lp", "freq": 300},
        {"type": "onepole", "name": "b", "input": "n1", "mode": "lp", "freq": 2000},
        {"type": "expr", "name": "ratio", "inputs": {"a": "a", "b": "b"},
         "out": "clamp(a / (abs(b) < 0.0005 ? 0.0005 : b), -50, 50)"},
        {"type": "onepole", "name": "rh", "input": "ratio", "mode": "hp", "freq": 400},
        # crunch on impact: the force's rise (stones giving way) drives the
        # spikes, not the force itself, so they bunch at heel and toe strikes
        {"type": "expr", "name": "dg", "inputs": {"g": "grf"}, "state": {"p": 0, "y": 0}, "out": "y",
         "update": {"y": "y + 0.02 * (max(g - p, 0) * sr * 0.05 - y)", "p": "g"}},
        {"type": "expr", "name": "spk", "inputs": {"x": "rh", "g": "grf", "d": "dg"},
         "out": "clamp(x * x * 0.02 * (0.05 + 6 * d + 0.5 * g), -0.9, 0.9)"},
        {"type": "filter", "name": "mlp", "input": "n2", "mode": "lp", "freq": 50, "slope": 24},
        {"type": "expr", "name": "fc", "inputs": {"m": "mlp", "g": "grf"},
         "out": "clamp(abs(m) * 20000 + 1500 + 3000 * g, 800, 7000)"},
        {"type": "filter", "name": "vcf", "input": "spk", "mode": "bp", "freq": "fc", "q": 1.2},
        {"type": "filter", "name": "hp", "input": "vcf", "mode": "hp", "freq": 200},
        {"type": "expr", "name": "crunch", "inputs": {"x": "hp", "g": "grf", "d": "dg"}, "out": "x * min(g + 4 * d, 1.5)"},
        # a soft thud under the crunch, the force on the ground (not in the book)
        {"type": "noise", "name": "n3", "color": "brown", "seed": 9},
        {"type": "filter", "name": "tl", "input": "n3", "mode": "lp", "freq": 180, "slope": 24},
        {"type": "math", "name": "thud", "op": "mul", "a": "tl", "b": "grf"},
        {"type": "mix", "name": "step", "inputs": ["crunch", {"input": "thud", "gain": 0.6}]},
        {"type": "output", "name": "out", "input": "step", "peak_db": -3},
    ]


def clock_book():
    """Metro 125 ms: tick 1 (clusters in bands 6543 / 3345 / 1356 Hz) on
    beats 1 and 5, tick 2 (7543 / 3988 / 2765 Hz) on 3 and 7, an escapement
    (sines alternating 10 / 8 kHz, short square decays) on every beat, a
    lower hand clunk once a second; a box body (two delays ~10 ms, bp 1.2
    kHz Q 3, feedback 0.3) in parallel."""
    def tick(name, beats, bands, sub):
        ts = [b * 0.125 + 0.005 + s for b in beats for s in sub]
        nodes = [{"type": "pattern", "name": name + "_ev", "times": ts}]
        patch = [{"type": "noise", "name": "n", "color": "white"},
                 {"type": "onepole", "name": "h", "input": "n", "mode": "hp", "freq": 200}]
        mix = []
        for i, (f, d) in enumerate(bands):
            patch += [{"type": "decay", "name": f"e{i}", "time": d},
                      {"type": "math", "name": f"x{i}", "op": "mul", "a": "h", "b": f"e{i}"},
                      {"type": "filter", "name": f"f{i}", "input": f"x{i}", "mode": "bp", "freq": f, "q": 30}]
            mix.append(f"f{i}")
        patch += [{"type": "mix", "name": "m", "inputs": mix}, {"type": "gain", "name": "g", "input": "m", "gain": 3}]
        nodes.append({"type": "voices", "name": name, "events": name + "_ev", "length": 0.06, "max": 64, "patch": patch})
        return nodes
    beats = range(16)
    nodes = []
    nodes += tick("t1", [b for b in beats if b % 8 in (0, 4)], [(6543, 0.03), (3345, 0.02), (1356, 0.01)], [0, 0.004, 0.011])
    nodes += tick("t2", [b for b in beats if b % 8 in (2, 6)], [(7543, 0.03), (3988, 0.02), (2765, 0.01)], [0, 0.006, 0.009])
    nodes += tick("hand", [0, 8], [(1200, 0.05), (640, 0.04), (310, 0.03)], [0.02, 0.026])
    esc = [b * 0.125 + 0.003 for b in beats]
    nodes += [
        {"type": "pattern", "name": "esc_ev", "times": esc},
        {"type": "voices", "name": "esc", "events": "esc_ev", "length": 0.03, "max": 32,
         "map": {"f": "$index - 2 * floor($index / 2) < 1 ? 10000 : 8000"},
         "patch": [{"type": "osc", "name": "o", "freq": "$f"},
                   {"type": "env", "name": "e", "points": [[0, 0], [0.0005, 1], [0.01, 1], [0.0105, 0]]},
                   {"type": "math", "name": "x", "op": "mul", "a": "o", "b": "e"}]},
        {"type": "mix", "name": "dry", "inputs": [{"input": "t1", "gain": 0.1}, {"input": "t2", "gain": 0.1},
                                                  {"input": "hand", "gain": 0.3}, {"input": "esc", "gain": 0.06}]},
        {"type": "filter", "name": "bb1", "input": "dry", "mode": "bp", "freq": 1200, "q": 3},
        {"type": "delay", "name": "bd1", "input": "bb1", "time": 0.01, "feedback": 0.3},
        {"type": "filter", "name": "bb2", "input": "dry", "mode": "bp", "freq": 1150, "q": 3},
        {"type": "delay", "name": "bd2", "input": "bb2", "time": 0.0102, "feedback": 0.3},
        {"type": "mix", "name": "all", "inputs": ["dry", {"input": "bd1", "gain": 0.6}, {"input": "bd2", "gain": 0.6}]},
        {"type": "output", "name": "out", "input": "all", "peak_db": -3},
    ]
    return nodes


SOUNDS = {
    "fz_motor_book": (motor_book(), 4.0),
    "fz_motor_node": ([{"type": "env", "name": "r", "points": [[0, 0], [1.2, 6000], [2.2, 6000], [4.0, 0]], "curve": "smooth"},
                       {"type": "motor", "name": "m", "rpm": "r"},
                       {"type": "output", "name": "out", "input": "m", "lufs": -22}], 4.0),
    "fz_fan_book": (fan_book(), 3.0),
    "fz_gravel_book": (gravel_book(), 1.2),
    "fz_gravel_node": ([{"type": "footstep", "name": "a", "surface": "gravel", "at": 0.02},
                        {"type": "footstep", "name": "b", "surface": "gravel", "at": 0.62, "seed": 2},
                        {"type": "mix", "name": "m", "inputs": ["a", "b"]},
                        {"type": "output", "name": "out", "input": "m", "peak_db": -3}], 1.2),
    "fz_clock_book": (clock_book(), 2.0),
    "fz_gun_fit2": ([{"type": "gunshot", "name": "g", "caliber": 9},
                     {"type": "filter", "name": "lp", "input": "g", "mode": "lp", "freq": 5500},
                     {"type": "shape", "name": "s", "input": "lp", "mode": "tanh", "drive": 6, "oversample": 4},
                     {"type": "mix", "name": "m", "inputs": ["s", {"input": "g", "gain": 0.25}]},
                     {"type": "output", "name": "out", "input": "m", "peak_db": -1}], 1.5),
}
KEEP = ("length", "level", "time", "energy shares", "pitch", "events", "density")


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    only = sys.argv[2:]
    calls = []
    for name, (ops, dur) in SOUNDS.items():
        if only and not any(o in name for o in only):
            continue
        calls += [{"name": "sound_ops", "args": {"sound": name, "ops": ops, "seed": seed, "duration": dur}},
                  {"name": "export", "args": {"doc": name, "format": "wav"}},
                  {"name": "verify", "args": {"doc": name, "only": ["finite", "clicks"]}},
                  {"name": "listen", "args": {"doc": name}}]
        if "gun" in name:
            calls.append({"name": "verify", "args": {"doc": name, "seeds": 2, "expect": {"reference_set": {"set": "gun_set"}}}})
    path = os.path.join(HERE, "calls.json")
    json.dump(calls, open(path, "w"))
    p = subprocess.run([sys.executable, "mcp_driver.py", "--client", "farnell", "--calls", path],
                       cwd=ROOT, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{"):
            j = json.loads(l)
            txt = " ".join(map(str, j.get("text", [])))
            keep = [s for s in txt.splitlines() if s.split(":")[0].strip() in KEEP
                    or "FAIL" in s or "nearest" in s or s.startswith("sound") or "rror" in s]
            print(("OK  " if j.get("ok") else "ERR ") + ("\n    ".join(keep) if keep else txt[:400]))


if __name__ == "__main__":
    main()
