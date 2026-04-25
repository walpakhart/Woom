//! macOS Keychain storage for source credentials.
//!
//! Items are stored under the `com.forgehold.desktop` service with
//! `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` — the keychain is only
//! readable while the user is logged in, items are bound to this Mac (no
//! iCloud Keychain sync), and they don't survive a wipe. The app-level
//! Touch ID gate (`biometric_unlock` at startup, see `biometry.rs`) is
//! what actually keeps a thief who steals the unlocked Mac out of the
//! tokens during the session.
//!
//! We previously layered `kSecAccessControlUserPresence` on every item to
//! force a Touch ID prompt on each read. That works on Developer-ID
//! signed builds, but ad-hoc signed builds (signingIdentity "-") fail
//! `SecItemAdd` with `errSecMissingEntitlement (-34018)` because the
//! biometric ACL requires keychain-access-groups entitlement that
//! ad-hoc bundles don't have. Dropping the per-item ACL fixes that and
//! also stops the Touch ID prompt from firing dozens of times per session
//! while the inbox refreshes — a bad UX trade for security gain we don't
//! actually realize (the app-level gate already covers this).

use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use security_framework_sys::{
    base::errSecItemNotFound,
    item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecMatchLimit,
        kSecReturnData, kSecValueData,
    },
    keychain_item::{SecItemAdd, SecItemCopyMatching, SecItemDelete},
};
use std::ptr;

// `kSecAttrAccessible` and the accessibility-tier constants live in
// Security.framework but aren't re-exported by the `security-framework-sys`
// version we depend on. Declare them as `extern "C"` statics — they're
// CFStringRef constants, stable since macOS 10.10.
extern "C" {
    static kSecAttrAccessible: CFStringRef;
    static kSecAttrAccessibleWhenUnlockedThisDeviceOnly: CFStringRef;
}

const SERVICE: &str = "com.forgehold.desktop";

#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("keychain: {0}")]
    Sf(String),
    #[error("stored value is not valid UTF-8")]
    Utf8,
}

fn check(status: i32) -> Result<(), KeychainError> {
    if status == 0 {
        Ok(())
    } else {
        Err(KeychainError::Sf(format!("OSStatus {status}")))
    }
}

fn cf_string_from_ref(r: core_foundation::string::CFStringRef) -> CFString {
    unsafe { CFString::wrap_under_get_rule(r) }
}

pub fn set(key: &str, value: &str) -> Result<(), KeychainError> {
    // SecItemAdd returns errSecDuplicateItem if (service, account) exists.
    // Delete first so writes are idempotent and so we always pick up the
    // current accessibility attribute (handy when migrating ACLs).
    let _ = delete(key);
    let pairs: Vec<(CFString, CFType)> = vec![
        (cf_string_from_ref(unsafe { kSecClass }), cf_string_from_ref(unsafe { kSecClassGenericPassword }).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrService }), CFString::new(SERVICE).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrAccount }), CFString::new(key).as_CFType()),
        (cf_string_from_ref(unsafe { kSecValueData }), CFData::from_buffer(value.as_bytes()).as_CFType()),
        (
            cf_string_from_ref(unsafe { kSecAttrAccessible }),
            cf_string_from_ref(unsafe { kSecAttrAccessibleWhenUnlockedThisDeviceOnly }).as_CFType(),
        ),
    ];
    let dict = CFDictionary::from_CFType_pairs(&pairs);
    let status = unsafe { SecItemAdd(dict.as_concrete_TypeRef(), ptr::null_mut()) };
    check(status)
}

pub fn get(key: &str) -> Result<Option<String>, KeychainError> {
    let pairs: Vec<(CFString, CFType)> = vec![
        (cf_string_from_ref(unsafe { kSecClass }), cf_string_from_ref(unsafe { kSecClassGenericPassword }).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrService }), CFString::new(SERVICE).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrAccount }), CFString::new(key).as_CFType()),
        (cf_string_from_ref(unsafe { kSecReturnData }), CFBoolean::true_value().as_CFType()),
        (cf_string_from_ref(unsafe { kSecMatchLimit }), CFNumber::from(1i64).as_CFType()),
    ];
    let dict = CFDictionary::from_CFType_pairs(&pairs);
    let mut result: CFTypeRef = ptr::null_mut();
    let status = unsafe { SecItemCopyMatching(dict.as_concrete_TypeRef(), &mut result) };
    if status == errSecItemNotFound {
        return Ok(None);
    }
    check(status)?;
    if result.is_null() {
        return Ok(None);
    }
    let data: CFData = unsafe { CFData::wrap_under_create_rule(result as _) };
    let bytes = data.bytes().to_vec();
    String::from_utf8(bytes).map(Some).map_err(|_| KeychainError::Utf8)
}

pub fn delete(key: &str) -> Result<(), KeychainError> {
    let pairs: Vec<(CFString, CFType)> = vec![
        (cf_string_from_ref(unsafe { kSecClass }), cf_string_from_ref(unsafe { kSecClassGenericPassword }).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrService }), CFString::new(SERVICE).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrAccount }), CFString::new(key).as_CFType()),
    ];
    let dict = CFDictionary::from_CFType_pairs(&pairs);
    let status = unsafe { SecItemDelete(dict.as_concrete_TypeRef()) };
    if status == errSecItemNotFound {
        return Ok(());
    }
    check(status)
}
