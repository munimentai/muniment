//! How a keychain refusal reads: a declined prompt is not a missing secret.

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
