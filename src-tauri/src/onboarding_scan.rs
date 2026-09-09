use muniment_core::harness_scan::{self, Environment, Platform, ScanReport};

/// Scans the current user's assistant memory without writes or memory body reads.
#[tauri::command]
pub async fn onboarding_scan() -> Result<ScanReport, String> {
    let home = std::env::home_dir().ok_or("Cannot locate the home directory.")?;
    let environment: Environment = std::env::vars_os().collect();
    tauri::async_runtime::spawn_blocking(move || {
        harness_scan::scan(&home, &environment, Platform::current())
    })
    .await
    .map_err(|_| "The assistant scan failed.".to_owned())
}
