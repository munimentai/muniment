use chrono::NaiveDate;
use muniment_core::{
    home::{
        choose_default_home, compile_onboarding_home_write_plan, confirm_home,
        persist_onboarding_home_write_plan, scaffold_home, validate_home_selection, HomeError,
        HomeErrorKind, OnboardingHomePersistenceError, OnboardingHomeWritePlanError,
    },
    import_preview::ExtractedEntry,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeStatus {
    configured: bool,
    home_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeImportSuccess {
    configured: bool,
    imported_file_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HomeImportErrorKind {
    InvalidInput,
    DestinationConflict,
    SecretRejected,
    SaveFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeImportError {
    kind: HomeImportErrorKind,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_name: Option<String>,
}

impl HomeImportError {
    fn invalid_input() -> Self {
        Self {
            kind: HomeImportErrorKind::InvalidInput,
            message: "The confirmed Home import input is invalid.",
            relative_path: None,
            source_name: None,
        }
    }

    /// The message names no matched text, so no secret leaves the core.
    fn secret_rejected(source_name: Option<String>) -> Self {
        Self {
            kind: HomeImportErrorKind::SecretRejected,
            message: "An approved Home import file contains a secret.",
            relative_path: None,
            source_name,
        }
    }

    fn save_failed() -> Self {
        Self {
            kind: HomeImportErrorKind::SaveFailed,
            message: "The confirmed Home import could not be saved.",
            relative_path: None,
            source_name: None,
        }
    }
}

fn map_home_error(error: HomeError) -> HomeImportError {
    match error.kind() {
        HomeErrorKind::InvalidInput => HomeImportError::invalid_input(),
        HomeErrorKind::Io => HomeImportError::save_failed(),
    }
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|_| "Muniment configuration storage is unavailable.".to_string())
}

pub(crate) fn default_home<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    choose_default_home(app.path().document_dir().ok(), app.path().home_dir().ok())
}

#[tauri::command]
pub fn home_status(app: AppHandle) -> Result<HomeStatus, String> {
    let config = config_dir(&app)?;
    let selected =
        muniment_core::home::configured_home(&config).map_err(|error| error.to_string())?;
    let configured = selected.is_some();
    let home = match selected {
        Some(home) => home,
        None => default_home(&app)?,
    };
    Ok(HomeStatus {
        configured,
        home_path: home.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn home_confirm(app: AppHandle, home_path: String) -> Result<HomeStatus, String> {
    let config = config_dir(&app)?;
    let home = PathBuf::from(home_path);
    muniment_core::home::confirm_home(&config, &home).map_err(|error| error.to_string())?;
    Ok(HomeStatus {
        configured: true,
        home_path: home.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn home_confirm_import(
    app: AppHandle,
    home_path: String,
    approved_entries: Vec<ExtractedEntry>,
) -> Result<HomeImportSuccess, HomeImportError> {
    let config = config_dir(&app).map_err(|_| HomeImportError::save_failed())?;
    confirm_import(
        &config,
        Path::new(&home_path),
        &approved_entries,
        chrono::Local::now().date_naive(),
    )
}

fn confirm_import(
    config: &Path,
    home: &Path,
    approved_entries: &[ExtractedEntry],
    import_date: NaiveDate,
) -> Result<HomeImportSuccess, HomeImportError> {
    confirm_import_with_hook(config, home, approved_entries, import_date, || {})
}

fn confirm_import_with_hook(
    config: &Path,
    home: &Path,
    approved_entries: &[ExtractedEntry],
    import_date: NaiveDate,
    before_confirm: impl FnOnce(),
) -> Result<HomeImportSuccess, HomeImportError> {
    let plan =
        compile_onboarding_home_write_plan(approved_entries, import_date).map_err(|error| {
            match error {
                OnboardingHomeWritePlanError::SecretRejected { source_name } => {
                    HomeImportError::secret_rejected(Some(source_name))
                }
                _ => HomeImportError::invalid_input(),
            }
        })?;
    let imported_file_count = plan.writes().len();

    validate_home_selection(config, home).map_err(map_home_error)?;
    scaffold_home(home).map_err(map_home_error)?;
    persist_onboarding_home_write_plan(home, &plan).map_err(|error| match error {
        OnboardingHomePersistenceError::DestinationConflict { relative_path } => HomeImportError {
            kind: HomeImportErrorKind::DestinationConflict,
            message: "A Home import destination already exists.",
            relative_path: Some(relative_path),
            source_name: None,
        },
        OnboardingHomePersistenceError::SecretRejected => HomeImportError::secret_rejected(None),
        OnboardingHomePersistenceError::InvalidHome => HomeImportError::invalid_input(),
        _ => HomeImportError::save_failed(),
    })?;
    before_confirm();
    confirm_home(config, home).map_err(map_home_error)?;

    Ok(HomeImportSuccess {
        configured: true,
        imported_file_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_core::{home::configured_home, import_preview::EntryKind};
    use std::fs;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "muniment-home-command-{name}-{}",
                uuid::Uuid::now_v7()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn entry() -> ExtractedEntry {
        ExtractedEntry {
            source_name: "notes.md".into(),
            kind: EntryKind::Markdown,
            text: "Original\r\n---\nbody 🦀\n".into(),
            source_provenance: "claude-export:notes.md".into(),
        }
    }

    #[test]
    fn confirmed_import_publishes_exact_bytes_before_configuring_home() {
        let root = TempRoot::new("success");
        let config = root.0.join("config");
        let home = root.0.join("home");
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let entry = entry();
        let expected = compile_onboarding_home_write_plan(&[entry.clone()], date).unwrap();

        let result = confirm_import(&config, &home, &[entry], date).unwrap();

        assert_eq!(
            result,
            HomeImportSuccess {
                configured: true,
                imported_file_count: expected.writes().len(),
            }
        );
        for write in expected.writes() {
            assert_eq!(
                fs::read(home.join(write.relative_path())).unwrap(),
                write.bytes()
            );
        }
        assert_eq!(configured_home(&config).unwrap(), Some(home));
    }

    #[test]
    fn destination_collision_preserves_user_content_and_unconfigured_state() {
        let root = TempRoot::new("collision");
        let config = root.0.join("config");
        let home = root.0.join("home");
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        let entry = entry();
        let plan = compile_onboarding_home_write_plan(&[entry.clone()], date).unwrap();
        let collision = &plan.writes()[0];
        scaffold_home(&home).unwrap();
        fs::create_dir_all(home.join(collision.relative_path()).parent().unwrap()).unwrap();
        fs::write(home.join(collision.relative_path()), b"user content").unwrap();

        let error = confirm_import(&config, &home, &[entry], date).unwrap_err();

        assert_eq!(
            error,
            HomeImportError {
                kind: HomeImportErrorKind::DestinationConflict,
                message: "A Home import destination already exists.",
                relative_path: Some(collision.relative_path.clone()),
                source_name: None,
            }
        );
        assert_eq!(
            fs::read(home.join(collision.relative_path())).unwrap(),
            b"user content"
        );
        for write in &plan.writes()[1..] {
            assert!(!home.join(write.relative_path()).exists());
        }
        assert_eq!(configured_home(&config).unwrap(), None);
    }

    #[test]
    fn an_approved_entry_holding_a_secret_serializes_as_secret_rejected() {
        let root = TempRoot::new("secret-entry");
        let config = root.0.join("config");
        let home = root.0.join("home");
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();
        // The segments repeat one character each, so no scanner reads this file as a secret.
        let token = format!("{}.{}.{}", "A".repeat(20), "B".repeat(18), "C".repeat(22));
        let carrier = ExtractedEntry {
            text: format!("{token}\n"),
            ..entry()
        };

        let error = confirm_import(&config, &home, &[carrier], date).unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "secretRejected",
                "message": "An approved Home import file contains a secret.",
                "sourceName": "notes.md"
            })
        );
        assert!(!home.exists());
        assert_eq!(configured_home(&config).unwrap(), None);
    }

    #[test]
    fn structural_home_error_serializes_as_invalid_input() {
        let root = TempRoot::new("invalid-home");
        let config = root.0.join("config");
        let home = root.0.join("home");
        fs::write(&home, b"not a directory").unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();

        let error = confirm_import(&config, &home, &[entry()], date).unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "invalidInput",
                "message": "The confirmed Home import input is invalid."
            })
        );
        assert!(!config.exists());
        assert_eq!(fs::read(home).unwrap(), b"not a directory");
    }

    #[test]
    fn home_inspection_io_error_serializes_as_save_failed() {
        let root = TempRoot::new("home-io");
        let non_directory = root.0.join("not-a-directory");
        fs::write(&non_directory, b"user content").unwrap();
        let config = non_directory.join("config");
        let home = root.0.join("home");
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();

        let error = confirm_import(&config, &home, &[entry()], date).unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "saveFailed",
                "message": "The confirmed Home import could not be saved."
            })
        );
        assert!(!home.exists());
        assert_eq!(fs::read(non_directory).unwrap(), b"user content");
    }

    #[test]
    fn final_structural_home_error_serializes_as_invalid_input() {
        let root = TempRoot::new("final-invalid-home");
        let config = root.0.join("config");
        let home = root.0.join("home");
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();

        let error = confirm_import_with_hook(&config, &home, &[entry()], date, || {
            fs::remove_dir_all(home.join("projects")).unwrap();
            fs::write(home.join("projects"), b"not a directory").unwrap();
        })
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "invalidInput",
                "message": "The confirmed Home import input is invalid."
            })
        );
        assert_eq!(configured_home(&config).unwrap(), None);
        assert_eq!(fs::read(home.join("projects")).unwrap(), b"not a directory");
    }

    #[test]
    fn final_configuration_io_error_serializes_as_save_failed() {
        let root = TempRoot::new("final-config-io");
        let config = root.0.join("config");
        let home = root.0.join("home");
        let date = NaiveDate::from_ymd_opt(2026, 7, 24).unwrap();

        let error = confirm_import_with_hook(&config, &home, &[entry()], date, || {
            fs::write(&config, b"not a directory").unwrap()
        })
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "saveFailed",
                "message": "The confirmed Home import could not be saved."
            })
        );
        assert_eq!(fs::read(config).unwrap(), b"not a directory");
    }
}
