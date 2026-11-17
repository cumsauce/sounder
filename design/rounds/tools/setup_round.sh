#!/bin/bash
# setup_round.sh N  -- run in the main checkout on swarm/main.
# Builds the binary, copies binary + driver + manual to .swarm/bin/rN, creates
# one empty project per sound (core + that round's reserve sounds).
set -eu
N="$1"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"
cargo build --bin sounder 2>&1 | tail -2
mkdir -p ".swarm/bin/r$N"
cp target/debug/sounder mcp_driver.py AGENTS.md ".swarm/bin/r$N/"
git rev-parse --short HEAD > ".swarm/bin/r$N/COMMIT"
python3 - "$N" <<'PY'
import json, os, sys
n = sys.argv[1]
b = json.load(open("design/rounds/briefs.json"))
names = [r["name"] for r in b["core"]] + [r["name"] for r in b["reserve"].get("r" + n, [])]
for s in names:
    os.makedirs(f".swarm/r{n}/{s}/.sounder", exist_ok=True)
print(len(names), "projects; commit", open(f".swarm/bin/r{n}/COMMIT").read().strip())
PY
