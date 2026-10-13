use std::path::PathBuf;

use muniment_core::import_preview::{
    extract_selected_zip_entries, preview_export_zip, ExtractedEntry, PreviewErrorKind,
    PreviewManifest,
};
use serde::Serialize;

/// Serialized failure for the import preview: a stable typed kind plus a
/// neutral fallback message. The picker UI branches on `kind` for its copy.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewError {
    pub kind: PreviewErrorKind,
    pub message: &'static str,
}

/// Previews one export ZIP whose path a future file-picker UI supplies. This
/// command reads only the given file: it never scans parent or home
/// directories, calls the model, changes `home.json`, or writes under Home.
#[tauri::command]
pub async fn onboarding_import_preview(
    archive_path: String,
) -> Result<PreviewManifest, ImportPreviewError> {
    tauri::async_runtime::spawn_blocking(move || preview_export_zip(&PathBuf::from(archive_path)))
        .await
        .unwrap_or(Err(PreviewErrorKind::Io))
        .map_err(preview_error)
}

/// Revalidates the chosen archive and reads only the exact normalized member
/// names the user approved in the preview UI. The returned data is not written
/// to Home or sent to a model by this command.
#[tauri::command]
pub async fn onboarding_import_extract(
    archive_path: String,
    selected_names: Vec<String>,
) -> Result<Vec<ExtractedEntry>, ImportPreviewError> {
    tauri::async_runtime::spawn_blocking(move || {
        extract_selected_zip_entries(&PathBuf::from(archive_path), &selected_names)
    })
    .await
    .unwrap_or(Err(PreviewErrorKind::Io))
    .map_err(preview_error)
}

fn preview_error(kind: PreviewErrorKind) -> ImportPreviewError {
    ImportPreviewError {
        kind,
        message: kind.message(),
    }
}
