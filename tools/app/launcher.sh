#!/bin/bash
# Sounder.app's executable: starts the shared sounder daemon with the window, using the server binary in this
# bundle. Agents' MCP proxies connect to that same daemon (/tmp/sounder-mcp.sock), so the window shows every
# agent's sounds. When the daemon is already running, its window is already open and this does nothing.
HERE="$(cd "$(dirname "$0")" && pwd)"
RES="$HERE/../Resources"
SOCK="${SOUNDER_SOCKET:-/tmp/sounder-mcp.sock}"
if [ -S "$SOCK" ] && "@PYTHON@" -c "import socket,sys; s=socket.socket(socket.AF_UNIX); s.settimeout(1); s.connect(sys.argv[1])" "$SOCK" 2>/dev/null; then
  exit 0
fi
export SOUNDER_BINARY="$HERE/sounder-server"
cd "$HOME"
nohup "@PYTHON@" "$RES/mcp_driver.py" --daemon --windowed >/dev/null 2>&1 &
