use std::ffi::OsStr;
use std::path::Path;

use muniment_runtime::{
    installed_desktop_executable, installed_desktop_executable_from, resolve_directory,
    DirectoryUnavailableError, APPLICATION_IDENTIFIER,
};

#[test]
fn maps_the_installed_runtime_to_the_desktop_executable() {
    assert_eq!(
        installed_desktop_executable_from(Path::new("/usr/lib/muniment/muniment-runtime")),
        Some(Path::new("/usr/bin/muniment").to_path_buf())
    );
    assert_eq!(
        installed_desktop_executable_from(Path::new("/opt/muniment/lib/muniment/muniment-runtime")),
        Some(Path::new("/opt/muniment/bin/muniment").to_path_buf())
    );
}

#[test]
fn rejects_runtime_paths_outside_the_installed_layout() {
    assert_eq!(
        installed_desktop_executable_from(Path::new("lib/muniment/muniment-runtime")),
        None
    );
    assert_eq!(
        installed_desktop_executable_from(Path::new("/usr/lib/muniment/another-runtime")),
        None
    );
    assert_eq!(
        installed_desktop_executable_from(Path::new("/usr/lib/another/muniment-runtime")),
        None
    );
    assert_eq!(
        installed_desktop_executable_from(Path::new("/usr/share/muniment/muniment-runtime")),
        None
    );
    assert_eq!(
        installed_desktop_executable_from(Path::new("/workspace/target/release/muniment-runtime")),
        None
    );
}

#[test]
fn resolves_the_running_executable_with_the_installed_layout_rule() {
    let expected = std::env::current_exe()
        .ok()
        .and_then(|path| installed_desktop_executable_from(&path));

    assert_eq!(installed_desktop_executable(), expected);
}

#[test]
fn uses_an_absolute_xdg_directory() {
    let directory = resolve_directory(
        Some(OsStr::new("/xdg/data")),
        Some(OsStr::new("/home/person")),
        ".local/share",
    )
    .unwrap();

    assert_eq!(directory, Path::new("/xdg/data/ai.muniment.desktop"));
}

#[test]
fn skips_a_relative_xdg_directory_and_uses_home() {
    let directory = resolve_directory(
        Some(OsStr::new("relative/data")),
        Some(OsStr::new("/home/person")),
        ".local/share",
    )
    .unwrap();

    assert_eq!(
        directory,
        Path::new("/home/person/.local/share/ai.muniment.desktop")
    );
}

#[test]
fn maps_the_config_fallback() {
    let directory = resolve_directory(None, Some(OsStr::new("/home/person")), ".config").unwrap();

    assert_eq!(
        directory,
        Path::new("/home/person/.config/ai.muniment.desktop")
    );
}

#[test]
fn rejects_missing_and_relative_sources() {
    assert_eq!(
        resolve_directory(None, None, ".config"),
        Err(DirectoryUnavailableError)
    );
    assert_eq!(
        resolve_directory(
            Some(OsStr::new("relative/config")),
            Some(OsStr::new("relative/home")),
            ".config",
        ),
        Err(DirectoryUnavailableError)
    );
}

#[test]
fn tauri_identifier_matches_the_runtime_identifier() {
    let config =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../tauri.conf.json"))
            .unwrap();
    let identifier = format!("\"identifier\": \"{APPLICATION_IDENTIFIER}\"");

    assert!(config.contains(&identifier));
}
