//! Human-readable workspace names with stable full-ID lookup.
use fs2::FileExt;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub fn basename(name: &str, id: &str) -> String {
    let mut clean = String::new();
    for c in name.chars() {
        let c = if c.is_alphanumeric() || c == '-' {
            c
        } else {
            '_'
        };
        if c == '_' && (clean.is_empty() || clean.ends_with('_')) {
            continue;
        }
        if clean.len() + c.len_utf8() > 72 {
            break;
        }
        clean.push(c);
    }
    let clean = clean.trim_matches('_');
    let clean = if clean.is_empty() { "Untitled" } else { clean };
    let short: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .rev()
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{clean}-{short}")
}

/// The folder name the index records for `id` under `root`, read without a
/// lock or a write. An index that names a path, not one folder, is refused.
pub fn recorded(profile: &Path, root: &Path, id: &str) -> Result<Option<String>, String> {
    let names: BTreeMap<String, String> = match fs::read(profile.join("workspace-names.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let Some(name) = names.get(&format!("{}:{id}", root.display())) else {
        return Ok(None);
    };
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(Some(name.clone())),
        _ => Err("Invalid workspace folder name.".into()),
    }
}

/// Every folder name the index records under `root`, read without a lock.
pub fn recorded_names(
    profile: &Path,
    root: &Path,
) -> Result<std::collections::BTreeSet<String>, String> {
    let names: BTreeMap<String, String> = match fs::read(profile.join("workspace-names.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(e.to_string()),
    };
    let prefix = format!("{}:", root.display());
    Ok(names
        .into_iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, leaf)| leaf)
        .collect())
}

/// An I/O error as text: plain words for a full disk, else the error itself.
fn io_text(error: std::io::Error) -> String {
    crate::disk_full::or_message(&error, &error.to_string())
}

pub fn resolve(
    profile: &Path,
    root: &Path,
    id: &str,
    name: Option<&str>,
    legacy: Option<&Path>,
) -> Result<PathBuf, String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Invalid workspace identifier.".into());
    }
    if root.is_symlink() {
        return Err("The workspace root must not be a symbolic link.".into());
    }
    crate::state_root::guard_owner_data(root, "a workspace folder");
    crate::state_root::guard_owner_data(profile, "the workspace name index");
    fs::create_dir_all(profile).map_err(io_text)?;
    fs::create_dir_all(root).map_err(io_text)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(profile.join("workspace-names.lock"))
        .map_err(io_text)?;
    lock.lock_exclusive().map_err(io_text)?;
    let index = profile.join("workspace-names.json");
    let mut names: BTreeMap<String, String> = match fs::read(&index) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(e.to_string()),
    };
    let key = format!("{}:{id}", root.display());
    if names.get(&key).is_some_and(|n| {
        Path::new(n).components().count() != 1
            || !matches!(
                Path::new(n).components().next(),
                Some(std::path::Component::Normal(_))
            )
    }) {
        return Err("Invalid workspace folder name.".into());
    }
    let previous = names.get(&key).map(|n| root.join(n));
    let source = previous
        .clone()
        .or_else(|| legacy.map(Path::to_path_buf))
        .unwrap_or_else(|| root.join(id));
    if previous.is_some() && !source.exists() {
        return Err("The workspace folder is unavailable.".into());
    }
    let desired = name
        .map(|n| basename(n, id))
        .or_else(|| names.get(&key).cloned())
        .unwrap_or_else(|| basename("Untitled", id));
    let mut target = root.join(&desired);
    let same_source = |path: &Path| {
        source.exists() && path.exists() && source.canonicalize().ok() == path.canonicalize().ok()
    };
    let collision = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .any(|entry| {
            entry.file_name().to_string_lossy().to_lowercase() == desired.to_lowercase()
                && !same_source(&entry.path())
        });
    if target != source && collision {
        target = root.join(format!("{desired}-{id}"));
    }
    if source.is_symlink() || target.is_symlink() {
        return Err("The workspace folder must not be a symbolic link.".into());
    }
    if target != source && source.exists() {
        if target.exists() && !same_source(&target) {
            return Err("A workspace folder with that identifier already exists.".into());
        }
        fs::rename(&source, &target).map_err(io_text)?;
    }
    fs::create_dir_all(&target).map_err(io_text)?;
    let leaf = target.file_name().unwrap().to_string_lossy().into_owned();
    if names.get(&key) != Some(&leaf) {
        names.insert(key, leaf);
        let temp = index.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        fs::write(
            &temp,
            serde_json::to_vec(&names).map_err(|e| e.to_string())?,
        )
        .map_err(io_text)?;
        crate::atomic_file::replace(&temp, &index).map_err(io_text)?;
    }
    Ok(target)
}

/// Removes each empty `Untitled-*` folder under `root` that the name index
/// names for no thread. Such a folder holds nothing and belongs to no thread.
/// A folder that holds anything, a file and a link stay. Returns the count.
pub fn remove_unused_untitled(profile: &Path, root: &Path) -> Result<usize, String> {
    if !root.is_dir() || root.is_symlink() {
        return Ok(0);
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(profile.join("workspace-names.lock"))
        .map_err(io_text)?;
    lock.lock_exclusive().map_err(io_text)?;
    let names: BTreeMap<String, String> = match fs::read(profile.join("workspace-names.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(e.to_string()),
    };
    let prefix = format!("{}:", root.display());
    let named = names
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, leaf)| leaf.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let mut removed = 0;
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let leaf = entry.file_name().to_string_lossy().into_owned();
        if !leaf.starts_with("Untitled-")
            || named.contains(&leaf.to_lowercase())
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let path = entry.path();
        let empty = fs::read_dir(&path).is_ok_and(|mut entries| entries.next().is_none());
        // `remove_dir` refuses a folder that is not empty.
        if empty && fs::remove_dir(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Holds the name index's lock while it lives.
fn lock(profile: &Path) -> Result<fs::File, String> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(profile.join("workspace-names.lock"))
        .map_err(io_text)?;
    lock.lock_exclusive().map_err(io_text)?;
    Ok(lock)
}

fn read_index(profile: &Path) -> Result<BTreeMap<String, String>, String> {
    match fs::read(profile.join("workspace-names.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_index(profile: &Path, names: &BTreeMap<String, String>) -> Result<(), String> {
    crate::state_root::guard_owner_data(profile, "the workspace name index");
    let index = profile.join("workspace-names.json");
    let temp = index.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = fs::write(&temp, serde_json::to_vec(names).map_err(|e| e.to_string())?)
        .map_err(io_text)
        .and_then(|()| crate::atomic_file::replace(&temp, &index).map_err(io_text));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Whether `path` is gone: a read that answers anything but not found, such
/// as a refused one, leaves it in place.
fn gone(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

/// Offers the folder the index records for `id` under `root` to `remove`,
/// under the index's lock, and forgets the record once the folder is gone.
/// `remove` answers the folder when it stays. Answers the folder that stays.
pub fn remove_recorded(
    profile: &Path,
    root: &Path,
    id: &str,
    remove: impl FnOnce(&Path) -> Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let _lock = lock(profile)?;
    let mut names = read_index(profile)?;
    let key = format!("{}:{id}", root.display());
    let Some(leaf) = names.get(&key).cloned() else {
        return Ok(None);
    };
    let mut components = Path::new(&leaf).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    ) {
        return Err("Invalid workspace folder name.".into());
    }
    let folder = root.join(&leaf);
    let kept = if gone(&folder) { None } else { remove(&folder) };
    if kept.is_none() && gone(&folder) {
        names.remove(&key);
        write_index(profile, &names)?;
    }
    Ok(kept)
}

/// The thread or agent ids the index records under `root`.
pub fn recorded_ids(profile: &Path, root: &Path) -> Result<Vec<String>, String> {
    let prefix = format!("{}:", root.display());
    Ok(read_index(profile)?
        .into_keys()
        .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
        .filter(|id| !id.contains(':'))
        .collect())
}

/// Forgets each record whose folder is gone from a folder that still reads,
/// under the index's lock. It never removes a folder, and a root that does
/// not read, such as an unmounted Home, keeps its records. Answers the count.
pub fn forget_missing(profile: &Path) -> Result<usize, String> {
    if !profile.join("workspace-names.json").exists() {
        return Ok(0);
    }
    let _lock = lock(profile)?;
    let mut names = read_index(profile)?;
    let before = names.len();
    names.retain(|key, leaf| {
        let Some((root, _)) = key.rsplit_once(':') else {
            return true;
        };
        let root = Path::new(root);
        let readable = fs::read_dir(root).is_ok();
        let single = matches!(
            Path::new(leaf.as_str())
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [std::path::Component::Normal(_)]
        );
        !(readable && single && gone(&root.join(leaf.as_str())))
    });
    let forgotten = before - names.len();
    if forgotten > 0 {
        write_index(profile, &names)?;
    }
    Ok(forgotten)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_are_portable_and_bounded() {
        for name in [
            "CON",
            "NUL.txt",
            "COM1",
            "LPT9",
            "Plan Builder",
            "a/b\\c:*?\"<>|",
            "...",
            "name. ",
            "a\n b",
        ] {
            let leaf = basename(name, "12345678");
            assert!(!leaf
                .chars()
                .any(|c| c.is_whitespace() || "/\\<>:\"|?*.".contains(c)));
            assert!(leaf.ends_with("-12345678"));
        }
        assert!(basename(&"界".repeat(100), "12345678").len() <= 81);
        assert_eq!(
            basename("Plan Builder", "12345678"),
            "Plan_Builder-12345678"
        );
    }
    #[test]
    fn names_migrate_and_rename_without_merging_colliding_ids() {
        let dir = std::env::temp_dir().join(format!("workspace-names-{}", uuid::Uuid::new_v4()));
        let root = dir.join("sessions");
        fs::create_dir_all(root.join("first-12345678")).unwrap();
        fs::write(root.join("first-12345678/output.txt"), "kept").unwrap();
        let a = resolve(&dir, &root, "first-12345678", Some("Sky color"), None).unwrap();
        assert_eq!(a.file_name().unwrap(), "Sky_color-12345678");
        assert_eq!(fs::read_to_string(a.join("output.txt")).unwrap(), "kept");
        let b = resolve(&dir, &root, "second-12345678", Some("Sky color"), None).unwrap();
        assert_ne!(a, b);
        let renamed = resolve(&dir, &root, "first-12345678", Some("New / title"), None).unwrap();
        assert!(renamed.join("output.txt").exists());
        assert!(!a.exists());
        assert_eq!(
            resolve(&dir, &root, "first-12345678", None, None).unwrap(),
            renamed
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn only_empty_untitled_folders_no_thread_names_are_removed() {
        let dir = std::env::temp_dir().join(format!("workspace-names-{}", uuid::Uuid::new_v4()));
        let root = dir.join("sessions");
        let named = resolve(&dir, &root, "thread-aaaa1111", None, None).unwrap();
        assert!(named.ends_with("Untitled-aaaa1111"));
        for leaf in ["Untitled-orphan01", "Untitled-files001", "Plan-orphan02"] {
            fs::create_dir_all(root.join(leaf)).unwrap();
        }
        fs::write(root.join("Untitled-files001/notes.txt"), "kept").unwrap();
        fs::create_dir_all(root.join("Untitled-nested01/.muniment")).unwrap();
        fs::write(root.join("Untitled-file0001"), "a file, not a folder").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("elsewhere"), root.join("Untitled-link0001")).unwrap();
        assert_eq!(remove_unused_untitled(&dir, &root).unwrap(), 1);
        assert!(!root.join("Untitled-orphan01").exists());
        assert!(named.is_dir());
        assert!(root.join("Untitled-files001/notes.txt").exists());
        assert!(root.join("Untitled-nested01/.muniment").is_dir());
        assert!(root.join("Plan-orphan02").is_dir());
        assert!(root.join("Untitled-file0001").is_file());
        #[cfg(unix)]
        assert!(root.join("Untitled-link0001").is_symlink());
        // A missing root has nothing to remove.
        assert_eq!(
            remove_unused_untitled(&dir, &dir.join("missing")).unwrap(),
            0
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_index_forgets_only_records_whose_folder_is_gone() {
        let dir = std::env::temp_dir().join(format!("workspace-names-{}", uuid::Uuid::new_v4()));
        let sessions = dir.join("sessions");
        let agents = dir.join("agents");
        let live = resolve(&dir, &sessions, "live-1", Some("Live"), None).unwrap();
        let gone_folder = resolve(&dir, &sessions, "gone-1", Some("Gone"), None).unwrap();
        let agent = resolve(&dir, &agents, "agent-1", Some("Agent"), None).unwrap();
        // A record under a Home that does not read stays.
        let mut names = read_index(&dir).unwrap();
        names.insert(
            format!("{}:away-1", dir.join("unmounted").display()),
            "Away-away1".into(),
        );
        write_index(&dir, &names).unwrap();
        fs::remove_dir(&gone_folder).unwrap();
        fs::remove_dir(&agent).unwrap();
        assert_eq!(forget_missing(&dir).unwrap(), 2);
        assert!(live.is_dir());
        assert_eq!(recorded_ids(&dir, &sessions).unwrap(), ["live-1"]);
        assert!(recorded_ids(&dir, &agents).unwrap().is_empty());
        assert_eq!(
            recorded_ids(&dir, &dir.join("unmounted")).unwrap(),
            ["away-1"]
        );
        assert_eq!(forget_missing(&dir).unwrap(), 0);

        // A removed folder leaves the index; a kept one keeps its record.
        let kept = remove_recorded(&dir, &sessions, "live-1", |folder| {
            Some(folder.to_path_buf())
        })
        .unwrap();
        assert_eq!(kept.as_deref(), Some(live.as_path()));
        assert_eq!(recorded_ids(&dir, &sessions).unwrap(), ["live-1"]);
        let kept = remove_recorded(&dir, &sessions, "live-1", |folder| {
            fs::remove_dir_all(folder).unwrap();
            None
        })
        .unwrap();
        assert_eq!(kept, None);
        assert!(!live.exists());
        assert!(recorded_ids(&dir, &sessions).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
