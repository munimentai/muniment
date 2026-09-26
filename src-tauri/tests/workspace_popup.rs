// Native windows require the process main thread, so each startup gets its own process.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src"]
mod desktop {
    pub mod window_state;
}

#[cfg(target_os = "macos")]
fn main() {
    use tauri::Manager;

    let mode = std::env::args().nth(1);
    if mode.is_none() {
        for mode in ["fresh", "saved"] {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg(mode)
                .status()
                .unwrap();
            assert!(status.success(), "Popup startup failed with {mode} state");
        }
        return;
    }

    let state_file =
        std::env::temp_dir().join(format!("muniment-popup-state-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&state_file);
    if mode.as_deref() == Some("saved") {
        std::fs::write(&state_file, r#"{"workspace-menu":{"width":480,"height":280,"x":500,"y":400,"prev_x":500,"prev_y":400,"maximized":false,"visible":true,"decorated":false,"fullscreen":false}}"#).unwrap();
    }
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    context.config_mut().app.windows =
        serde_json::from_value(config["app"]["windows"].clone()).unwrap();
    context
        .config_mut()
        .app
        .windows
        .retain(|window| window.label == "workspace-menu");

    tauri::Builder::default()
        .plugin(
            desktop::window_state::plugin_builder()
                .with_filename(state_file.to_string_lossy())
                .build(),
        )
        .build(context)
        .unwrap()
        .run(move |app, event| {
            if matches!(event, tauri::RunEvent::Ready) {
                let popup = app.get_webview_window("workspace-menu").unwrap();
                assert!(
                    !popup.is_visible().unwrap(),
                    "Popup must stay hidden on startup"
                );
                // Excluding restoration must still allow the owner to open and close the popup.
                popup.show().unwrap();
                assert!(popup.is_visible().unwrap());
                popup.hide().unwrap();
                assert!(!popup.is_visible().unwrap());
                app.exit(0);
            }
            if matches!(event, tauri::RunEvent::Exit) {
                let _ = std::fs::remove_file(&state_file);
            }
        });
}

#[cfg(not(target_os = "macos"))]
fn main() {}
