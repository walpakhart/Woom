#!/usr/bin/env bash
# swap-binary.sh — applies a Woom auto-update DMG over the running
# /Applications/Woom.app install during the host process's quit.
#
# Invoked from `updater.rs::ExitRequested` with two positional args:
#   $1 = absolute path to the .dmg
#   $2 = absolute path to the .app to replace (typically
#        /Applications/Woom.app)
#
# Side effects:
#   - Mounts the DMG at a temp mount point.
#   - Removes $2 and ditto-copies the new Woom.app on top.
#   - Detaches the mount.
#   - Strips the com.apple.quarantine xattr so Gatekeeper doesn't
#     re-prompt on relaunch.
#   - Appends a log line to ~/Library/Logs/Woom/update-swap.log so the
#     next launch can detect success/failure (see updater.rs's
#     pending-update startup detection).
#
# Exit codes:
#   0 = swap completed; log line ends with "OK: swap complete <version>"
#   1 = pre-swap precondition failed (DMG missing / mount failed)
#   2 = swap itself failed (rm / ditto error); old install left intact
#
# Phase reference: SDD workspace `sdd-2508eeb82e`, phase 5 task 2.

set -uo pipefail

DMG_PATH="${1:-}"
APP_INSTALL_PATH="${2:-/Applications/Woom.app}"
LOG_DIR="$HOME/Library/Logs/Woom"
LOG="$LOG_DIR/update-swap.log"

mkdir -p "$LOG_DIR"
log() { printf '[%s] %s\n' "$(date -u +%FT%TZ)" "$*" >> "$LOG"; }

log "=== swap start dmg=$DMG_PATH target=$APP_INSTALL_PATH ==="

if [[ -z "$DMG_PATH" || ! -f "$DMG_PATH" ]]; then
    log "FAIL: DMG missing at '$DMG_PATH'"
    exit 1
fi

MOUNT_POINT="$(
    hdiutil attach -nobrowse -noverify -quiet "$DMG_PATH" 2>&1 \
      | awk '/\/Volumes\// { for (i = NF; i >= 1; i--) if ($i ~ /\/Volumes\//) { print $i; exit } }'
)"
if [[ -z "$MOUNT_POINT" || ! -d "$MOUNT_POINT" ]]; then
    log "FAIL: hdiutil attach produced no mount point (DMG corrupt?)"
    exit 1
fi
log "mounted at $MOUNT_POINT"

NEW_APP="$MOUNT_POINT/Woom.app"
if [[ ! -d "$NEW_APP" ]]; then
    hdiutil detach -quiet "$MOUNT_POINT" || true
    log "FAIL: Woom.app not found in DMG"
    exit 1
fi

# Extract the new version's CFBundleShortVersionString for the
# success-toast detection on next launch.
NEW_VERSION="$(
    /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' \
      "$NEW_APP/Contents/Info.plist" 2>/dev/null || echo unknown
)"
log "incoming version: $NEW_VERSION"

# The destructive section. We accept that $APP_INSTALL_PATH may not
# exist (fresh install relocated by the user) — `rm -rf` is no-op for
# missing paths, and `ditto` will create the destination dir.
if ! rm -rf "$APP_INSTALL_PATH" 2>>"$LOG"; then
    hdiutil detach -quiet "$MOUNT_POINT" || true
    log "FAIL: could not remove existing $APP_INSTALL_PATH"
    exit 2
fi
if ! ditto "$NEW_APP" "$APP_INSTALL_PATH" 2>>"$LOG"; then
    hdiutil detach -quiet "$MOUNT_POINT" || true
    log "FAIL: ditto copy failed; install slot now MISSING — re-download manually"
    exit 2
fi

hdiutil detach -quiet "$MOUNT_POINT" || true
xattr -d com.apple.quarantine "$APP_INSTALL_PATH" 2>/dev/null || true

log "OK: swap complete $NEW_VERSION"
exit 0
