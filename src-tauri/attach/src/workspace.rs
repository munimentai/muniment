use std::{fs, io, path::Path};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceOnboardRequest {
    pub opened_directory: String,
    pub memory_location: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceOnboarded {
    pub opened_directory: String,
    pub memory_location: String,
    pub instructions: Option<String>,
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
    use std::io::Write;
    static NEXT_TEMPORARY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if path.exists() {
        return validate_scaffold_file(path);
    }
    let sequence = NEXT_TEMPORARY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("muniment-{}-{sequence}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                validate_scaffold_file(path)
            }
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}

fn validate_scaffold_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "memory seed is not a regular file",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("muniment-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
    #[test]
    fn existing_scaffold_file_is_not_overwritten() {
        let p = temp("atomic-scaffold");
        let file = p.join("README.md");
        fs::write(&file, "mine").unwrap();
        write_scaffold_file_if_missing(&file, b"seed").unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "mine");
        fs::remove_dir_all(p).unwrap();
    }

    #[test]
    fn racing_scaffold_writers_publish_one_complete_file() {
        let p = temp("racing-scaffold");
        let file = p.join("ONBOARDING.md");
        let writers = [
            b"first complete report".as_slice(),
            b"second complete report".as_slice(),
        ]
        .into_iter()
        .map(|contents| {
            let file = file.clone();
            std::thread::spawn(move || write_scaffold_file_if_missing(&file, contents).unwrap())
        })
        .collect::<Vec<_>>();
        for writer in writers {
            writer.join().unwrap();
        }
        let contents = fs::read(file).unwrap();
        assert!(contents == b"first complete report" || contents == b"second complete report");
        fs::remove_dir_all(p).unwrap();
    }
}
