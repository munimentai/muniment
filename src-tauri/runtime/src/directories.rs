use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::ffi::CStr;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStringExt;

pub const APPLICATION_IDENTIFIER: &str = "ai.muniment.desktop";

pub fn installed_desktop_executable_from(runtime_executable: &Path) -> Option<PathBuf> {
    if !runtime_executable.is_absolute()
        || runtime_executable.file_name() != Some(OsStr::new("muniment-runtime"))
    {
        return None;
    }

    let resource_directory = runtime_executable.parent()?;
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

pub fn macos_log_directory_from_home(home: &Path) -> Result<PathBuf, DirectoryUnavailableError> {
    if !home.is_absolute() {
        return Err(DirectoryUnavailableError);
    }
    Ok(home.join("Library/Logs/Muniment"))
}

/// Resolves the effective user's home from the macOS user database.
#[cfg(target_os = "macos")]
pub fn effective_user_macos_log_directory() -> Result<PathBuf, DirectoryUnavailableError> {
    let uid = unsafe { libc::geteuid() };
    let buffer_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let buffer_size = if buffer_size > 0 {
        usize::try_from(buffer_size).map_err(|_| DirectoryUnavailableError)?
    } else {
        16 * 1024
    };
    let mut buffer = vec![0_u8; buffer_size];
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(DirectoryUnavailableError);
    }
    let record = unsafe { record.assume_init() };
    if record.pw_dir.is_null() {
        return Err(DirectoryUnavailableError);
    }
    let home = unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes();
    macos_log_directory_from_home(&PathBuf::from(std::ffi::OsString::from_vec(home.to_vec())))
}
