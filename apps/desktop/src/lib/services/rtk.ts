// RTK integration — TS-side mirror of `src-tauri/src/rtk.rs::RtkStatus`.
// The Composer pill (Phase 4) reads this to choose between on / off /
// error / unavailable states. The shape MUST stay in lockstep with the
// Rust struct's serde-camelCased fields.

import { invoke } from '@tauri-apps/api/core';

export interface RtkStatus {
  bundledAvailable: boolean;
  bundledPath: string | null;
  bundledVersion: string | null;
  systemPath: string | null;
  systemVersion: string | null;
  jqAvailable: boolean;
  /** RTK ≥ 0.42 ships a native `rtk hook claude` JSON envelope handler;
   *  when true, the Phase-2 wrapper script can skip jq entirely. */
  usesNativeHook: boolean;
  /** false on native Windows — pill should hide itself there. */
  platformSupported: boolean;
}

/** Snapshot RTK availability from the Rust side. Cheap enough to call
 *  per component-mount; the Rust impl shells out for `--version` and
 *  `hook claude --help` with a 2s deadline. Returns a fully-defaulted
 *  object on IPC failure so the UI never has to guard against null. */
export async function getRtkStatus(): Promise<RtkStatus> {
  try {
    return await invoke<RtkStatus>('rtk_status');
  } catch {
    return {
      bundledAvailable: false,
      bundledPath: null,
      bundledVersion: null,
      systemPath: null,
      systemVersion: null,
      jqAvailable: false,
      usesNativeHook: false,
      platformSupported: false,
    };
  }
}
