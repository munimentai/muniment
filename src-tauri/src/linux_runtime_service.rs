use std::time::Duration;

use muniment_core::attach::linux::AttachFilesystem;
use tauri::Manager;

mod activation;
mod process;

#[cfg(test)]
mod tests;

pub(crate) fn stop_runtime<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), ()> {
    use std::process::{Command, Stdio};
    let filesystem = AttachFilesystem::from_environment().map_err(|_| ())?;
    let owner = app.state::<crate::runtime_owner::RuntimeOwner>();
    stop_owned_runtime(&owner, &filesystem, || {
        Command::new("/usr/bin/systemctl")
            .args(["--user", "stop", "muniment-runtime.service"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| ())?
            .success()
            .then_some(())
            .ok_or(())
    })
}

fn stop_owned_runtime(
    owner: &crate::runtime_owner::RuntimeOwner,
    filesystem: &AttachFilesystem,
    stop_service: impl FnOnce() -> Result<(), ()>,
) -> Result<(), ()> {
    // Serialize stop with starts from other desktop processes.
    let _startup_lock = filesystem.acquire_startup_lock().map_err(|_| ())?;
    if owner.stop_child()? || process::stop_runtime(filesystem).map_err(|_| ())? {
        Ok(())
    } else {
        stop_service()
    }
}

pub(crate) fn start_runtime<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    (|| {
        let filesystem = AttachFilesystem::from_environment().map_err(|error| error.to_string())?;
        let executable = app
            .path()
            .resource_dir()
            .map_err(|error| error.to_string())?
            .join("muniment-runtime");
        activation::activate_runtime(
            &filesystem,
            || {
                let child = process::spawn_runtime(&filesystem, &executable)?;
                app.state::<crate::runtime_owner::RuntimeOwner>()
                    .keep_child(child);
                Ok(())
            },
            || crate::attach_service::runtime_clients_ready(app),
            Duration::from_secs(10),
        )
    })()
}
