use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const APPLICATION_IDENTIFIER: &str = "ai.muniment.desktop";

pub fn installed_desktop_executable_from(runtime_executable: &Path) -> Option<PathBuf> {
    if !runtime_executable.is_absolute()
        || runtime_executable.file_name() != Some(OsStr::new("muniment-runtime"))
    {
        return None;
    }

    let resource_directory = runtime_executable.parent()?;
    if resource_directory.ends_with("Contents/Library/LaunchServices") {
        return Some(
            resource_directory
                .parent()?
                .parent()?
                .join("MacOS/muniment-desktop"),
        );
    }
    if resource_directory.file_name() != Some(OsStr::new("muniment")) {
        return None;
    }

    let library_directory = resource_directory.parent()?;
    if library_directory.file_name() != Some(OsStr::new("lib")) {
        return None;
    }

    Some(library_directory.parent()?.join("bin/muniment"))
}

pub fn installed_desktop_executable() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| installed_desktop_executable_from(&path))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectoryUnavailableError;

impl std::fmt::Display for DirectoryUnavailableError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("directory is unavailable")
    }
}

impl std::error::Error for DirectoryUnavailableError {}

pub fn resolve_directory(
    xdg_value: Option<&OsStr>,
    home_value: Option<&OsStr>,
    fallback_segment: impl AsRef<Path>,
) -> Result<PathBuf, DirectoryUnavailableError> {
    xdg_value
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            home_value
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(fallback_segment))
        })
        .map(|path| path.join(APPLICATION_IDENTIFIER))
        .ok_or(DirectoryUnavailableError)
}

pub fn windows_state_directory_from_app_data(
    app_data: &Path,
) -> Result<PathBuf, DirectoryUnavailableError> {
    if !app_data.is_absolute() {
        return Err(DirectoryUnavailableError);
    }
    Ok(app_data.join(APPLICATION_IDENTIFIER))
}

#[cfg(target_os = "windows")]
fn windows_state_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let app_data = muniment_core::windows_known_folders::windows_roaming_app_data()
        .map_err(|_| DirectoryUnavailableError)?;
    windows_state_directory_from_app_data(&app_data)
}

#[cfg(target_os = "windows")]
pub fn profile_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    windows_state_directory()
}

#[cfg(not(target_os = "windows"))]
pub fn profile_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let xdg_value = std::env::var_os("XDG_DATA_HOME");
    let home_value = std::env::var_os("HOME");
    resolve_directory(xdg_value.as_deref(), home_value.as_deref(), ".local/share")
}

#[cfg(target_os = "windows")]
pub fn config_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    windows_state_directory()
}

#[cfg(not(target_os = "windows"))]
pub fn config_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let xdg_value = std::env::var_os("XDG_CONFIG_HOME");
    let home_value = std::env::var_os("HOME");
    resolve_directory(xdg_value.as_deref(), home_value.as_deref(), ".config")
}

pub fn macos_log_directory_from_home(home: &Path) -> Result<PathBuf, DirectoryUnavailableError> {
    if !home.is_absolute() {
        return Err(DirectoryUnavailableError);
    }
    Ok(home.join("Library/Logs/Muniment"))
}

pub fn windows_log_directory_from_local_app_data(
    local_app_data: &Path,
) -> Result<PathBuf, DirectoryUnavailableError> {
    if !local_app_data.is_absolute() {
        return Err(DirectoryUnavailableError);
    }
    Ok(local_app_data.join("muniment/logs"))
}

#[cfg(target_os = "windows")]
pub fn windows_local_app_data() -> Result<PathBuf, DirectoryUnavailableError> {
    muniment_core::windows_known_folders::windows_local_app_data()
        .map_err(|_| DirectoryUnavailableError)
}

#[cfg(target_os = "windows")]
pub fn windows_log_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    windows_log_directory_from_local_app_data(&windows_local_app_data()?)
}

/// Resolves the effective user's home from the macOS user database.
#[cfg(target_os = "macos")]
pub fn effective_user_macos_log_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let home = muniment_core::user_diagnostics::effective_user_home()
        .map_err(|_| DirectoryUnavailableError)?;
    macos_log_directory_from_home(&home)
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    #[test]
    fn profile_directory_uses_the_roaming_known_folder() {
        let roaming_app_data = muniment_core::windows_known_folders::windows_roaming_app_data()
            .expect("roaming application data should resolve");

        assert_eq!(
            profile_directory(),
            Ok(roaming_app_data.join(APPLICATION_IDENTIFIER))
        );
    }

    #[test]
    fn log_directory_uses_the_local_application_data_root() {
        let local_app_data = muniment_core::windows_known_folders::windows_local_app_data()
            .expect("local application data should resolve");

        assert_eq!(windows_local_app_data(), Ok(local_app_data.clone()));
        assert_eq!(
            windows_log_directory(),
            Ok(local_app_data.join("muniment/logs"))
        );
    }
}
