//! Fleet update: which paired runtimes run an older version than the one this
//! app brings them to.

use std::collections::BTreeSet;

use muniment_core::attach::{SCOPE_RUNTIME_ADMIN, SCOPE_RUNTIME_UPDATE};

/// This app's version from `package.json`. The app, the CLI, the server and
/// the container release together under it, so every paired runtime updates
/// to it.
pub(crate) const APP_VERSION: &str = env!("MUNIMENT_APP_VERSION");

/// The version a runtime of `kind` updates to, or none for a kind the app
/// does not know.
pub(crate) fn target_version(kind: Option<&str>) -> Option<&'static str> {
    match kind? {
        "desktop" | "server" | "container" => Some(APP_VERSION),
        _ => None,
    }
}

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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The next minor version after `version`.
    pub(crate) fn bump(version: &str) -> String {
        let parsed = parse(version).unwrap();
        format!("{}.{}.0", parsed.major, parsed.minor + 1)
    }

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
    fn an_update_needs_the_update_or_admin_scope() {
        let admin = BTreeSet::from(["thread.read".to_owned(), "runtime.admin".to_owned()]);
        let reader = BTreeSet::from(["thread.read".to_owned(), "run.write".to_owned()]);
        let updater = BTreeSet::from(["thread.read".to_owned(), "runtime.update".to_owned()]);
        assert!(may_update(&updater));
        assert!(may_update(&admin));
        assert!(!may_update(&reader));
    }

    #[test]
    fn every_runtime_kind_follows_the_app_version() {
        assert_eq!(target_version(Some("desktop")), Some(APP_VERSION));
        assert_eq!(target_version(Some("container")), Some(APP_VERSION));
        assert_eq!(target_version(Some("server")), Some(APP_VERSION));
        assert_eq!(target_version(Some("toaster")), None);
        assert_eq!(target_version(None), None);
        assert!(parse(APP_VERSION).is_some());
    }
}
