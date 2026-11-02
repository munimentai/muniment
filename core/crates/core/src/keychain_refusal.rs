//! How a keychain refusal reads: a declined prompt is not a missing secret.
//! A delete here never reads the secret, so it raises no prompt for it.

use crate::model_router::credentials::CredentialRefusal;

/// The refusal a keyring error names. On macOS a declined prompt
/// (`errSecUserCanceled`), a failed authentication (`errSecAuthFailed`) and a
/// prompt the keychain may not show (`errSecInteractionNotAllowed`) are
/// declines. Every other failure is a store failure.
pub fn keyring_refusal(error: &keyring::Error) -> CredentialRefusal {
    let message = format!(
        "The keychain refused the secret: {}.",
        error.to_string().trim_end_matches('.')
    );
    #[cfg(target_os = "macos")]
    if let keyring::Error::PlatformFailure(inner) | keyring::Error::NoStorageAccess(inner) = error {
        if let Some(status) = inner.downcast_ref::<security_framework::base::Error>() {
            return keychain_status_refusal(status.code(), message);
        }
    }
    CredentialRefusal::Failed(message)
}

/// The refusal a macOS keychain status names.
pub fn keychain_status_refusal(status: i32, message: String) -> CredentialRefusal {
    const USER_CANCELED: i32 = -128;
    const AUTH_FAILED: i32 = -25293;
    const INTERACTION_NOT_ALLOWED: i32 = -25308;
    match status {
        USER_CANCELED | AUTH_FAILED | INTERACTION_NOT_ALLOWED => {
            CredentialRefusal::Declined(message)
        }
        _ => CredentialRefusal::Failed(message),
    }
}

/// Deletes the generic password item `service` holds for `account` without
/// reading its secret. The keyring's own macOS delete first finds the item
/// with its password, which asks the person for the secret, so a person who
/// declined the key would see a prompt for its removal too. A missing item
/// answers [`keyring::Error::NoEntry`].
pub fn delete_without_reading(service: &str, account: &str) -> Result<(), keyring::Error> {
    #[cfg(target_os = "macos")]
    {
        const ITEM_NOT_FOUND: i32 = -25300;
        security_framework::passwords::delete_generic_password(service, account).map_err(|error| {
            match error.code() {
                ITEM_NOT_FOUND => keyring::Error::NoEntry,
                _ => keyring::Error::PlatformFailure(Box::new(error)),
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        keyring::Entry::new(service, account).and_then(|entry| entry.delete_credential())
    }
}

/// The keyring error a held refusal answers, so a caller that speaks the
/// keyring's errors sees the refusal without the system hearing a request.
pub fn refusal_error(refusal: &CredentialRefusal) -> keyring::Error {
    keyring::Error::NoStorageAccess(Box::new(std::io::Error::other(
        refusal.message().to_owned(),
    )))
}

/// The keyring error a macOS keychain status raises, as tests build one.
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub fn status_error(status: i32) -> keyring::Error {
    keyring::Error::PlatformFailure(Box::new(security_framework::base::Error::from_code(status)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declined_prompt_reads_as_a_decline_and_other_statuses_as_failures() {
        for status in [-128, -25293, -25308] {
            assert!(matches!(
                keychain_status_refusal(status, "x".into()),
                CredentialRefusal::Declined(_)
            ));
        }
        for status in [-25291, -25295, -34018, -50] {
            assert!(matches!(
                keychain_status_refusal(status, "x".into()),
                CredentialRefusal::Failed(_)
            ));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_keyring_error_carries_the_keychain_status() {
        let canceled = keyring::Error::PlatformFailure(Box::new(
            security_framework::base::Error::from_code(-128),
        ));
        assert!(matches!(
            keyring_refusal(&canceled),
            CredentialRefusal::Declined(_)
        ));
        let missing = keyring::Error::NoStorageAccess(Box::new(
            security_framework::base::Error::from_code(-25294),
        ));
        assert!(matches!(
            keyring_refusal(&missing),
            CredentialRefusal::Failed(_)
        ));
        assert!(matches!(
            keyring_refusal(&keyring::Error::NoEntry),
            CredentialRefusal::Failed(_)
        ));
    }
}
