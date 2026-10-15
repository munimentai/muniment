//! Fleet update: which paired runtimes run an older version than this app.

use std::collections::BTreeSet;

use muniment_core::attach::{SCOPE_RUNTIME_ADMIN, SCOPE_RUNTIME_UPDATE};

/// This app's version from `package.json`, which an update request names.
pub(crate) const APP_VERSION: &str = env!("MUNIMENT_APP_VERSION");

fn parse(version: &str) -> Option<semver::Version> {
    let version = version.trim();
    semver::Version::parse(version.strip_prefix('v').unwrap_or(version)).ok()
}

/// Whether `runtime` is an older version than `app`. A version that is not
/// semver compares as unknown, so the app offers no update for it.
pub(crate) fn is_older(runtime: &str, app: &str) -> Option<bool> {
    Some(parse(runtime)? < parse(app)?)
}

/// Whether this device may ask a runtime to update: it holds the update or
/// the admin scope there.
pub(crate) fn may_update(scopes: &BTreeSet<String>) -> bool {
    scopes.contains(SCOPE_RUNTIME_UPDATE) || scopes.contains(SCOPE_RUNTIME_ADMIN)
}

/// Whether the app asks a runtime to update: it runs an older version and
/// this device may ask.
pub(crate) fn update_offered(
    runtime_version: Option<&str>,
    scopes: &BTreeSet<String>,
    app_version: &str,
) -> bool {
    may_update(scopes)
        && runtime_version.and_then(|version| is_older(version, app_version)) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_semver() {
        assert_eq!(is_older("0.1.9", "0.2.0"), Some(true));
        assert_eq!(is_older("0.2.0", "0.2.0"), Some(false));
        assert_eq!(is_older("0.10.0", "0.9.0"), Some(false));
        assert_eq!(is_older("v1.0.0", "1.0.1"), Some(true));
        assert_eq!(is_older("1.0.0-rc.1", "1.0.0"), Some(true));
        assert_eq!(is_older("nightly", "1.0.0"), None);
        assert_eq!(is_older("1.0.0", "dev"), None);
    }

    #[test]
    fn an_update_needs_an_older_version_and_the_update_or_admin_scope() {
        let admin = BTreeSet::from(["thread.read".to_owned(), "runtime.admin".to_owned()]);
        let reader = BTreeSet::from(["thread.read".to_owned(), "run.write".to_owned()]);
        let updater = BTreeSet::from(["thread.read".to_owned(), "runtime.update".to_owned()]);
        assert!(update_offered(Some("0.1.0"), &updater, "0.2.0"));
        assert!(!update_offered(Some("0.2.0"), &updater, "0.2.0"));
        assert!(update_offered(Some("0.1.0"), &admin, "0.2.0"));
        assert!(!update_offered(Some("0.1.0"), &reader, "0.2.0"));
        assert!(!update_offered(Some("0.2.0"), &admin, "0.2.0"));
        assert!(!update_offered(Some("0.3.0"), &admin, "0.2.0"));
        assert!(!update_offered(None, &admin, "0.2.0"));
        assert!(!update_offered(Some("unknown"), &admin, "0.2.0"));
    }
}
