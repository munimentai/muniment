use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Runtime-owned, idempotent workspace onboarding used by companion surfaces.
pub fn onboard_workspace(workspace: &Path) -> io::Result<Option<PathBuf>> {
    let workspace = workspace.canonicalize()?;
    if !workspace.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace is not a directory",
        ));
    }
    let memory = workspace.join("memory");
    ensure_scaffold_directory(&memory)?;
    write_scaffold_file_if_missing(
        &memory.join("README.md"),
        b"# Memory\n\nDurable context for this workspace.\n",
    )?;
    write_scaffold_file_if_missing(&memory.join("ONBOARDING.md"), b"# Muniment workspace onboarding report\n\nThis opened directory is the workspace memory location. User-level Home remains lazy until cross-project context is needed. Repository instructions are resolved from the nearest `AGENTS.md`.\n")?;
    Ok(nearest_agents_file(&workspace))
}

pub fn nearest_agents_file(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        let candidate = current.join("AGENTS.md");
        if candidate.is_file() {
            return Some(candidate);
        }
        if current.join(".git").exists() {
            return None;
        }
        current = current.parent()?;
    }
}

pub fn ensure_scaffold_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "memory path is not a directory",
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(path),
        Err(error) => Err(error),
    }
}

pub fn write_scaffold_file_if_missing(path: &Path, contents: &[u8]) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "memory seed is not a regular file",
            ))
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("muniment-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn onboard_is_idempotent_and_does_not_create_home() {
        let p = temp("workspace");
        onboard_workspace(&p).unwrap();
        fs::write(p.join("memory/README.md"), "mine").unwrap();
        onboard_workspace(&p).unwrap();
        assert_eq!(
            fs::read_to_string(p.join("memory/README.md")).unwrap(),
            "mine"
        );
        for child in ["agents", "projects", "sessions"] {
            assert!(!p.join(child).exists());
        }
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn nearest_agents_wins_in_a_monorepo() {
        let p = temp("agents");
        fs::create_dir(p.join(".git")).unwrap();
        fs::create_dir_all(p.join("one/two")).unwrap();
        fs::write(p.join("AGENTS.md"), "root").unwrap();
        fs::write(p.join("one/AGENTS.md"), "near").unwrap();
        assert_eq!(
            nearest_agents_file(&p.join("one/two")),
            Some(p.join("one/AGENTS.md"))
        );
        fs::remove_dir_all(p).unwrap();
    }
}
