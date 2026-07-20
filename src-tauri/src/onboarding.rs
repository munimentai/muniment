use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager, State};

use crate::model_install::GemmaInstallState;

const REPORT_FILE: &str = "pending-onboarding.md";
const HOME_FILE: &str = "muniment-home";
const TRIAGE_PENDING_FILE: &str = "onboarding-triage-pending";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    complete: bool,
    home_path: String,
    warning: Option<String>,
    triage_pending: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingProposal {
    report: String,
    used_model: bool,
    warning: Option<String>,
}

fn state_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|_| "Onboarding storage is unavailable.".into())
}

fn default_home(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .document_dir()
        .map(|path| path.join("Muniment"))
        .map_err(|_| "The Documents folder is unavailable.".into())
}

fn selected_home(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let file = state_dir(app)?.join(HOME_FILE);
    match fs::read_to_string(file) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(PathBuf::from(value.trim()))),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("The saved Home location could not be read.".into()),
    }
}

fn warning(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let unusual = text.starts_with("//")
        || text.starts_with("\\\\")
        || ["/Volumes/", "/media/", "/mnt/", "/run/media/"]
            .iter()
            .any(|prefix| text.starts_with(prefix));
    unusual.then(|| "This looks like removable or network storage. Muniment can use it, but Home may be unavailable when the device or connection is offline.".into())
}

#[tauri::command]
pub fn onboarding_status(app: AppHandle) -> Result<OnboardingStatus, String> {
    let selected = selected_home(&app)?;
    let path = selected.clone().unwrap_or(default_home(&app)?);
    Ok(OnboardingStatus {
        complete: selected.is_some(),
        home_path: path.to_string_lossy().into_owned(),
        warning: warning(&path),
        triage_pending: selected.is_some() && state_dir(&app)?.join(TRIAGE_PENDING_FILE).exists(),
    })
}

#[tauri::command]
pub async fn onboarding_propose(
    app: AppHandle,
    model: State<'_, GemmaInstallState>,
    home_path: String,
) -> Result<OnboardingProposal, String> {
    let home = validate_home(&home_path)?;
    let triage = if model.acquisition_status().await.ai_features_available {
        model.onboarding_triage(&home)
    } else {
        None
    };
    let used_model = triage.is_some();
    let mode = if used_model {
        "On-device triage"
    } else {
        "Manual setup (triage will run when the on-device model is ready)"
    };
    let proposal = triage.unwrap_or_else(|| "## User type\n\nGeneral knowledge worker\n\n## Proposed Home layout\n\n- `memory/` — durable personal context\n- `agents/` — reusable agent instructions\n- `projects/` — project context\n- `sessions/` — readable session transcripts\n\n## Starter agents\n\n- Researcher — gathers and checks sources\n- Writer — turns context into clear drafts".into());
    let report = format!(
        "# Muniment onboarding report\n\n{proposal}\n\n## Setup mode\n\n{mode}\n\n## Home location\n\n`{}`\n",
        home.to_string_lossy().replace('`', "'")
    );
    let directory = state_dir(&app)?;
    fs::create_dir_all(&directory).map_err(|_| "The onboarding report could not be saved.")?;
    atomic_write(&directory.join(REPORT_FILE), report.as_bytes())
        .map_err(|_| "The onboarding report could not be saved.")?;
    Ok(OnboardingProposal {
        report,
        used_model,
        warning: warning(&home),
    })
}

#[tauri::command]
pub fn onboarding_confirm(app: AppHandle, home_path: String) -> Result<OnboardingStatus, String> {
    let home = validate_home(&home_path)?;
    let directory = state_dir(&app)?;
    let report = fs::read_to_string(directory.join(REPORT_FILE))
        .map_err(|_| "Create and review an onboarding report before setting up Home.")?;
    let expected = format!("## Home location\n\n`{}`\n", home.to_string_lossy());
    if !report.ends_with(&expected) {
        return Err("The Home location changed. Review a new report before continuing.".into());
    }

    fs::create_dir_all(&home).map_err(|_| "The selected Home folder could not be created.")?;
    for (name, description) in [
        ("memory", "Durable personal context and preferences."),
        ("agents", "Reusable agent instructions."),
        ("projects", "Context grouped by project."),
        ("sessions", "Readable session transcripts."),
    ] {
        let child = home.join(name);
        ensure_directory(&child)?;
        write_if_missing(
            &child.join("README.md"),
            format!("# {}\n\n{}\n", title(name), description).as_bytes(),
        )?;
    }
    let report_name = if report.contains("## Setup mode\n\nOn-device triage") {
        "ONBOARDING-TRIAGE.md"
    } else {
        "ONBOARDING.md"
    };
    write_if_missing(&home.join(report_name), report.as_bytes())?;
    write_if_missing(
        &home.join("agents/researcher.md"),
        b"# Researcher\n\nGather relevant sources, check claims, and preserve citations.\n",
    )?;
    write_if_missing(&home.join("agents/writer.md"), b"# Writer\n\nTurn available context into a clear draft while preserving the user's voice.\n")?;
    fs::create_dir_all(&directory).map_err(|_| "The Home selection could not be saved.")?;
    atomic_write(
        &directory.join(HOME_FILE),
        home.to_string_lossy().as_bytes(),
    )
    .map_err(|_| "The Home selection could not be saved.")?;
    let pending = directory.join(TRIAGE_PENDING_FILE);
    let triage_pending =
        report.contains("Manual setup (triage will run when the on-device model is ready)");
    if triage_pending {
        atomic_write(&pending, b"pending").map_err(|_| "The setup state could not be saved.")?;
    } else if pending.exists() {
        fs::remove_file(pending).map_err(|_| "The setup state could not be saved.")?;
    }
    let _ = fs::remove_file(directory.join(REPORT_FILE));
    Ok(OnboardingStatus {
        complete: true,
        home_path: home.to_string_lossy().into_owned(),
        warning: warning(&home),
        triage_pending,
    })
}

fn validate_home(value: &str) -> Result<PathBuf, String> {
    if value.contains(['\n', '\r', '`']) {
        return Err("The Home folder location is invalid.".into());
    }
    let path = PathBuf::from(value.trim());
    if value.trim().is_empty() || !path.is_absolute() {
        return Err("Choose an absolute Home folder location.".into());
    }
    if path.parent().is_none() {
        return Err("A filesystem root cannot be used as Muniment Home.".into());
    }
    Ok(path)
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err("A Home scaffold path is not a directory.".into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| "Muniment Home could not be scaffolded.".into())
        }
        Err(_) => Err("Muniment Home could not be scaffolded.".into()),
    }
}

fn title(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn write_if_missing(path: &Path, contents: &[u8]) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err("A Home seed path is not a regular file.".into());
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err("Muniment Home could not be scaffolded.".into()),
    }
    fs::write(path, contents).map_err(|_| "Muniment Home could not be scaffolded.".into())
}

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::{validate_home, warning};
    use std::path::Path;

    #[test]
    fn home_must_be_an_absolute_non_root_path() {
        let valid = std::env::current_dir().unwrap().join("Muniment");
        assert!(validate_home("").is_err());
        assert!(validate_home("relative/Muniment").is_err());
        assert!(validate_home(valid.to_str().unwrap()).is_ok());
        assert!(validate_home(&format!("{}\nbad", valid.to_string_lossy())).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unusual_mounts_warn_without_rejecting_the_path() {
        let path = Path::new("/mnt/removable/Muniment");
        assert!(warning(path).is_some());
        assert!(validate_home(path.to_str().unwrap()).is_ok());
        assert!(warning(Path::new("/home/user/Documents/Muniment")).is_none());
    }
}
