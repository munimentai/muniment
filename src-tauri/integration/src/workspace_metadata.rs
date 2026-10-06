use fs2::FileExt;
use std::{fs, io, path::Path};

/// Recover a thread before the core validates its metadata directory.
pub fn recover_thread_metadata(profile: &Path, thread: &str) -> Result<(), String> {
    let folder = if let Some(id) = muniment_core::agents::state(profile)?.threads.get(thread) {
        let agent = muniment_core::agents::get(profile, id)?;
        match agent.project_id {
            Some(project) => muniment_core::projects::folder(profile, &project)?,
            None => muniment_core::agents::folder(profile, id)?,
        }
    } else if let Some(folder) = muniment_core::projects::thread_folder(profile, thread)? {
        folder
    } else {
        let home = muniment_core::home::configured_home(profile)
            .map_err(|e| e.to_string())?
            .ok_or("The Home folder is unavailable.")?;
        muniment_core::workspace_names::resolve(
            profile,
            &home.join("sessions"),
            thread,
            None,
            None,
        )?
    };
    recover_metadata(profile, &folder)
        .map_err(|_| "Muniment could not recover the session metadata folder.".into())
}

fn recover_metadata(profile: &Path, folder: &Path) -> io::Result<()> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(profile.join("metadata-recovery.lock"))?;
    lock.lock_exclusive()?;
    let old = folder.join(".pi");
    let new = folder.join(".muniment");
    if folder.is_symlink() || old.is_symlink() || new.is_symlink() {
        return Err(io::Error::other(
            "The metadata folder must not be a symbolic link.",
        ));
    }
    if !old.try_exists()? || !new.try_exists()? {
        return Ok(());
    }
    if !old.is_dir() || !new.is_dir() {
        return Err(io::Error::other("The metadata path must be a folder."));
    }
    // Keep both trees intact. A private, exclusive parent prevents destination collisions.
    // Rename the whole tree so a failed move leaves the original files together.
    let archive = new.join(format!("recovered-{}", uuid::Uuid::new_v4()));
    let mut builder = fs::DirBuilder::new();
    builder.recursive(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&archive)?;
    if let Err(error) = fs::rename(&old, archive.join("metadata")) {
        let _ = fs::remove_dir(&archive);
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("metadata-recovery-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            muniment_core::home::confirm_home(&root, &root.join("home")).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn archive(folder: &Path) -> std::path::PathBuf {
        fs::read_dir(folder.join(".muniment"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("recovered-")
            })
            .unwrap()
            .join("metadata")
    }

    #[test]
    fn recovery_preserves_conflicts_and_serializes_repeated_calls() {
        let fixture = Fixture::new();
        let folder = muniment_core::projects::workspace(&fixture.0, "thread").unwrap();
        for (root, content) in [(".pi", "old"), (".muniment", "new")] {
            fs::create_dir_all(folder.join(root).join("tasks")).unwrap();
            fs::write(folder.join(root).join("tasks/result"), content).unwrap();
        }
        assert!(muniment_core::projects::workspace(&fixture.0, "thread").is_err());
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| recover_thread_metadata(&fixture.0, "thread").unwrap());
            }
        });
        assert_eq!(
            fs::read_to_string(folder.join(".muniment/tasks/result")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(archive(&folder).join("tasks/result")).unwrap(),
            "old"
        );
        assert_eq!(fs::read_dir(folder.join(".muniment")).unwrap().count(), 2);
        assert!(!folder.join(".pi").exists());
        assert_eq!(
            muniment_core::projects::workspace(&fixture.0, "thread").unwrap(),
            folder
        );
    }

    #[test]
    fn recovery_handles_projects_and_leaves_single_roots_to_core() {
        let fixture = Fixture::new();
        let project = muniment_core::projects::create(&fixture.0, "Research").unwrap();
        muniment_core::projects::assign(&fixture.0, "thread", &project).unwrap();
        let folder = muniment_core::projects::workspace(&fixture.0, "thread").unwrap();
        recover_thread_metadata(&fixture.0, "thread").unwrap();
        fs::create_dir(folder.join(".pi")).unwrap();
        fs::write(folder.join(".pi/keep"), "old").unwrap();
        recover_thread_metadata(&fixture.0, "thread").unwrap();
        assert!(folder.join(".pi/keep").exists());
        muniment_core::projects::workspace(&fixture.0, "thread").unwrap();
        fs::create_dir(folder.join(".pi")).unwrap();
        fs::write(folder.join(".pi/keep"), "conflict").unwrap();
        recover_thread_metadata(&fixture.0, "thread").unwrap();
        assert_eq!(
            fs::read_to_string(folder.join(".muniment/keep")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(archive(&folder).join("keep")).unwrap(),
            "conflict"
        );
    }

    #[test]
    fn recovery_rejects_invalid_threads_and_non_directory_metadata() {
        let fixture = Fixture::new();
        for thread in ["", "../outside", "/outside"] {
            assert!(recover_thread_metadata(&fixture.0, thread).is_err());
        }
        let folder = muniment_core::projects::workspace(&fixture.0, "thread").unwrap();
        fs::create_dir(folder.join(".pi")).unwrap();
        fs::write(folder.join(".pi/keep"), "old").unwrap();
        fs::write(folder.join(".muniment"), "not a directory").unwrap();
        assert!(recover_thread_metadata(&fixture.0, "thread").is_err());
        assert_eq!(fs::read_to_string(folder.join(".pi/keep")).unwrap(), "old");
        assert_eq!(
            fs::read_to_string(folder.join(".muniment")).unwrap(),
            "not a directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recovery_rejects_root_symlinks_and_preserves_nested_symlinks() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let folder = muniment_core::projects::workspace(&fixture.0, "thread").unwrap();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), "outside").unwrap();
        fs::create_dir(folder.join(".pi")).unwrap();
        symlink(&outside, folder.join(".muniment")).unwrap();
        assert!(recover_thread_metadata(&fixture.0, "thread").is_err());
        fs::remove_file(folder.join(".muniment")).unwrap();
        fs::create_dir(folder.join(".muniment")).unwrap();
        fs::remove_dir(folder.join(".pi")).unwrap();
        symlink(&outside, folder.join(".pi")).unwrap();
        assert!(recover_thread_metadata(&fixture.0, "thread").is_err());
        fs::remove_file(folder.join(".pi")).unwrap();
        fs::create_dir(folder.join(".pi")).unwrap();
        symlink(&outside, folder.join(".pi/link")).unwrap();
        recover_thread_metadata(&fixture.0, "thread").unwrap();
        assert!(archive(&folder).join("link").is_symlink());
        assert_eq!(fs::read_to_string(outside.join("keep")).unwrap(), "outside");
    }
}
