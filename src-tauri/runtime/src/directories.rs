use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use muniment_runtime_core::DirectoryUnavailableError;

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
    // Tauri stages the release runtime beside the debug desktop, regardless of the runtime's build profile.
    #[cfg(target_os = "linux")]
    if resource_directory.ends_with("debug") {
        return Some(resource_directory.join("muniment-desktop"));
    }
    if resource_directory.file_name() != Some(OsStr::new("muniment")) {
        return None;
    }

    let library_directory = resource_directory.parent()?;
    if library_directory.file_name() != Some(OsStr::new("lib")) {
        return None;
    }

    Some(library_directory.parent()?.join("bin/muniment-desktop"))
}

pub fn installed_desktop_executable() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| installed_desktop_executable_from(&path))
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
    // Keep diagnostics outside the per-user install directory.
    Ok(local_app_data.join(APPLICATION_IDENTIFIER).join("logs"))
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

/// Resolves the effective user's home from the macOS user database. A test
/// run, or a process `MUNIMENT_LOG_DIR` points elsewhere, logs there instead.
#[cfg(target_os = "macos")]
pub fn effective_user_macos_log_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    if let Some(directory) = muniment_core::state_root::log_directory_override() {
        return Ok(directory);
    }
    let home = muniment_core::user_diagnostics::effective_user_home()
        .map_err(|_| DirectoryUnavailableError)?;
    macos_log_directory_from_home(&home)
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    #[test]
    fn log_directory_uses_the_local_application_data_root() {
        let local_app_data = muniment_core::windows_known_folders::windows_local_app_data()
            .expect("local application data should resolve");

        assert_eq!(windows_local_app_data(), Ok(local_app_data.clone()));
        assert_eq!(
            windows_log_directory(),
            Ok(local_app_data.join(APPLICATION_IDENTIFIER).join("logs"))
        );
    }
}
