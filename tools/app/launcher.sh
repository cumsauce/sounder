#!/bin/bash
# Sounder.app's executable: starts the shared sounder daemon with the window, using the server binary in this
# bundle. Agents' MCP proxies connect to that same daemon (/tmp/sounder-mcp.sock), so the window shows every
# agent's sounds. When the daemon is already running, a harmless call makes it reopen the window if it was closed.
HERE="$(cd "$(dirname "$0")" && pwd)"
RES="$HERE/../Resources"
SOCK="${SOUNDER_SOCKET:-/tmp/sounder-mcp.sock}"
if [ -S "$SOCK" ] && "@PYTHON@" -c "import socket,sys; s=socket.socket(socket.AF_UNIX); s.settimeout(1); s.connect(sys.argv[1])" "$SOCK" 2>/dev/null; then
  # The daemon is up, but its server (which owns the window) may have exited: closing the window ends it, and
  # the daemon starts it again on the next call. A call with no side effects brings the window back.
  CALLS="$(mktemp -t sounder-app)"
  echo '[{"name": "describe_op", "args": {"kind": "output"}}]' > "$CALLS"
  "@PYTHON@" "$RES/mcp_driver.py" --connect --socket "$SOCK" --client sounder-app --calls "$CALLS" >/dev/null 2>&1
  rm -f "$CALLS"
  exit 0
fi
export SOUNDER_BINARY="$HERE/sounder-server"
cd "$HOME"
nohup "@PYTHON@" "$RES/mcp_driver.py" --daemon --windowed >/dev/null 2>&1 &
