#!/bin/bash
# sx.sh ROUND SOUND start|calls FILE|stop
# One sounder daemon per (round, sound). Run from anywhere.
#   start        start a headless daemon for this sound (idempotent)
#   calls FILE   run the JSON list of tool calls in FILE; prints one JSON line
#                per call; images go to <project>/shots/runNN/
#   stop         stop the daemon
# Calls file: [{"name": "sound_ops", "args": {...}}, ...]
set -u
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
R="$1"; S="$2"; CMD="${3:-}"; ARG="${4:-}"
BIN="$ROOT/.swarm/bin/r$R"
export SOUNDER_BINARY="$BIN/sounder"
export SOUNDER_PROJECT="$ROOT/.swarm/r$R/$S"
export SOUNDER_SOCKET="/tmp/sr$R-$S.sock"
export SOUNDER_STDERR="$SOUNDER_PROJECT/server.log"
mkdir -p "$SOUNDER_PROJECT/.sounder"
DRV="$BIN/mcp_driver.py"
case "$CMD" in
  start)
    if [ -S "$SOUNDER_SOCKET" ]; then echo "daemon already up: $SOUNDER_SOCKET"; exit 0; fi
    nohup python3 "$DRV" --daemon --headless --socket "$SOUNDER_SOCKET" >"$SOUNDER_PROJECT/daemon.out" 2>&1 &
    for i in $(seq 1 40); do [ -S "$SOUNDER_SOCKET" ] && { echo "daemon up: $SOUNDER_SOCKET"; exit 0; }; sleep 0.25; done
    echo "daemon failed to start; see $SOUNDER_PROJECT/daemon.out"; exit 1 ;;
  calls)
    [ -f "$ARG" ] || { echo "no such calls file: $ARG"; exit 1; }
    n=$(ls -d "$SOUNDER_PROJECT"/shots/run* 2>/dev/null | wc -l | tr -d ' ')
    d="$SOUNDER_PROJECT/shots/run$(printf %02d $((n+1)))"; mkdir -p "$d"
    python3 "$DRV" --connect --socket "$SOUNDER_SOCKET" --client "t-$R-$S" --calls "$ARG" --save-dir "$d" ;;
  stop)
    echo '[{"name":"quit"}]' > "$SOUNDER_PROJECT/.quit.json"
    python3 "$DRV" --connect --socket "$SOUNDER_SOCKET" --calls "$SOUNDER_PROJECT/.quit.json"; rm -f "$SOUNDER_PROJECT/.quit.json" ;;
  *) echo "usage: sx.sh ROUND SOUND start|calls FILE|stop"; exit 2 ;;
esac
