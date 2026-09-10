use std::time::Duration;

use muniment_core::attach::linux::AttachFilesystem;
use tauri::Manager;

mod activation;

#[cfg(test)]
mod tests;

pub(crate) fn start_runtime<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    crate::attach_service::start_runtime_clients(app);
    let result = (|| {
        let filesystem = AttachFilesystem::from_environment().map_err(|error| error.to_string())?;
        let executable = app
            .path()
            .resource_dir()
            .map_err(|error| error.to_string())?
            .join("muniment-runtime");
        activation::activate_runtime(
            &filesystem,
            || {
                let mut child = activation::spawn_runtime(&executable)?;
                // The runtime outlives the window. Reap it if it exits while the desktop stays open.
                std::thread::spawn(move || match child.wait() {
                    Ok(status) if status.success() => {}
                    Ok(status) => eprintln!("muniment-desktop: runtime exited: {status}"),
                    Err(error) => eprintln!("muniment-desktop: runtime wait failed: {error}"),
                });
                Ok(())
            },
            || crate::attach_service::runtime_clients_ready(app),
            Duration::from_secs(10),
        )
    })();
    if let Err(error) = result {
        eprintln!("muniment-desktop: runtime start failed: {error}");
    }
}
