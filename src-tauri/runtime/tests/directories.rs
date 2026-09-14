use std::path::Path;

use muniment_runtime::{
    config_directory, installed_desktop_executable, installed_desktop_executable_from,
    macos_log_directory_from_home, profile_directory, windows_log_directory_from_local_app_data,
    DirectoryUnavailableError, APPLICATION_IDENTIFIER,
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
fn state_and_config_share_one_root_named_by_the_override_or_the_home() {
    let state = profile_directory().unwrap();
    assert_eq!(config_directory().unwrap(), state);
    assert!(state.is_absolute());
    match std::env::var_os(muniment_core::state_root::STATE_DIRECTORY_OVERRIDE) {
        Some(value) if Path::new(&value).is_absolute() => assert_eq!(state, Path::new(&value)),
        _ => assert!(state.ends_with(muniment_core::state_root::STATE_DIRECTORY_NAME)),
    }
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
fn maps_an_absolute_local_app_data_path_to_the_windows_log_directory() {
    assert_eq!(
        windows_log_directory_from_local_app_data(Path::new("/Users/person/AppData/Local"))
            .unwrap(),
        Path::new("/Users/person/AppData/Local/ai.muniment.desktop/logs")
    );
    assert_eq!(
        windows_log_directory_from_local_app_data(Path::new("relative/AppData/Local")),
        Err(DirectoryUnavailableError)
    );
}

#[test]
fn windows_diagnostics_do_not_prevent_per_user_install_directory_removal() {
    use muniment_runtime::{write_windows_diagnostic, WindowsDiagnosticEvent};
    use std::fs;

    let template = include_str!("../../windows/per-user.wxs");
    assert!(template
        .contains(r#"<SetDirectory Id="INSTALLDIR" Value="[LocalAppDataFolder]{{product_name}}""#));
    assert!(include_str!("../../tauri.conf.json").contains(r#""productName": "muniment""#));

    let root = std::env::temp_dir().join(format!(
        "muniment-log-location-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let install_directory = root.join("muniment");
    fs::create_dir(&install_directory).unwrap();
    let payload = install_directory.join("muniment-desktop.exe");
    fs::write(&payload, b"payload").unwrap();

    let log_directory = windows_log_directory_from_local_app_data(&root).unwrap();
    assert!(!log_directory.starts_with(&install_directory));
    write_windows_diagnostic(&root, WindowsDiagnosticEvent::ActivationFailed).unwrap();
    assert_eq!(
        fs::read_to_string(log_directory.join("runtime.log")).unwrap(),
        "event=activation_failed message=runtime activation failed\n"
    );

    // Model MSI payload removal followed by its empty-directory RemoveFolder action.
    fs::remove_file(payload).unwrap();
    fs::remove_dir(&install_directory).unwrap();
    assert!(!install_directory.exists());
    assert!(log_directory.join("runtime.log").is_file());
    fs::remove_dir_all(root).unwrap();
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
