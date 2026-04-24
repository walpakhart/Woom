//! Touch ID / device owner authentication via LocalAuthentication's
//! `LAContext`. Used to gate the app at launch so secrets in the legacy
//! macOS keychain sit behind a biometric confirmation, even though the
//! keychain items themselves don't carry an `SecAccessControl` flag
//! (that path needs a real Apple Developer ID — see keychain.rs).
//!
//! The Obj-C `evaluatePolicy:localizedReason:reply:` method takes a
//! completion block that fires on the main queue. We bridge it to Rust
//! with a channel: the block sends its outcome, we block on `recv`. This
//! is fine from a tokio task (we park one thread), and the Tauri main
//! thread is free to pump the LAContext's UI events while we wait.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

const TIMEOUT_SECS: u64 = 120;

/// Run the LAContext flow off the main tokio worker — it's a blocking call
/// that parks until the user taps / cancels / times out.
pub async fn authenticate(reason: &str) -> Result<(), String> {
    let reason = reason.to_string();
    tokio::task::spawn_blocking(move || authenticate_blocking(&reason))
        .await
        .map_err(|e| format!("biometry: join failed: {e}"))?
}

fn authenticate_blocking(reason: &str) -> Result<(), String> {
    // DeviceOwnerAuthentication = Touch ID first, Mac passcode fallback.
    // DeviceOwnerAuthenticationWithBiometrics would refuse to fall back,
    // which is too strict for users without an enrolled fingerprint.
    let policy = LAPolicy::DeviceOwnerAuthentication;

    let context: Retained<LAContext> = unsafe { LAContext::new() };

    // Pre-flight: tells us Touch ID / passcode is at all possible. Returning
    // a clearer error up front beats showing an empty prompt.
    if let Err(err) = unsafe { context.canEvaluatePolicy_error(policy) } {
        return Err(ns_error_description(&err));
    }

    let reason_ns = NSString::from_str(reason);
    let (tx, rx) = channel::<Result<(), String>>();
    // Wrap in Mutex<Option<_>> so the (possibly-called-twice) block can take
    // the sender out on first firing and no-op on spurious replays.
    let tx = Mutex::new(Some(tx));

    let block = RcBlock::new(move |success: Bool, error: *mut NSError| {
        let result: Result<(), String> = if success.as_bool() {
            Ok(())
        } else {
            let msg = unsafe { ns_error_from_raw(error) }
                .unwrap_or_else(|| "authentication cancelled".into());
            Err(msg)
        };
        if let Ok(mut guard) = tx.lock() {
            if let Some(sender) = guard.take() {
                let _ = sender.send(result);
            }
        }
    });
    unsafe {
        context.evaluatePolicy_localizedReason_reply(policy, &reason_ns, &block);
    }

    // Keep `context` alive until the block fires: the LAContext docs note
    // that dropping it mid-eval cancels the prompt. We pin it by shadowing
    // below, but the `recv_timeout` already keeps it in scope.
    let outcome = match rx.recv_timeout(Duration::from_secs(TIMEOUT_SECS)) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err("biometric prompt timed out".into()),
        Err(RecvTimeoutError::Disconnected) => {
            Err("biometric prompt cancelled (channel closed)".into())
        }
    };
    drop(context);
    outcome
}

fn ns_error_description(err: &NSError) -> String {
    err.localizedDescription().to_string()
}

unsafe fn ns_error_from_raw(err: *mut NSError) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let err_ref: &NSError = &*err;
    Some(err_ref.localizedDescription().to_string())
}
