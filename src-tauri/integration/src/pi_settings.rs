use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

use fs2::FileExt;
use muniment_core::{pi_launch::PiLaunchError, sidecar::pi_install::PiArtifactDescriptor};

pub fn prepare_pi_settings(
    artifact: PiArtifactDescriptor,
    executable: &Path,
) -> Result<(), PiLaunchError> {
    muniment_core::pi_settings::prepare_pi_settings(artifact, executable)?;
    let agent = muniment_core::state_root::state_directory()
        .map(|state| muniment_core::state_root::agent_directory(&state))
        .ok_or_else(|| {
            PiLaunchError::rejected(
                "agent_directory_resolve",
                "The state directory is unavailable.",
            )
        })?;
    brand_compiled_background_tasks(&agent.join("npm"))
        .map_err(|error| PiLaunchError::rejected("package_branding", error))
}

// Core brands the TypeScript sources. The pinned package loads compiled JavaScript instead.
fn brand_compiled_background_tasks(npm: &Path) -> io::Result<()> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(npm.join(".muniment-install.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
    let package = npm.join("node_modules/pi-background-tasks");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("package.json"))?)?;
    let compiled = manifest["pi"]["extensions"]
        .as_array()
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .as_str()
                    .is_some_and(|entry| entry.starts_with("./dist/"))
            })
        });
    if !compiled {
        return Ok(());
    }
    for (relative, old, new) in [
        ("dist/src/core/registry.js", "'.pi'", "'.muniment'"),
        ("dist/src/extension.js", ".pi/tasks", ".muniment/tasks"),
        (
            "dist/src/core/attested-pi-run.js",
            "parts[0] === '.pi'",
            "['.pi', '.muniment'].includes(parts[0])",
        ),
    ] {
        let path = package.join(relative);
        let source = fs::read_to_string(&path)?;
        if source.contains(old) {
            muniment_core::model_router::config::write_private(
                &path,
                source.replace(old, new).as_bytes(),
            )?;
        } else if !source.contains(new) {
            return Err(io::Error::other(
                "The background task storage path is unavailable.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_paths_are_branded_idempotently_without_changing_user_files() {
        let root =
            std::env::temp_dir().join(format!("compiled-task-paths-{}", uuid::Uuid::new_v4()));
        let package = root.join("node_modules/pi-background-tasks");
        fs::create_dir_all(package.join("dist/src/core")).unwrap();
        fs::write(
            package.join("package.json"),
            r#"{"pi":{"extensions":["./dist/extensions/background-tasks.js"]}}"#,
        )
        .unwrap();
        for (file, source) in [
            (
                "core/registry.js",
                "join(cwd, '.pi', 'tasks'); join('.pi', 'tasks');",
            ),
            ("extension.js", "Output is written to .pi/tasks"),
            (
                "core/attested-pi-run.js",
                "parts[0] === '.pi' && parts[1] === 'tasks'",
            ),
        ] {
            fs::write(package.join("dist/src").join(file), source).unwrap();
        }
        fs::create_dir_all(root.join("workspace/.pi")).unwrap();
        fs::write(root.join("workspace/.pi/keep"), "user data").unwrap();
        for _ in 0..2 {
            brand_compiled_background_tasks(&root).unwrap();
            assert_eq!(
                fs::read_to_string(package.join("dist/src/core/registry.js")).unwrap(),
                "join(cwd, '.muniment', 'tasks'); join('.muniment', 'tasks');"
            );
            assert_eq!(
                fs::read_to_string(package.join("dist/src/extension.js")).unwrap(),
                "Output is written to .muniment/tasks"
            );
            assert_eq!(
                fs::read_to_string(package.join("dist/src/core/attested-pi-run.js")).unwrap(),
                "['.pi', '.muniment'].includes(parts[0]) && parts[1] === 'tasks'"
            );
        }
        assert_eq!(
            fs::read_to_string(root.join("workspace/.pi/keep")).unwrap(),
            "user data"
        );
        fs::write(
            package.join("dist/src/core/registry.js"),
            "unexpected layout",
        )
        .unwrap();
        assert!(brand_compiled_background_tasks(&root).is_err());
        fs::remove_file(package.join("dist/src/core/registry.js")).unwrap();
        assert!(brand_compiled_background_tasks(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
