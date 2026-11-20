#!/bin/bash
# Builds Sounder.app from a branch's latest commit and installs it in /Applications.
#
#   tools/app/install.sh            build + install (skips the build when the installed app is that commit)
#   tools/app/install.sh --force    rebuild even when up to date
#   tools/app/install.sh --agent    also install the LaunchAgent that reruns this when the branch moves
#
# It builds from a clean worktree of the branch (SOUNDER_APP_BRANCH, default swarm/main), never from the
# shared checkout, so other sessions' uncommitted edits never reach the app. Build output is cached in
# ~/Library/Caches/sounder-app, so an update rebuilds only what changed.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
BRANCH="${SOUNDER_APP_BRANCH:-swarm/main}"
CACHE="$HOME/Library/Caches/sounder-app"
WT="$CACHE/src"
DEST="${SOUNDER_APP_DEST:-/Applications/Sounder.app}"
LOG="$HOME/Library/Logs/sounder-app.log"
LABEL="com.sounder.app-updater"
FORCE=0; AGENT=0
for a in "$@"; do case "$a" in --force) FORCE=1 ;; --agent) AGENT=1 ;; *) echo "unknown argument $a" >&2; exit 2 ;; esac; done
mkdir -p "$CACHE" "$(dirname "$LOG")"
PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

# one run at a time (the agent can fire while a manual run builds)
exec 9>"$CACHE/lock"
if ! /usr/bin/lockf -t 0 9 2>/dev/null; then echo "another install is running" >&2; exit 0; fi

if [ "$AGENT" = 1 ]; then
  PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
  GITDIR="$(git -C "$REPO" rev-parse --git-common-dir)"; case "$GITDIR" in /*) ;; *) GITDIR="$REPO/$GITDIR" ;; esac
  cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>/bin/bash</string><string>$REPO/tools/app/install.sh</string></array>
  <key>EnvironmentVariables</key><dict><key>SOUNDER_APP_BRANCH</key><string>$BRANCH</string></dict>
  <key>WatchPaths</key><array><string>$GITDIR/refs/heads/$BRANCH</string><string>$GITDIR/packed-refs</string></array>
  <key>StartInterval</key><integer>1800</integer>
  <key>ThrottleInterval</key><integer>60</integer>
  <key>RunAtLoad</key><true/>
  <key>LowPriorityIO</key><true/>
  <key>Nice</key><integer>10</integer>
  <key>StandardOutPath</key><string>$LOG</string>
  <key>StandardErrorPath</key><string>$LOG</string>
</dict></plist>
EOF
  launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$PLIST"
  echo "updater installed: $PLIST (watches $BRANCH; log $LOG)"
  exit 0
fi

REV="$(git -C "$REPO" rev-parse --verify "$BRANCH^{commit}")"
SHORT="${REV:0:9}"
INSTALLED="$(cat "$DEST/Contents/Resources/commit" 2>/dev/null || true)"
if [ "$FORCE" = 0 ] && [ "$INSTALLED" = "$REV" ]; then exit 0; fi
echo "$(date '+%F %T') building $BRANCH @ $SHORT (installed: ${INSTALLED:0:9})"

# the clean worktree, moved to this commit (only changed files are touched, so cargo stays incremental)
if [ ! -e "$WT/.git" ]; then git -C "$REPO" worktree prune; git -C "$REPO" worktree add --detach "$WT" "$REV" >/dev/null; fi
git -C "$WT" checkout --detach --force -q "$REV"
git -C "$WT" clean -fdq
CARGO_TARGET_DIR="$CACHE/target" cargo build --release --manifest-path "$WT/Cargo.toml" --bin sounder -q
BIN="$CACHE/target/release/sounder"

# the bundle, built beside the destination and swapped in whole
STAGE="$CACHE/Sounder.app"; rm -rf "$STAGE"; mkdir -p "$STAGE/Contents/MacOS" "$STAGE/Contents/Resources"
cp "$BIN" "$STAGE/Contents/MacOS/sounder-server"
cp "$WT/mcp_driver.py" "$STAGE/Contents/Resources/mcp_driver.py"
PY="$(command -v python3)"
sed -e "s|@PYTHON@|$PY|" "$WT/tools/app/launcher.sh" > "$STAGE/Contents/MacOS/Sounder"; chmod +x "$STAGE/Contents/MacOS/Sounder"
echo "$REV" > "$STAGE/Contents/Resources/commit"
ICONSET="$CACHE/sounder.iconset"; rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$WT/src/bin/sounder/window/icon.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); [ $d -le 512 ] && sips -z $d $d "$WT/src/bin/sounder/window/icon.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$STAGE/Contents/Resources/sounder.icns"
cat > "$STAGE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Sounder</string>
  <key>CFBundleDisplayName</key><string>Sounder</string>
  <key>CFBundleIdentifier</key><string>com.sounder.window</string>
  <key>CFBundleExecutable</key><string>Sounder</string>
  <key>CFBundleIconFile</key><string>sounder</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>$SHORT</string>
  <key>LSUIElement</key><true/>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
codesign --force --deep --sign - "$STAGE" 2>/dev/null || true
if [ -e "$DEST" ]; then mv "$DEST" "$CACHE/Sounder.old.app.$$"; fi
mv "$STAGE" "$DEST"
rm -rf "$CACHE"/Sounder.old.app.*
touch "$DEST"
echo "$(date '+%F %T') installed $DEST ($BRANCH @ $SHORT)"
