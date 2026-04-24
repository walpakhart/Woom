//! macOS Keychain storage for source credentials — Touch ID gated.
//!
//! Items are stored under the `com.forgehold.desktop` service with an
//! access control that requires `UserPresence` (Touch ID with Mac passcode
//! fallback). Tagged `AccessibleWhenUnlockedThisDeviceOnly` so they don't
//! sync to iCloud Keychain and stay bound to this Mac. Each read invokes
//! the native Touch ID UI via LocalAuthentication.

use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework_sys::{
    base::errSecItemNotFound,
    item::{
        kSecAttrAccessControl, kSecAttrAccount, kSecAttrService, kSecClass,
        kSecClassGenericPassword, kSecMatchLimit, kSecReturnData, kSecValueData,
    },
    keychain_item::{SecItemAdd, SecItemCopyMatching, SecItemDelete},
};
use std::ptr;

const SERVICE: &str = "com.forgehold.desktop";

/// `kSecAccessControlUserPresence` — bit 0. Lets macOS pick the best
/// authentication UI: Touch ID first, Mac passcode if biometry is
/// unavailable. We don't track enrolled finger set (that would be
/// `BiometryCurrentSet` = bit 3), so re-enrolling fingers keeps the item
/// accessible. `CFOptionFlags` is a `usize` on Apple platforms.
const USER_PRESENCE: usize = 1;

#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("keychain: {0}")]
    Sf(String),
    #[error("stored value is not valid UTF-8")]
    Utf8,
}

impl From<security_framework::base::Error> for KeychainError {
    fn from(e: security_framework::base::Error) -> Self {
        KeychainError::Sf(format!("code {} — {}", e.code(), e))
    }
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

fn access_control() -> Result<SecAccessControl, KeychainError> {
    SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
        USER_PRESENCE,
    )
    .map_err(KeychainError::from)
}

pub fn set(key: &str, value: &str) -> Result<(), KeychainError> {
    // SecItemAdd returns errSecDuplicateItem if (service, account) exists.
    // Delete first to make writes idempotent.
    let _ = delete(key);
    let ac = access_control()?;
    let pairs: Vec<(CFString, CFType)> = vec![
        (cf_string_from_ref(unsafe { kSecClass }), cf_string_from_ref(unsafe { kSecClassGenericPassword }).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrService }), CFString::new(SERVICE).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrAccount }), CFString::new(key).as_CFType()),
        (cf_string_from_ref(unsafe { kSecValueData }), CFData::from_buffer(value.as_bytes()).as_CFType()),
        (cf_string_from_ref(unsafe { kSecAttrAccessControl }), ac.as_CFType()),
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
