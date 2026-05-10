#!/usr/bin/env bash
# Build a distributable .dmg for Woom (macOS only).
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

# Where cargo actually writes build artefacts. When `CARGO_TARGET_DIR` is
# set in the environment (the Cursor IDE sandbox sets it to a per-project
# scratch dir, and CI / devs sometimes override it for the same reason),
# cargo ignores the in-repo `target/` and writes there. Reading from
# `$TAURI_DIR/target/...` after the build then silently picks up a STALE
# copy of the binary (whatever happened to be there from a prior local
# build), which is exactly the bug that shipped a sidecar with no
# `type` field on its MCP schema even though `cargo build` had just
# regenerated that very file in cache. Resolve from the env var first,
# fall back to the workspace path otherwise.
CARGO_OUT_DIR="${CARGO_TARGET_DIR:-$TAURI_DIR/target}"

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

SIDECARS=(woom-github woom-jira woom-memory woom-sentry woom-app)

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
# Source is `$CARGO_OUT_DIR/$TARGET/release/$name` so `CARGO_TARGET_DIR`
# overrides land at the right place — see the comment by `CARGO_OUT_DIR`
# above for the bug this avoids.
for name in "${SIDECARS[@]}"; do
  src="$CARGO_OUT_DIR/$TARGET/release/$name"
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
#   <cargo-out>/<triple>/release/bundle/{macos,dmg}/… — this is
# `CARGO_TARGET_DIR` when set, otherwise the workspace `target/`. Same
# reasoning as the `CARGO_OUT_DIR` resolution at the top: when Cursor
# (or a CI runner) redirects cargo to a scratch dir, the bundle ends
# up there, and a hard-coded `$TAURI_DIR/target/...` lookup misses it.
BUNDLE_DIR="$CARGO_OUT_DIR/$TARGET/release/bundle"
APP_PATH=$(find "$BUNDLE_DIR/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -n1 || true)
DMG_PATH=$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -n1 || true)

# Sign + (optionally) notarize the .app + .dmg.
#
# Two paths:
# 1. **Production / 1.0.** When `APPLE_SIGNING_IDENTITY` is set (e.g.
#    `Developer ID Application: Foo (TEAMID)`), sign with that identity,
#    enable hardened runtime, then submit the result to Apple's notary
#    service when `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` are
#    also present. Notarized DMGs open without the "unidentified
#    developer" warning on every Mac the user has — the bar for
#    `docs/ROADMAP_1.0.md §1.3`. Use an app-specific password (not the
#    Apple-ID password) for `APPLE_PASSWORD`.
# 2. **Local dev (default).** Fall back to ad-hoc signing — the .app
#    runs on this machine and `codesign --force --sign -` on every
#    nested binary keeps macOS from killing sidecars on first launch.
#    Quarantine attribute is stripped on the local artifacts so we
#    don't inherit a stale bit between rebuilds.
SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
ENTITLEMENTS_FILE="$TAURI_DIR/Entitlements.plist"

if [[ -n "$APP_PATH" ]]; then
  if [[ -n "$SIGNING_IDENTITY" ]]; then
    echo "→ Developer ID signing $APP_PATH (identity: ${SIGNING_IDENTITY})"
    # --options runtime enables hardened runtime, mandatory for
    # notarization. --timestamp uses Apple's timestamp server, also
    # mandatory. The entitlements file declares the JIT / disable-
    # library-validation needs of WKWebView — without it the app
    # launches but Tauri's renderer can't load.
    if [[ -f "$ENTITLEMENTS_FILE" ]]; then
      codesign --force --deep --options runtime --timestamp \
        --entitlements "$ENTITLEMENTS_FILE" \
        --sign "$SIGNING_IDENTITY" "$APP_PATH"
    else
      codesign --force --deep --options runtime --timestamp \
        --sign "$SIGNING_IDENTITY" "$APP_PATH"
    fi
  else
    echo "→ ad-hoc signing $APP_PATH (set APPLE_SIGNING_IDENTITY to release-sign)"
    # --deep hits every nested binary (sidecars, frameworks). Errors on already-
    # signed items are fine; we use `|| true` so one failed sign doesn't abort
    # the whole build.
    codesign --force --deep --sign - --timestamp=none "$APP_PATH" 2>/dev/null || true
  fi
  xattr -cr "$APP_PATH" 2>/dev/null || true
fi

# Notarization: only when caller provides ALL three creds. Submit the
# DMG (Apple prefers DMG to .zip for desktop apps), wait for it to
# settle, then staple the ticket so the .app + .dmg work offline. We
# notarize the DMG rather than the .app so end users get the staple
# embedded in the DMG too — opening the DMG triggers Gatekeeper, and
# the staple short-circuits the network round-trip there.
NOTARIZE_OK=0
if [[ -n "$DMG_PATH" && -n "$SIGNING_IDENTITY" \
      && -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  echo "→ notarizing $DMG_PATH (Apple ID: ${APPLE_ID})"
  # `notarytool submit --wait` blocks until Apple finishes scanning;
  # exit 0 means accepted, anything else means rejected. We don't pipe
  # to `tee` because the `submit` log is already concise.
  if xcrun notarytool submit "$DMG_PATH" \
       --apple-id "$APPLE_ID" \
       --password "$APPLE_PASSWORD" \
       --team-id "$APPLE_TEAM_ID" \
       --wait; then
    echo "→ stapling notary ticket onto $DMG_PATH"
    # `stapler staple` embeds the ticket into the DMG so Gatekeeper
    # can validate offline. Failure here is recoverable (the artifact
    # is still notarized; just the offline-validation cache misses)
    # but loud — surface it.
    xcrun stapler staple "$DMG_PATH" || echo "  (stapler failed; DMG is still notarized)"
    NOTARIZE_OK=1
  else
    echo "  (notarytool rejected the submission — see Apple's log above)" >&2
  fi
fi

if [[ -n "$DMG_PATH" ]]; then
  xattr -cr "$DMG_PATH" 2>/dev/null || true
fi

echo ""
echo "Done."
[[ -n "$APP_PATH" ]] && echo "  .app: $APP_PATH"
if [[ -n "$DMG_PATH" ]]; then
  if (( NOTARIZE_OK )); then
    echo "  .dmg: $DMG_PATH (signed + notarized)"
  elif [[ -n "$SIGNING_IDENTITY" ]]; then
    echo "  .dmg: $DMG_PATH (signed, NOT notarized — set APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID to enable)"
  else
    echo "  .dmg: $DMG_PATH (ad-hoc signed — set APPLE_SIGNING_IDENTITY for Developer ID + APPLE_ID etc. for notarization)"
  fi
fi
if [[ -z "$APP_PATH" && -z "$DMG_PATH" ]]; then
  echo "  (no bundles found under $BUNDLE_DIR — check tauri build output above)" >&2
  exit 1
fi
# Explicit success — the previous `[[ ... ]]` returned non-zero when at
# least one bundle existed, which bash propagated as the script's exit
# code, making CI think the build failed despite the artefacts being
# on disk. Anchor the exit so that's no longer a worry.
exit 0
