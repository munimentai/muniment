use std::ffi::OsStr;
use std::path::Path;

use muniment_runtime::{
    installed_desktop_executable, installed_desktop_executable_from, macos_log_directory_from_home,
    resolve_directory, windows_log_directory_from_local_app_data,
    windows_state_directory_from_app_data, DirectoryUnavailableError, APPLICATION_IDENTIFIER,
};

#[test]
fn maps_the_installed_runtime_to_the_desktop_executable() {
    assert_eq!(
        installed_desktop_executable_from(Path::new("/usr/lib/muniment/muniment-runtime")),
        Some(Path::new("/usr/bin/muniment-desktop").to_path_buf())
    );
    assert_eq!(
        installed_desktop_executable_from(Path::new("/opt/muniment/lib/muniment/muniment-runtime")),
        Some(Path::new("/opt/muniment/bin/muniment-desktop").to_path_buf())
    );
}

#[test]
fn maps_the_appimage_runtime_to_the_desktop_without_a_deb_alias() {
    let root = Path::new("/tmp/.mount_muniment/usr");
    assert_eq!(
        installed_desktop_executable_from(&root.join("lib/muniment/muniment-runtime")),
        Some(root.join("bin/muniment-desktop"))
    );
}

#[cfg(target_os = "linux")]
#[test]
fn maps_the_development_runtime_in_every_build_profile() {
    for directory in [
        "/workspace/target/debug",
        "/workspace/target/x86_64-unknown-linux-gnu/debug",
        "/tmp/custom-target/debug",
    ] {
        let directory = Path::new(directory);
        assert_eq!(
            installed_desktop_executable_from(&directory.join("muniment-runtime")),
            Some(directory.join("muniment-desktop"))
        );
    }
    for path in [
        "target/debug/muniment-runtime",
        "/workspace/target/debug/another-runtime",
        "/workspace/target/debug/deps/muniment-runtime",
        "/workspace/target/muniment-runtime",
    ] {
        assert_eq!(installed_desktop_executable_from(Path::new(path)), None);
    }
}

#[test]
fn maps_the_bundled_macos_runtime_to_the_desktop_executable() {
    for bundle in [
        "/Applications/muniment.app",
        "/Users/person/My Apps/muniment.app",
    ] {
        assert_eq!(
            installed_desktop_executable_from(
                &Path::new(bundle).join("Contents/Library/LaunchServices/muniment-runtime")
            ),
            Some(Path::new(bundle).join("Contents/MacOS/muniment-desktop"))
        );
    }
}

#[test]
fn rejects_runtime_paths_outside_the_macos_bundle_layout() {
    for path in [
        "",
        "muniment.app/Contents/Library/LaunchServices/muniment-runtime",
        "/Applications/muniment.app/Contents/Library/LaunchServices/another-runtime",
        "/Applications/muniment.app/Contents/Library/Other/muniment-runtime",
        "/Applications/muniment.app/Contents/Other/LaunchServices/muniment-runtime",
        "/Applications/muniment.app/Other/Library/LaunchServices/muniment-runtime",
        "/Applications/muniment.app/Contents/LaunchServices/muniment-runtime",
        "/Applications/muniment.app/Contents/MacOS/muniment-runtime",
    ] {
        assert_eq!(installed_desktop_executable_from(Path::new(path)), None);
    }
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

#[test]
fn maps_an_absolute_app_data_root_to_the_windows_state_directory() {
    assert_eq!(
        windows_state_directory_from_app_data(Path::new("/Users/person/AppData/Roaming")).unwrap(),
        Path::new("/Users/person/AppData/Roaming/ai.muniment.desktop")
    );
}

#[test]
fn rejects_a_relative_windows_app_data_root() {
    assert_eq!(
        windows_state_directory_from_app_data(Path::new("AppData/Roaming")),
        Err(DirectoryUnavailableError)
    );
}

#[test]
fn preserves_a_parent_segment_in_the_windows_app_data_root() {
    assert_eq!(
        windows_state_directory_from_app_data(Path::new("/Users/person/AppData/Local/../Roaming"))
            .unwrap(),
        Path::new("/Users/person/AppData/Local/../Roaming/ai.muniment.desktop")
    );
}

#[test]
fn maps_an_absolute_local_app_data_path_to_the_windows_log_directory() {
    assert_eq!(
        windows_log_directory_from_local_app_data(Path::new("/Users/person/AppData/Local"))
            .unwrap(),
        Path::new("/Users/person/AppData/Local/muniment/logs")
    );
    assert_eq!(
        windows_log_directory_from_local_app_data(Path::new("relative/AppData/Local")),
        Err(DirectoryUnavailableError)
    );
}

#[test]
fn maps_an_absolute_home_to_the_macos_log_directory() {
    assert_eq!(
        macos_log_directory_from_home(Path::new("/Users/person")).unwrap(),
        Path::new("/Users/person/Library/Logs/Muniment")
    );
    assert_eq!(
        macos_log_directory_from_home(Path::new("relative/home")),
        Err(DirectoryUnavailableError)
    );
}
