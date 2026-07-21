use std::path::PathBuf;

use muniment_core::import_preview::{preview_export_zip, PreviewErrorKind, PreviewManifest};
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
pub fn onboarding_import_preview(
    archive_path: String,
) -> Result<PreviewManifest, ImportPreviewError> {
    preview_export_zip(&PathBuf::from(archive_path)).map_err(|kind| ImportPreviewError {
        kind,
        message: kind.message(),
    })
}
