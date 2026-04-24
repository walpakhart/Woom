#!/usr/bin/env bash
# Build a distributable .dmg for Forgehold (macOS only).
#
# Steps:
#   1. Compile every sidecar from the Cargo workspace in release mode.
#   2. Copy each sidecar into `src-tauri/binaries/<name>-<triple>` — the
#      path/naming convention Tauri's `externalBin` expects (the triple
#      suffix is stripped when the binary is placed inside the .app).
#   3. Invoke `pnpm tauri build`, which produces `.app` and `.dmg` under
#      `src-tauri/target/release/bundle/`.
#
# Set TARGET=universal-apple-darwin to build a universal binary (slower,
# requires both aarch64 and x86_64 rust targets installed via rustup).
# Otherwise we pick the host triple.

set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script only builds macOS artifacts (.app/.dmg)." >&2
  exit 1
fi

DESKTOP_DIR="apps/desktop"
TAURI_DIR="$DESKTOP_DIR/src-tauri"
BINARIES_DIR="$TAURI_DIR/binaries"

# Resolve host triple if caller didn't override. `uname -m` returns arm64 on
# Apple Silicon; the Rust triple is aarch64 — translate.
if [[ -z "${TARGET:-}" ]]; then
  case "$(uname -m)" in
    arm64|aarch64) TARGET="aarch64-apple-darwin" ;;
    x86_64)        TARGET="x86_64-apple-darwin" ;;
    *) echo "Unsupported arch: $(uname -m)" >&2; exit 1 ;;
  esac
fi

echo "→ target triple: $TARGET"

SIDECARS=(forgehold-github forgehold-jira forgehold-memory)

mkdir -p "$BINARIES_DIR"

# Step 1: build each sidecar in release mode for the chosen target.
for name in "${SIDECARS[@]}"; do
  echo "→ cargo build --release -p $name --target $TARGET"
  (
    cd "$TAURI_DIR"
    # If the target isn't installed, `cargo build --target` will fail with a
    # clear hint to run `rustup target add`. Don't swallow it.
    cargo build --release -p "$name" --target "$TARGET"
  )
done

# Step 2: copy to binaries/ with the triple-suffixed name Tauri expects.
for name in "${SIDECARS[@]}"; do
  src="$TAURI_DIR/target/$TARGET/release/$name"
  dst="$BINARIES_DIR/$name-$TARGET"
  if [[ ! -f "$src" ]]; then
    echo "✗ missing built binary: $src" >&2
    exit 1
  fi
  cp -f "$src" "$dst"
  # Ad-hoc sign sidecars so macOS doesn't kill them on first run with SIGKILL
  # when the parent .app is itself only ad-hoc signed (SIP / amfid heuristics).
  codesign --force --sign - --timestamp=none "$dst" 2>/dev/null || true
  echo "→ $name → binaries/$name-$TARGET ($(du -h "$dst" | awk '{print $1}'))"
done

# Step 3: Tauri bundle. Uses the `externalBin` entries from tauri.conf.json,
# which reference `binaries/<name>` → Tauri resolves `<name>-<triple>` at
# bundle time from the files we just copied.
echo "→ pnpm tauri build --target $TARGET"
(
  cd "$DESKTOP_DIR"
  pnpm tauri build --target "$TARGET"
)

# Report where the artifacts ended up. Tauri lays them out as
#   <tauri>/target/<triple>/release/bundle/{macos,dmg}/…
BUNDLE_DIR="$TAURI_DIR/target/$TARGET/release/bundle"
APP_PATH=$(find "$BUNDLE_DIR/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -n1 || true)
DMG_PATH=$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -n1 || true)

# Ad-hoc sign the .app (and re-sign sidecars inside it) so that when someone
# copies the .dmg to another Mac — where macOS attaches `com.apple.quarantine`
# — the app opens instead of being flagged as "damaged". Still shows an
# "unidentified developer" prompt on first launch; that's expected without an
# Apple Developer ID. Also strip quarantine from the built artifacts on this
# machine so re-runs don't inherit a stale bit.
if [[ -n "$APP_PATH" ]]; then
  echo "→ ad-hoc signing $APP_PATH"
  # --deep hits every nested binary (sidecars, frameworks). Errors on already-
  # signed items are fine; we use `|| true` so one failed sign doesn't abort
  # the whole build.
  codesign --force --deep --sign - --timestamp=none "$APP_PATH" 2>/dev/null || true
  xattr -cr "$APP_PATH" 2>/dev/null || true
fi
if [[ -n "$DMG_PATH" ]]; then
  xattr -cr "$DMG_PATH" 2>/dev/null || true
fi

echo ""
echo "Done."
[[ -n "$APP_PATH" ]] && echo "  .app: $APP_PATH"
[[ -n "$DMG_PATH" ]] && echo "  .dmg: $DMG_PATH"
[[ -z "$APP_PATH" && -z "$DMG_PATH" ]] && {
  echo "  (no bundles found under $BUNDLE_DIR — check tauri build output above)" >&2
  exit 1
}
