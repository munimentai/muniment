//! The composer opens the native picker directly.
use std::path::PathBuf;

#[tauri::command]
pub async fn chat_pick_attachments(app: tauri::AppHandle) -> Result<Vec<PathBuf>, String> {
    let (send, receive) = std::sync::mpsc::channel();
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(move || {
        let result = pick_mixed();
        let _ = send.send(result);
    })
    .map_err(|_| "The attachment picker could not be opened.")?;
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_dialog::DialogExt;
        app.dialog().file().pick_files(move |files| {
            let result = files
                .unwrap_or_default()
                .into_iter()
                .map(|file| {
                    file.into_path()
                        .map_err(|_| "The selected path is unavailable.".to_string())
                })
                .collect();
            let _ = send.send(result);
        });
    }
    tauri::async_runtime::spawn_blocking(move || receive.recv())
        .await
        .map_err(|_| "The attachment picker could not be opened.")?
        .map_err(|_| "The attachment picker closed unexpectedly.".to_string())?
}

#[cfg(target_os = "macos")]
fn pick_mixed() -> Result<Vec<PathBuf>, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    let mtm = MainThreadMarker::new().ok_or("The attachment picker needs the main thread.")?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(true);
    panel.setCanCreateDirectories(false);
    if panel.runModal() != NSModalResponseOK {
        return Ok(Vec::new());
    }
    panel
        .URLs()
        .iter()
        .map(|url| {
            url.path()
                .map(|path| PathBuf::from(path.to_string()))
                .ok_or_else(|| "The selected path is unavailable.".to_string())
        })
        .collect()
}
