use super::*;

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn runtime_upgrade_pending(client: &DesktopClientHolder) -> bool {
    runtime_version_upgrade_pending(client.runtime_version().as_deref())
}

#[cfg(any(unix, target_os = "windows"))]
pub(crate) fn runtime_version_compatible(version: &str) -> bool {
    !runtime_version_upgrade_pending(Some(version))
}

#[cfg(any(unix, target_os = "windows"))]
pub(super) fn runtime_version_upgrade_pending(connected_version: Option<&str>) -> bool {
    let minimum = semver::Version::parse(MINIMUM_COMPATIBLE_RUNTIME_VERSION)
        .expect("minimum compatible runtime version must be valid");
    connected_version
        .and_then(|version| semver::Version::parse(version).ok())
        .map_or(true, |version| version < minimum)
}
