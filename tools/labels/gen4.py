"""Batch 4: metal and stone by synthesis, for blind labelling.

Every earlier metal build was heard as glass, ceramic or plastic. They rang
0.1-0.4 s on a few clean modes. Struck metal rings for seconds, its modes
come in split pairs that beat, its decay shortens with frequency, and a
dense shimmer of weak upper partials sits over the low ring. These cues use
hand-built `resonators` tables with exactly that. Stone is the other way
round: almost no ring, a very bright hard click, crunchy grit, short
band-limited noise.

Metal comes in two versions: A, hand-built `resonators` (clean split
pairs), dry only; B, `modal` with narrow noise bands, dry and dressed.
Stone is dry and dressed. Dressed is a small room, for listening only; under shuffled ids m01.. so the listener cannot tell
which is which. batch4.json maps ids to designs and graphs.

  python3 tools/labels/gen4.py <project dir>
"""
import json, os, random, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
rng = random.Random(20261120)

BAR = [1, 2.757, 5.404, 8.933, 13.344, 18.638, 24.81]
TUBE = [1, 2.828, 5.423, 8.771, 12.86, 17.69]


def plate_ratios(n):
    r = sorted({(a * a + b * b) / 2 for a in range(1, 9) for b in range(1, 9)})
    return [x / r[0] for x in r][:n]


def metal_modes(f0, ratios, t0, split=(0.0006, 0.004), hash_n=24, hash_band=(3000, 12000), tilt=-3.0, ceiling=16000):
    """Doublets on each ratio (beating), T60 falling as f^-0.6, plus weak
    short upper partials (the shimmer)."""
    out = []
    for k, r in enumerate(ratios):
        f = f0 * r * (1 + rng.uniform(-0.01, 0.01))
        if f > ceiling:
            break
        t = t0 * (f0 / f) ** 0.6
        g = 10 ** (tilt * (k / 6) / 20) * rng.uniform(0.7, 1.0)
        d = rng.uniform(*split)
        out.append([round(f, 2), round(t, 3), round(g, 3)])
        out.append([round(f * (1 + d), 2), round(t * rng.uniform(0.8, 1.1), 3), round(g * rng.uniform(0.5, 0.9), 3)])
    for _ in range(hash_n):
        f = rng.uniform(*hash_band)
        out.append([round(f, 1), round(rng.uniform(0.15, 0.6) * (t0 / 2) ** 0.5, 3), round(rng.uniform(0.04, 0.12), 3)])
    return out


def strike(name, at, burst=0.003, gain=1.0):
    return [{"type": "decay", "name": f"{name}_e", "time": burst, "at": at},
            {"type": "math", "name": f"{name}_x", "op": "mul", "a": "wn", "b": f"{name}_e"},
            {"type": "gain", "name": name, "input": f"{name}_x", "gain": gain}]


def crack(at, gain_db, hp=3000, t=0.006, name="crk"):
    return [{"type": "decay", "name": f"{name}_e", "time": t, "at": at},
            {"type": "math", "name": f"{name}_x", "op": "mul", "a": "wn", "b": f"{name}_e"},
            {"type": "filter", "name": f"{name}_f", "input": f"{name}_x", "mode": "hp", "freq": hp},
            {"type": "gain", "name": name, "input": f"{name}_f", "db": gain_db}]


def thud(at, gain_db, lp=250, t=0.12, name="thd"):
    return [{"type": "decay", "name": f"{name}_e", "time": t, "at": at},
            {"type": "math", "name": f"{name}_x", "op": "mul", "a": "bn", "b": f"{name}_e"},
            {"type": "filter", "name": f"{name}_f", "input": f"{name}_x", "mode": "lp", "freq": lp, "slope": 24},
            {"type": "gain", "name": name, "input": f"{name}_f", "db": gain_db}]


def hits(times_gains, burst=0.003):
    nodes, names = [], []
    for i, (t, g) in enumerate(times_gains):
        nodes += strike(f"s{i}", t, burst, g)
        names.append(f"s{i}")
    nodes.append({"type": "mix", "name": "strikes", "inputs": names})
    return nodes


VARIANT = {"v": "A"}


def ring(table, shape, f0, loss):
    """A: hand-built resonators (split pairs, f^-0.6 decay, shimmer).
    B: `modal` metal with narrow noise bands (noise 0.4, width 0.1), rung
    long; on the hint it was the first build heard as metal."""
    if VARIANT["v"] == "A":
        return {"type": "resonators", "name": "ring", "input": "strikes", "modes": table}
    return {"type": "modal", "name": "ring", "excite": "strikes", "material": "metal", "shape": shape, "f0": f0,
            "loss": loss, "noise": 0.4, "noise_width": 0.1, "tilt": 3, "modes": 64}


SRC = [{"type": "noise", "name": "wn", "color": "white"}, {"type": "noise", "name": "bn", "color": "brown"}]


def steel_pipe():
    """A 1 m steel pipe dropped on concrete: low bar modes ringing ~2.5 s, two bounces."""
    bounces = [(0.005, 1.0), (0.19, 0.45), (0.32, 0.22)]
    n = SRC + hits(bounces) + [
        ring(metal_modes(165, BAR, 2.6), "bar", 165, 0.3),
        {"type": "gain", "name": "ring_g", "input": "ring", "db": 0}]
    for i, (t, g) in enumerate(bounces):
        n += crack(t, -10 + 20 * __import__("math").log10(g), name=f"c{i}") + thud(t, -8 + 20 * __import__("math").log10(g), lp=300, t=0.08, name=f"t{i}")
    n.append({"type": "mix", "name": "dry", "inputs": ["ring_g"] + [f"c{i}" for i in range(3)] + [f"t{i}" for i in range(3)], "to": "mono"})
    return n, 3.0


def pot_lid():
    """A metal pot lid struck once with a spoon: dense plate modes, ~1.6 s."""
    n = SRC + hits([(0.005, 1.0)], burst=0.0015) + [
        ring(metal_modes(310, plate_ratios(18), 1.7, hash_n=30), "plate", 310, 0.4),
    ] + crack(0.005, -6, hp=4000, t=0.004)
    n.append({"type": "mix", "name": "dry", "inputs": ["ring", "crk"], "to": "mono"})
    return n, 2.2


def coin_drop():
    """A coin dropped on a wooden table: quick bounces speeding up, then a thin long ring."""
    t, gap, ev = 0.005, 0.11, []
    for i in range(7):
        ev.append((round(t, 4), round(0.8 ** i, 3)))
        t += gap
        gap *= 0.66
    n = SRC + hits(ev, burst=0.0008) + [
        ring(metal_modes(2900, [1, 2.31, 4.04, 6.1, 8.5], 1.2, split=(0.0008, 0.0025), hash_n=10, hash_band=(8000, 16000)), "plate", 2900, 0.5),
        {"type": "gain", "name": "ring_g", "input": "ring", "db": -4}]
    n += thud(0.005, -18, lp=900, t=0.03)
    n.append({"type": "mix", "name": "dry", "inputs": ["ring_g", "thd"], "to": "mono"})
    return n, 1.8


def wrench_drop():
    """A heavy wrench dropped on concrete: a mid bar ringing ~0.9 s, a hard clank, one bounce."""
    bounces = [(0.005, 1.0), (0.14, 0.35)]
    n = SRC + hits(bounces, burst=0.002) + [
        ring(metal_modes(620, BAR, 0.95, hash_n=20), "bar", 620, 0.6),
        {"type": "gain", "name": "ring_g", "input": "ring", "db": -2}]
    n += crack(0.005, -4, hp=2500, t=0.008) + crack(0.14, -13, hp=2500, t=0.006, name="crk2") + thud(0.005, -10, lp=350, t=0.06)
    n.append({"type": "mix", "name": "dry", "inputs": ["ring_g", "crk", "crk2", "thd"], "to": "mono"})
    return n, 1.6


def metal_bin():
    """An empty metal trash can kicked: low tube modes, boomy body, ~1.3 s."""
    n = SRC + hits([(0.005, 1.0)], burst=0.006) + [
        ring(metal_modes(118, TUBE + [23, 30], 1.3, split=(0.002, 0.01), hash_n=30, hash_band=(1500, 8000)), "tube", 118, 0.5),
        {"type": "decay", "name": "boom_e", "time": 0.35, "at": 0.005},
        {"type": "math", "name": "boom_x", "op": "mul", "a": "wn", "b": "boom_e"},
        {"type": "filter", "name": "boom", "input": "boom_x", "mode": "bp", "freq": 230, "q": 1.2},
        {"type": "gain", "name": "boom_g", "input": "boom", "db": 2}] + crack(0.005, -10, hp=2500, t=0.01)
    n.append({"type": "mix", "name": "dry", "inputs": ["ring", "boom_g", "crk"], "to": "mono"})
    return n, 2.0


def rock_on_concrete():
    """A fist-sized rock dropped on concrete: a hard bright click, a short dead body, grit, one small bounce."""
    n = SRC + [{"type": "noise", "name": "grit_n", "color": "crackle"}]
    for i, (t, g) in enumerate([(0.005, 0), (0.115, -10)]):
        n += crack(t, -2 + g, hp=2200, t=0.004, name=f"c{i}")
        n += [{"type": "decay", "name": f"b{i}_e", "time": 0.05, "at": t},
              {"type": "math", "name": f"b{i}_x", "op": "mul", "a": "wn", "b": f"b{i}_e"},
              {"type": "filter", "name": f"b{i}_hi", "input": f"b{i}_x", "mode": "bp", "freq": 1400, "q": 0.9},
              {"type": "filter", "name": f"b{i}_lo", "input": f"b{i}_x", "mode": "bp", "freq": 520, "q": 1.1},
              {"type": "mix", "name": f"b{i}_m", "inputs": [f"b{i}_hi", {"input": f"b{i}_lo", "gain": 0.7}]},
              {"type": "gain", "name": f"b{i}", "input": f"b{i}_m", "db": 2 + g}]
        n += [{"type": "decay", "name": f"g{i}_e", "time": 0.06, "at": t},
              {"type": "math", "name": f"g{i}_x", "op": "mul", "a": "grit_n", "b": f"g{i}_e"},
              {"type": "filter", "name": f"g{i}_f", "input": f"g{i}_x", "mode": "hp", "freq": 1800},
              {"type": "gain", "name": f"g{i}", "input": f"g{i}_f", "db": -6 + g}]
        n += thud(t, -12 + g, lp=200, t=0.07, name=f"t{i}")
    n.append({"type": "mix", "name": "dry", "inputs": [f"{p}{i}" for p in "cbgt" for i in range(2)], "to": "mono"})
    return n, 0.8


def rock_modal():
    """A rock on concrete, struck stone modes instead of noise bands: a
    noise burst into `modal` stone (high partials dying in tens of ms), the
    same thud and a second small hit."""
    n = SRC + hits([(0.005, 1.0), (0.115, 0.32)], burst=0.002) + [
        {"type": "modal", "name": "rock", "excite": "strikes", "material": "stone", "f0": 1500, "loss": 1.5, "noise": 0.45, "noise_width": 0.2, "tilt": 0, "modes": 48},
        {"type": "gain", "name": "rock_g", "input": "rock", "db": 0}]
    n += crack(0.005, -6, hp=2500, t=0.003) + thud(0.005, -10, lp=200, t=0.07) + thud(0.115, -20, lp=200, t=0.05, name="thd2")
    n.append({"type": "mix", "name": "dry", "inputs": ["rock_g", "crk", "thd", "thd2"], "to": "mono"})
    return n, 0.8


def pebbles_clack():
    """Two pebbles knocked together three times: a sharp, slightly pitched clack, no ring."""
    times = [0.005, 0.34, 0.66]
    n = SRC + hits([(t, 1.0) for t in times], burst=0.0006) + [
        {"type": "modal", "name": "clack", "excite": "strikes", "material": "stone", "f0": 3400, "loss": 2.5, "noise": 0.35, "tilt": 2, "modes": 24},
        {"type": "gain", "name": "clack_g", "input": "clack", "db": 0}]
    for i, t in enumerate(times):
        n += crack(t, -8, hp=5000, t=0.002, name=f"c{i}")
    n.append({"type": "mix", "name": "dry", "inputs": ["clack_g"] + [f"c{i}" for i in range(3)], "to": "mono"})
    return n, 1.0


def stone_slab():
    """A heavy stone slab dropped flat: a big dead thud, a gritty crack, a few chips settling."""
    n = SRC + [{"type": "noise", "name": "grit_n", "color": "crackle"}]
    n += thud(0.005, 0, lp=140, t=0.35) + crack(0.005, -6, hp=1800, t=0.012)
    n += [{"type": "decay", "name": "b_e", "time": 0.14, "at": 0.005},
          {"type": "math", "name": "b_x", "op": "mul", "a": "wn", "b": "b_e"},
          {"type": "filter", "name": "b", "input": "b_x", "mode": "bp", "freq": 420, "q": 0.8},
          {"type": "decay", "name": "g_e", "time": 0.12, "at": 0.005},
          {"type": "math", "name": "g_x", "op": "mul", "a": "grit_n", "b": "g_e"},
          {"type": "filter", "name": "g", "input": "g_x", "mode": "bp", "freq": 2500, "q": 0.7}]
    chips = []
    for i in range(5):
        t = round(0.09 + i * rng.uniform(0.04, 0.09), 3)
        n += crack(t, -18 - 2 * i, hp=1500, t=0.003, name=f"ch{i}")
        chips.append(f"ch{i}")
    n.append({"type": "mix", "name": "dry", "inputs": ["thd", "crk", {"input": "b", "gain": 1.6}, "g"] + chips, "to": "mono"})
    return n, 1.2


DESIGNS = [("metal", "steel_pipe", steel_pipe), ("metal", "pot_lid", pot_lid), ("metal", "coin_drop", coin_drop),
           ("metal", "wrench_drop", wrench_drop), ("metal", "metal_bin", metal_bin),
           ("stone", "rock_on_concrete", rock_on_concrete), ("stone", "rock_modal", rock_modal), ("stone", "pebbles_clack", pebbles_clack),
           ("stone", "stone_slab", stone_slab)]


def main(project):
    cues = []
    jobs = []
    for fam, name, make in DESIGNS:
        if fam == "metal":
            jobs += [(fam, name + "_A", make, "A", (False,)), (fam, name + "_B", make, "B", (False, True))]
        else:
            jobs.append((fam, name, make, "B", (False, True)))
    for fam, name, make, variant, dresses in jobs:
        VARIANT["v"] = variant
        state = rng.getstate()
        nodes, dur = make()
        rng.setstate(state)
        for dress in dresses:
            ops = [dict(x) for x in nodes]
            last = "dry"
            if dress:
                ops.append({"type": "reverb", "name": "room", "input": "dry", "model": "room", "size": 0.25, "decay": 0.6, "mix": 0.18})
                last = "room"
            ops.append({"type": "output", "name": "out", "input": last, "peak_db": -3, "fade_in": 0.0005})
            cues.append({"family": fam, "design": name, "dressed": dress, "ops": ops, "duration": dur})
    rng.shuffle(cues)
    calls, batch = [], []
    for i, c in enumerate(cues):
        sid = f"m{i + 1:02d}"
        args = {"sound": sid, "ops": c["ops"], "duration": c["duration"], "seed": 1}
        calls += [{"name": "sound_ops", "args": args}, {"name": "listen", "args": {"doc": sid, "descriptors": False}},
                  {"name": "export", "args": {"doc": sid, "format": "wav", "path": "batch4"}}]
        batch.append({"id": sid, "family": c["family"], "design": c["design"], "dressed": c["dressed"], "doc": args})
    cf = os.path.join(project, "calls4.json")
    json.dump(calls, open(cf, "w"))
    json.dump(batch, open(os.path.join(HERE, "batch4.json"), "w"), indent=1)
    env = dict(os.environ, SOUNDER_PROJECT=project)
    p = subprocess.run([sys.executable, os.path.join(REPO, "mcp_driver.py"), "--calls", cf], cwd=REPO, env=env, capture_output=True, text=True)
    sid = iter(b["id"] for b in batch)
    for l in p.stdout.splitlines():
        if not l.startswith("{"):
            continue
        d = json.loads(l)
        t = " ".join(map(str, d.get("text", [])))
        if d.get("ok") is False:
            print("FAIL", t[:300])
        for line in t.splitlines():
            if line.startswith("heard_as"):
                print(next(sid, "?"), line.split("): ", 1)[1][:150])


if __name__ == "__main__":
    main(sys.argv[1])
