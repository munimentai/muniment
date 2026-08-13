use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const APPLICATION_IDENTIFIER: &str = "ai.muniment.desktop";

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

pub fn profile_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let xdg_value = std::env::var_os("XDG_DATA_HOME");
    let home_value = std::env::var_os("HOME");
    resolve_directory(xdg_value.as_deref(), home_value.as_deref(), ".local/share")
}

pub fn config_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let xdg_value = std::env::var_os("XDG_CONFIG_HOME");
    let home_value = std::env::var_os("HOME");
    resolve_directory(xdg_value.as_deref(), home_value.as_deref(), ".config")
}
