"""Farnell-style rebuilds (see notes.md) as sounder graphs, rendered next to
the current ready-made node for an A/B listen. Usage: python3 gen.py [seed]"""
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))


def firegen(k, crackle_rate):
    p = f"g{k}_"
    return [
        # lapping: 30 Hz resonant swell, clipped
        {"type": "noise", "name": p + "ln", "color": "white", "seed": 10 + k},
        {"type": "filter", "name": p + "lbp", "input": p + "ln", "mode": "bp", "freq": 30, "q": 5},
        {"type": "gain", "name": p + "lg", "input": p + "lbp", "gain": 100},
        {"type": "onepole", "name": p + "lh1", "input": p + "lg", "mode": "hp", "freq": 25},
        {"type": "shape", "name": p + "lc", "input": p + "lh1", "mode": "clip", "drive": 1.1},
        {"type": "onepole", "name": p + "lh2", "input": p + "lc", "mode": "hp", "freq": 25},
        {"type": "gain", "name": p + "lap", "input": p + "lh2", "gain": 0.6},
        # hiss: high noise under a slow squared random envelope
        {"type": "noise", "name": p + "hn", "color": "white", "seed": 20 + k},
        {"type": "filter", "name": p + "hh", "input": p + "hn", "mode": "hp", "freq": 1500},
        {"type": "wander", "name": p + "hw", "rate": 4, "range": [0, 1], "seed": 30 + k},
        {"type": "math", "name": p + "hw2", "op": "mul", "a": p + "hw", "b": p + "hw"},
        {"type": "math", "name": p + "hiss", "op": "mul", "a": p + "hh", "b": p + "hw2"},
        # crackle: rare pops, length 2-30 ms, centre 1.5-16.5 kHz with length
        {"type": "poisson", "name": p + "pe", "rate": crackle_rate, "seed": 40 + k},
        {"type": "voices", "name": p + "crk", "events": p + "pe", "length": 0.04, "max": 400,
         "map": {"d": "0.002 + $rand * 0.028", "f": "1500 + (0.002 + $rand * 0.028) * 500000"},
         "patch": [
             {"type": "noise", "name": "n", "color": "white"},
             {"type": "ramp", "name": "r", "from": 1, "to": 0, "time": "$d", "curve": "linear"},
             {"type": "math", "name": "r2", "op": "mul", "a": "r", "b": "r"},
             {"type": "math", "name": "r4", "op": "mul", "a": "r2", "b": "r2"},
             {"type": "math", "name": "x", "op": "mul", "a": "n", "b": "r4"},
             {"type": "filter", "name": "bp", "input": "x", "mode": "bp", "freq": "$f", "q": 1}]},
        {"type": "mix", "name": p + "sum", "inputs": [p + "lap", {"input": p + "hiss", "gain": 0.15}, {"input": p + "crk", "gain": 1.5}]},
    ]


def fire_all(rate=6):
    nodes = []
    for k in range(4):
        nodes += firegen(k, rate)
    nodes += [
        {"type": "filter", "name": "b600", "input": "g0_sum", "mode": "bp", "freq": 600, "q": 0.2},
        {"type": "filter", "name": "b1200", "input": "g1_sum", "mode": "bp", "freq": 1200, "q": 0.6},
        {"type": "filter", "name": "b2600", "input": "g2_sum", "mode": "bp", "freq": 2600, "q": 0.4},
        {"type": "filter", "name": "h1000", "input": "g3_sum", "mode": "hp", "freq": 1000},
        {"type": "mix", "name": "fire", "inputs": ["b600", "b1200", "b2600", "h1000"], "to": "mono"},
        {"type": "output", "name": "out", "input": "fire", "lufs": -24},
    ]
    return nodes


def splash_book():
    """Ch. 35/38: an impact bump then a cloud of bubbles whose pitch RISES;
    size s sets length (longer), pitch (lower) and level (louder)."""
    return [
        # impact: a positive pressure bump plus the slap of the surface
        {"type": "env", "name": "bump_e", "points": [[0, 0], [0.004, 0], [0.007, 1], [0.012, 0]], "curve": "smooth"},
        {"type": "onepole", "name": "bump", "input": "bump_e", "mode": "hp", "freq": 40},
        {"type": "noise", "name": "sn", "color": "white"},
        {"type": "decay", "name": "slap_e", "time": 0.06, "at": 0.005},
        {"type": "math", "name": "slap_r", "op": "mul", "a": "sn", "b": "slap_e"},
        {"type": "filter", "name": "slap", "input": "slap_r", "mode": "bp", "freq": 3500, "q": 0.7},
        {"type": "decay", "name": "spray_e", "time": 0.35, "at": 0.008},
        {"type": "math", "name": "spray_r", "op": "mul", "a": "sn", "b": "spray_e"},
        {"type": "filter", "name": "spray", "input": "spray_r", "mode": "bp", "freq": 3200, "q": 0.6},
        # bubbles: many small ones early, a few big ones later
        {"type": "burst", "name": "be", "count": 140, "spread": 0.5, "curve": "exp", "at": 0.01, "values": {"min": 0.05, "max": 1}},
        {"type": "voices", "name": "bub", "events": "be", "length": 0.2, "max": 200,
         "map": {"s": "$value * (0.6 + 0.4 * $rand)", "f": "350 + (1 - $value) * 2600 * (0.8 + 0.4 * $rand)",
                 "dur": "0.02 + 0.09 * $value"},
         "patch": [
             {"type": "ramp", "name": "rise", "from": "$f", "to": "=$f * 1.7", "time": "$dur", "curve": "exp"},
             {"type": "osc", "name": "o", "freq": "rise"},
             {"type": "ramp", "name": "att", "from": 0, "to": 1, "time": "=$dur * 0.15", "curve": "linear"},
             {"type": "decay", "name": "dec", "time": "=$dur * 1.6"},
             {"type": "math", "name": "env", "op": "mul", "a": "att", "b": "dec"},
             {"type": "math", "name": "x", "op": "mul", "a": "o", "b": "env"},
             {"type": "gain", "name": "g", "input": "x", "gain": "=0.1 + $s"}]},
        {"type": "onepole", "name": "bub_h", "input": "bub", "mode": "hp", "freq": 60},
        {"type": "mix", "name": "dry", "inputs": [{"input": "bump", "gain": 0.9}, {"input": "slap", "gain": 1.4}, {"input": "spray", "gain": 0.6}, {"input": "bub_h", "gain": 0.07}], "to": "mono"},
        {"type": "output", "name": "out", "input": "dry", "peak_db": -3},
    ]


def spark_book():
    """Ch. 39: beating 100 Hz hum, chirp-pulse arcing under a random gate,
    and snaps through the spark formant bank, all into a short comb."""
    return [
        {"type": "osc", "name": "s1", "shape": "saw", "freq": 99.8, "amp": 0.5},
        {"type": "osc", "name": "s2", "shape": "saw", "freq": 100.2, "amp": 0.5},
        {"type": "mix", "name": "hs", "inputs": ["s1", "s2"]},
        {"type": "shape", "name": "hc", "input": "hs", "mode": "clip", "drive": 4},
        {"type": "wander", "name": "hm", "rate": 2, "range": [0, 1]},
        {"type": "math", "name": "hm2", "op": "mul", "a": "hm", "b": "hm"},
        {"type": "math", "name": "hum", "op": "mul", "a": "hc", "b": "hm2"},
        # arcing: a reversed phasor squashed by a random factor, squared, cos
        {"type": "wander", "name": "k", "rate": 0.1, "range": [3, 12]},
        {"type": "osc", "name": "ph", "shape": "saw", "freq": 99.8},
        {"type": "expr", "name": "chirp", "inputs": {"p": "ph", "k": "k"},
         "out": "cos(2 * pi * 12 * pow(min((1 - p) * 0.5 * k, 1) + 0.1, 2))"},
        {"type": "filter", "name": "arc_h", "input": "chirp", "mode": "hp", "freq": 300},
        {"type": "wander", "name": "gw", "rate": 3, "range": [-1, 1]},
        {"type": "expr", "name": "gsq", "inputs": {"w": "gw"}, "out": "w > 0.2 ? 1 : 0"},
        {"type": "onepole", "name": "gate", "input": "gsq", "mode": "lp", "freq": 30},
        {"type": "math", "name": "arc", "op": "mul", "a": "arc_h", "b": "gate"},
        # snaps
        {"type": "poisson", "name": "se", "rate": 0.8},
        {"type": "voices", "name": "snap", "events": "se", "length": 0.6, "max": 32, "patch": [
            {"type": "noise", "name": "n", "color": "white"},
            {"type": "ramp", "name": "l", "from": 1, "to": 0, "time": 0.5, "curve": "linear"},
            {"type": "math", "name": "l2", "op": "mul", "a": "l", "b": "l"},
            {"type": "math", "name": "l4", "op": "mul", "a": "l2", "b": "l2"},
            {"type": "math", "name": "nb", "op": "mul", "a": "n", "b": "l4"},
            {"type": "ramp", "name": "sw", "from": 7020, "to": 20, "time": 0.01, "curve": "linear"},
            {"type": "osc", "name": "so", "freq": "sw"},
            {"type": "decay", "name": "sd", "time": 0.03},
            {"type": "math", "name": "sb", "op": "mul", "a": "so", "b": "sd"},
            {"type": "mix", "name": "ex", "inputs": [{"input": "nb", "gain": 0.5}, "sb"]},
            {"type": "filter", "name": "f1", "input": "ex", "mode": "bp", "freq": 4600, "q": 5},
            {"type": "filter", "name": "f2", "input": "ex", "mode": "bp", "freq": 7200, "q": 5},
            {"type": "filter", "name": "f3", "input": "ex", "mode": "bp", "freq": 480, "q": 7},
            {"type": "filter", "name": "f4", "input": "ex", "mode": "bp", "freq": 720, "q": 8},
            {"type": "mix", "name": "fb", "inputs": [{"input": "f1", "gain": 2}, "f2", {"input": "f3", "gain": 1.2}, {"input": "f4", "gain": 2.5}]},
            {"type": "filter", "name": "o", "input": "fb", "mode": "bp", "freq": 2500, "q": 0.5}]},
        {"type": "mix", "name": "all", "inputs": [{"input": "hum", "gain": 0.25}, {"input": "arc", "gain": 0.3}, {"input": "snap", "gain": 3}], "to": "mono"},
        {"type": "comb", "name": "box", "input": "all", "time": 0.012, "feedback": 0.2, "damp": 0.3},
        {"type": "output", "name": "out", "input": "box", "lufs": -24},
    ]


SOUNDS = {
    "fz_splash_book": (splash_book(), 1.2),
    "fz_splash_node": ([{"type": "splash", "name": "s", "size": 0.2},
                        {"type": "output", "name": "out", "input": "s", "peak_db": -3}], 1.2),
    "fz_spark_book": (spark_book(), 4.0),
    "fz_spark_node": ([{"type": "spark", "name": "s", "rate": 10},
                       {"type": "output", "name": "out", "input": "s", "lufs": -24}], 4.0),
    "fz_fire_book": (fire_all(), 4.0),
    "fz_fire_node": ([{"type": "fire", "name": "f", "size": 1.0, "intensity": 0.5, "crackle": 0.5},
                      {"type": "output", "name": "out", "input": "f", "lufs": -24}], 4.0),
}


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    calls = []
    for name, (ops, dur) in SOUNDS.items():
        calls += [{"name": "sound_ops", "args": {"sound": name, "ops": ops, "seed": seed, "duration": dur}},
                  {"name": "export", "args": {"doc": name, "format": "wav"}},
                  {"name": "verify", "args": {"doc": name, "only": ["finite", "clicks"]}},
                  {"name": "listen", "args": {"doc": name}}]
        if "splash" in name:
            calls.append({"name": "verify", "args": {"doc": name, "expect": {"reference_set": {"set": "splash_set"}}}})
    path = os.path.join(HERE, "calls.json")
    json.dump(calls, open(path, "w"))
    p = subprocess.run([sys.executable, "mcp_driver.py", "--client", "farnell", "--calls", path],
                       cwd=ROOT, capture_output=True, text=True)
    for l in p.stdout.splitlines():
        if l.startswith("{"):
            j = json.loads(l)
            txt = " ".join(map(str, j.get("text", [])))
            keep = [s for s in txt.splitlines() if s.split(":")[0].strip() in
                    ("length", "level", "time", "spectrum whole", "energy shares", "density", "pitch", "events")
                    or "PASS" in s or "FAIL" in s or s.startswith("sound") or "reference_set" in s]
            print(("OK  " if j.get("ok") else "ERR ") + ("\n    ".join(keep) if keep else txt[:300]))


if __name__ == "__main__":
    main()
