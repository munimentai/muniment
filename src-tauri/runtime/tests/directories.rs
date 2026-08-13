use std::ffi::OsStr;
use std::path::Path;

use muniment_runtime::{resolve_directory, DirectoryUnavailableError, APPLICATION_IDENTIFIER};

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
