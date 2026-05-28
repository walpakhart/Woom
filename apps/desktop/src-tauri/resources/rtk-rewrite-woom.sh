#!/usr/bin/env bash
# rtk-hook-version: 1 (woom-managed)
# Claude Code PreToolUse hook installed by Woom. Reads the tool-call
# JSON envelope from stdin and either:
#   - exits 0 + JSON envelope on stdout (rewrite to compact `rtk <cmd>`),
#   - exits 0 with no stdout (passthrough — when Woom signals
#     per-session opt-out via WOOM_RTK_SESSION_DISABLED=1, or when the
#     bundled rtk binary is unavailable).
#
# Delegates rewrite logic to `rtk hook claude` (native subcommand,
# RTK ≥ 0.42) — no jq dependency required. The Rust copy step
# substitutes the absolute path to the bundled rtk binary into the
# RTK_BIN placeholder below before chmod-ing the file at
# `~/.local/share/woom/hooks/rtk-rewrite-woom.sh` (macOS:
# `~/Library/Application Support/woom/hooks/...`).

set -u

# Per-session opt-out (composer "RTK off" pill in Phase 4 sets this
# in the spawned `claude` CLI's environment). When set, we exit
# cleanly without writing an envelope, which tells Claude Code to
# run the original command unchanged.
if [ "${WOOM_RTK_SESSION_DISABLED:-0}" = "1" ]; then
  exit 0
fi

# Absolute path to the bundled rtk binary. The Rust-side
# `copy_wrapper_script` swaps `__WOOM_RTK_BIN__` for the real path
# (.app's Contents/MacOS/rtk-<triple> or dev `target/debug/...`).
RTK_BIN="__WOOM_RTK_BIN__"

if [ ! -x "$RTK_BIN" ]; then
  # Bundled rtk got moved / corrupted between installs. Stay quiet on
  # stdout so Claude Code passes the original command through; surface
  # the failure on stderr so it lands in the user's Claude log.
  echo "[woom-rtk] bundled rtk binary missing at $RTK_BIN — passthrough" >&2
  exit 0
fi

# Hand stdin straight to the native handler. The "[rtk] /!\ No hook
# installed" advisory rtk prints to stderr is informational only
# (intended for users running `rtk hook claude` from a terminal); we
# suppress it because in our setup the hook IS installed.
exec "$RTK_BIN" hook claude 2>/dev/null
