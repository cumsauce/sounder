#!/usr/bin/env python3
"""Write a grouped pick-the-best listening page from a batchN.json.

Takes are grouped by `group` when a cue has one, else by its first node's
type; within a group they are shuffled (seeded), so the letters stay blind.
The page saves one `picks` doc per group: {best: cue id | "none", note}.

    python3 tools/labels/grouped_page.py tools/labels/batch9.json OUT_DIR "Title"
"""
import json
import random
import sys
from pathlib import Path

TITLES = {"rain": "Rain", "creature": "Creature calls", "leaves": "Wind in plants",
          "surf": "Surf on a shore", "frog": "Frogs", "lava": "Lava", "cave": "Cave",
          "underwater": "Underwater",
          "creature_roar": "Creature roar", "creature_growl": "Creature growl",
          "creature_snarl": "Creature snarl", "creature_whine": "Creature whine"}


def main():
    batch, out, title = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
    cues = json.loads(batch.read_text())
    groups = {}
    for c in cues:
        op = c["doc"]["ops"][0]
        key = c.get("group") or (op["type"] + ("_" + op["call"] if "call" in op else ""))
        groups.setdefault(key, []).append(c["id"])
    rng = random.Random(batch.stem)
    out_groups = []
    for key, ids in groups.items():
        rng.shuffle(ids)
        out_groups.append({"key": key, "title": TITLES.get(key, key.replace("_", " ").capitalize()), "takes": ids})
    html = (Path(__file__).with_name("grouped.html").read_text()
            .replace("__TITLE__", title)
            .replace("__STORE__", "cue-picks-" + batch.stem)
            .replace("__GROUPS__", json.dumps(out_groups)))
    out.mkdir(parents=True, exist_ok=True)
    (out / "index.html").write_text(html)
    for g in out_groups:
        print(g["key"], len(g["takes"]))


if __name__ == "__main__":
    main()
