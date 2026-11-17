"""Random + local search over slam_b's levers, minimising the mean STFT
distance to the reference recordings. Runs a throwaway sounder server via
mcp_driver.py (one process, many calls)."""
import json, random, re, subprocess, sys, os

REPO = "/Users/doomy/Code/sounder"
MODE = os.environ.get("MODE", "b")
REFS = [
    "refs/freesound/406635__tlcolbe__bathroom-door-slam-shut_more-air.wav",
    "refs/freesound/724124__saha213131__door-slam2.wav",
]
SPACE = {  # name: (lo, hi, log?)
    "imp_db": (-6, 14, False), "imp_hz": (1200, 6000, True), "imp_t": (0.02, 0.15, True),
    "panel_size": (0.5, 1.2, False), "panel_loss": (0.3, 1.6, True), "panel_db": (-14, 0, False),
    "frame_db": (-26, -4, False), "sub_db": (-26, -4, False), "sub_t": (0.2, 1.4, True),
    "latch_db": (-14, 4, False), "rattle_db": (-18, 0, False),
    "room_decay": (0.3, 1.5, True), "room_wet": (0.1, 1.3, False), "room_size": (0.15, 0.75, False),
    "body_eq": (-4, 8, False), "punch": (0, 8, False),
}
BASE = {"imp_db": 8, "imp_hz": 3500, "imp_t": 0.06, "panel_size": 0.8, "panel_loss": 0.6,
        "panel_db": -4, "frame_db": -14, "sub_db": -16, "sub_t": 0.9, "latch_db": 0,
        "rattle_db": -4, "room_decay": 1.1, "room_wet": 1.0, "room_size": 0.45, "body_eq": 3,
        "punch": 4}


SPACE_C = {
    "rec_db": (-6, 6, False), "rec_rate": (0.8, 1.15, False),
    "sub_db": (-30, -4, False), "sub_t": (0.2, 1.4, True), "latch_db": (-24, 0, False),
    "room_decay": (0.3, 1.5, True), "room_wet": (0.0, 1.0, False), "room_size": (0.15, 0.75, False),
    "punch": (0, 8, False),
}
BASE_C = {"rec_db": 0, "rec_rate": 1.0, "sub_db": -16, "sub_t": 0.9, "latch_db": -12,
          "room_decay": 0.8, "room_wet": 0.3, "room_size": 0.4, "punch": 2}


def ops_c(p):
    return [
        {"type": "sample", "name": "rec", "file": "refs/freesound/src_571797_slam.wav", "at": 0.005, "rate": p["rec_rate"]},
        {"type": "gain", "name": "rec_g", "input": "rec", "db": p["rec_db"]},
        {"type": "ramp", "name": "sub_f", "from": 75, "to": 38, "time": 0.25, "curve": "exp"},
        {"type": "osc", "name": "sub_o", "freq": "sub_f"},
        {"type": "decay", "name": "sub_e", "time": p["sub_t"], "at": 0.006},
        {"type": "math", "name": "sub", "op": "mul", "a": "sub_o", "b": "sub_e"},
        {"type": "gain", "name": "sub_g", "input": "sub", "db": p["sub_db"]},
        {"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.022},
        {"type": "gain", "name": "latch_g", "input": "latch", "db": p["latch_db"]},
        {"type": "mix", "name": "dry", "inputs": ["rec_g", "sub_g", "latch_g"], "to": "mono"},
        {"type": "transient", "name": "punch", "input": "dry", "attack_db": p["punch"]},
        {"type": "shape", "name": "warm", "input": "punch", "mode": "tube", "drive": 1.5, "oversample": 4, "mix": 0.2},
        {"type": "onepole", "name": "dc", "input": "warm", "mode": "hp", "freq": 25},
        {"type": "filter", "name": "send_hp", "input": "dc", "mode": "hp", "freq": 150},
        {"type": "reverb", "name": "room", "input": "send_hp", "model": "room", "size": p["room_size"],
         "decay": p["room_decay"], "damp": 0.55, "mix": 1, "predelay": 0.006, "er_mix": 0.5, "width": 1.6},
        {"type": "mix", "name": "bus", "inputs": [{"input": "dc", "gain": 1}, {"input": "room.wet", "gain": p["room_wet"]}], "to": "stereo"},
        {"type": "output", "name": "out", "input": "bus", "peak_db": -1.5},
    ]


if MODE == "c":
    SPACE, BASE = SPACE_C, BASE_C


def ops(p):
    if MODE == "c":
        return ops_c(p)
    return [
        {"type": "noise", "name": "imp_n", "color": "white"},
        {"type": "decay", "name": "imp_e", "time": p["imp_t"], "at": 0.005},
        {"type": "math", "name": "imp_raw", "op": "mul", "a": "imp_n", "b": "imp_e"},
        {"type": "filter", "name": "imp", "input": "imp_raw", "mode": "bp", "freq": p["imp_hz"], "q": 0.8},
        {"type": "gain", "name": "imp_g", "input": "imp", "db": p["imp_db"]},
        {"type": "contact", "name": "blow", "speed": 3, "mass": 1.5, "material": "wood", "hardness": 0.55, "at": 0.005},
        {"type": "modal", "name": "panel", "excite": "blow", "material": "wood", "size": p["panel_size"],
         "loss": p["panel_loss"], "strike_position": 0.27, "gain_db": p["panel_db"]},
        {"type": "modal", "name": "frame", "excite": "blow", "material": "wood", "shape": "bar", "size": 0.5,
         "loss": 1.2, "gain_db": p["frame_db"]},
        {"type": "ramp", "name": "sub_f", "from": 75, "to": 38, "time": 0.25, "curve": "exp"},
        {"type": "osc", "name": "sub_o", "freq": "sub_f"},
        {"type": "decay", "name": "sub_e", "time": p["sub_t"], "at": 0.006},
        {"type": "math", "name": "sub", "op": "mul", "a": "sub_o", "b": "sub_e"},
        {"type": "gain", "name": "sub_g", "input": "sub", "db": p["sub_db"]},
        {"type": "click", "name": "latch", "kind": "latch", "size": 0.6, "hardness": 0.8, "at": 0.022},
        {"type": "gain", "name": "latch_g", "input": "latch", "db": p["latch_db"]},
        {"type": "decay", "name": "rat_e", "time": 0.18, "at": 0.01},
        {"type": "math", "name": "rat_d", "op": "mul", "a": "rat_e", "b": 90},
        {"type": "crumple", "name": "rattle", "density": "rat_d", "size": 0.15, "hardness": 0.9},
        {"type": "gain", "name": "rattle_g", "input": "rattle", "db": p["rattle_db"]},
        {"type": "mix", "name": "dry", "inputs": ["imp_g", "panel", "frame", "sub_g", "latch_g", "rattle_g"], "to": "mono"},
        {"type": "transient", "name": "punch", "input": "dry", "attack_db": p["punch"]},
        {"type": "shape", "name": "warm", "input": "punch", "mode": "tube", "drive": 1.5, "oversample": 4, "mix": 0.25},
        {"type": "onepole", "name": "dc", "input": "warm", "mode": "hp", "freq": 25},
        {"type": "eq", "name": "tone", "input": "dc", "bands": [{"mode": "lp", "freq": 9000},
                                                              {"mode": "peak", "freq": 300, "gain_db": p["body_eq"], "q": 0.8}]},
        {"type": "filter", "name": "send_hp", "input": "tone", "mode": "hp", "freq": 150},
        {"type": "reverb", "name": "room", "input": "send_hp", "model": "room", "size": p["room_size"],
         "decay": p["room_decay"], "damp": 0.55, "mix": 1, "predelay": 0.006, "er_mix": 0.5, "width": 1.6},
        {"type": "mix", "name": "bus", "inputs": [{"input": "tone", "gain": 1}, {"input": "room.wet", "gain": p["room_wet"]}], "to": "stereo"},
        {"type": "output", "name": "out", "input": "bus", "peak_db": -1.5},
    ]


class Server:
    def __init__(self):
        self.p = subprocess.Popen([sys.executable, "mcp_driver.py", "--client", "search_" + MODE], cwd=REPO,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                  text=True, bufsize=1)

    def call(self, name, args):
        self.p.stdin.write(json.dumps({"name": name, "args": args}) + "\n")
        self.p.stdin.flush()
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("driver exited")
            line = line.strip()
            if line.startswith("{") and '"ok"' in line:
                return json.loads(line)


def score(srv, doc, p):
    r = srv.call("sound_ops", {"sound": doc, "duration": 1.6, "ops": ops(p)})
    if not r.get("ok"):
        return None, str(r)[:300]
    ds = []
    for ref in REFS:
        r = srv.call("verify", {"doc": doc, "expect": {"reference": {"file": ref, "max_distance": 0.01}}})
        m = re.search(r"reference: ([0-9.]+) dB", " ".join(map(str, r.get("text", []))))
        if not m:
            return None, str(r)[:300]
        ds.append(float(m.group(1)))
    return sum(ds) / len(ds), ds


def sample(center=None, scale=1.0):
    p = {}
    for k, (lo, hi, lg) in SPACE.items():
        if center is None:
            v = random.uniform(lo, hi) if not lg else lo * (hi / lo) ** random.random()
        else:
            span = (hi - lo) * 0.15 * scale
            v = min(hi, max(lo, center[k] + random.gauss(0, span)))
        p[k] = round(v, 4)
    return p


def main():
    random.seed(int(sys.argv[1]) if len(sys.argv) > 1 else 7)
    n_rand, n_local = 60, 120
    srv = Server()
    doc = "slam_search_" + MODE
    best_p, (best_s, best_ds) = BASE, score(srv, doc, BASE)
    print("base", round(best_s, 3), best_ds, flush=True)
    for i in range(n_rand + n_local):
        cand = sample() if i < n_rand else sample(best_p, scale=max(0.2, 1 - (i - n_rand) / n_local))
        s, ds = score(srv, doc, cand)
        if s is None:
            print("err", ds, flush=True)
            continue
        if s < best_s:
            best_p, best_s, best_ds = cand, s, ds
            print(i, "best", round(s, 3), ds, flush=True)
    json.dump({"score": best_s, "dists": best_ds, "params": best_p, "ops": ops(best_p)},
              open(os.path.join(os.path.dirname(__file__), "best_slam_" + MODE + ".json"), "w"), indent=1)
    print("done", best_s, best_ds, json.dumps(best_p))
    srv.p.stdin.close()


if __name__ == "__main__":
    main()
