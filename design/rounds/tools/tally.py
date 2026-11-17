#!/usr/bin/env python3
"""tally.py ROUND -> scores table (markdown) for design/rounds/r<N>/scores.md"""
import json, sys, os, glob
R = sys.argv[1]; root = os.path.join(os.path.dirname(__file__), "..", "..", "..", ".swarm", f"r{R}")
rows = []
for d in sorted(glob.glob(os.path.join(root, "*"))):
    s, f = os.path.join(d, "score.json"), os.path.join(d, "feedback.json")
    if not (os.path.exists(s) or os.path.exists(f)): continue
    sc = json.load(open(s)) if os.path.exists(s) else {}
    fb = json.load(open(f)) if os.path.exists(f) else {}
    L = sc.get("lines", {})
    rows.append((os.path.basename(d), sc.get("total"), [L.get(k, {}).get("score") for k in ("checks","time_shape","spectrum","parts","control_variation")],
                 fb.get("difficulty"), fb.get("self_score"), fb.get("calls"), fb.get("failed_calls"), fb.get("finished")))
print("| sound | judge | chk/time/spec/parts/ctl | difficulty | self | calls | failed | finished |\n|---|---|---|---|---|---|---|---|")
for r in rows: print(f"| {r[0]} | {r[1]} | {'/'.join(str(x) for x in r[2])} | {r[3]} | {r[4]} | {r[5]} | {r[6]} | {r[7]} |")
t=[r[1] for r in rows if r[1] is not None]; dd=[r[3] for r in rows if r[3] is not None]
fl=[r[6] for r in rows if r[6] is not None]; cl=[r[5] for r in rows if r[5] is not None]
print(f"\nsounds {len(rows)}; mean judge {sum(t)/max(1,len(t)):.2f}; mean difficulty {sum(dd)/max(1,len(dd)):.2f}; mean calls {sum(cl)/max(1,len(cl)):.1f}; mean failed calls {sum(fl)/max(1,len(fl)):.1f}")
