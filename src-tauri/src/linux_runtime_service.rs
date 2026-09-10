use std::time::Duration;

use muniment_core::attach::linux::AttachFilesystem;
use tauri::Manager;

mod activation;

#[cfg(test)]
mod tests;

pub(crate) fn stop_runtime<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), ()> {
    use std::process::{Command, Stdio};
    use std::time::Instant;

    if !Command::new("/usr/bin/systemctl")
        .args(["--user", "stop", "muniment-runtime.service"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| ())?
        .success()
    {
        return Err(());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app
        .state::<crate::attach_service::AttachCompanionState>()
        .runtime_disconnected()
    {
        if Instant::now() >= deadline {
            return Err(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
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
                let child = activation::spawn_runtime(&executable)?;
                app.state::<crate::runtime_owner::RuntimeOwner>()
                    .keep_child(child);
                Ok(())
            },
            || crate::attach_service::runtime_clients_ready(app),
            Duration::from_secs(10),
        )
    })()
}
