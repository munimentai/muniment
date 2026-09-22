use crate::workspace_tools::{directory, shell};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri_plugin_opener::OpenerExt;

fn entry(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let parent = directory(path.parent().ok_or("Choose an item inside the folder.")?)?;
    if !parent.starts_with(root) {
        return Err("The item is outside this folder.".into());
    }
    let path = parent.join(path.file_name().ok_or("Choose a file name.")?);
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Use the system file browser to change symbolic links.".into());
    }
    Ok(path)
}
fn named(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0', ':'])
    {
        return Err("Enter a file name without path separators.".into());
    }
    let path = parent.join(name);
    if path.try_exists().map_err(|e| e.to_string())? || fs::symlink_metadata(&path).is_ok() {
        return Err("An item with this name already exists.".into());
    }
    Ok(path)
}
fn copy_entry(source: &Path, target: &Path) -> Result<(), String> {
    use std::io;
    let metadata = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(
            "This folder contains a symbolic link. Copy it with the system file browser.".into(),
        );
    }
    if metadata.is_dir() {
        fs::create_dir(target).map_err(|e| e.to_string())?;
        let result = (|| {
            for item in fs::read_dir(source).map_err(|e| e.to_string())? {
                let item = item.map_err(|e| e.to_string())?;
                copy_entry(&item.path(), &target.join(item.file_name()))?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(target);
        }
        result
    } else if metadata.is_file() {
        let mut input = fs::File::open(source).map_err(|e| e.to_string())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        let result = io::copy(&mut input, &mut output)
            .and_then(|_| output.set_permissions(metadata.permissions()))
            .map_err(|e| e.to_string());
        if result.is_err() {
            drop(output);
            let _ = fs::remove_file(target);
        }
        result
    } else {
        Err("Only regular files and folders can be copied.".into())
    }
}
fn unique_copy(parent: &Path, source: &Path) -> PathBuf {
    let name = source.file_name().unwrap().to_string_lossy();
    if !parent.join(name.as_ref()).exists() {
        return parent.join(name.as_ref());
    }
    let (stem, extension) = if source.is_file() {
        (
            source.file_stem().unwrap().to_string_lossy().into_owned(),
            source
                .extension()
                .map(|s| format!(".{}", s.to_string_lossy()))
                .unwrap_or_default(),
        )
    } else {
        (name.into_owned(), String::new())
    };
    for i in 1.. {
        let suffix = if i == 1 {
            " copy".to_string()
        } else {
            format!(" copy {i}")
        };
        let candidate = parent.join(format!("{stem}{suffix}{extension}"));
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    unreachable!()
}
fn act(
    root: &Path,
    action: &str,
    paths: Vec<PathBuf>,
    destination: Option<PathBuf>,
    name: Option<String>,
) -> Result<Vec<PathBuf>, String> {
    let root = directory(root)?;
    let mut paths: Vec<_> = paths
        .iter()
        .map(|p| {
            if action == "paste" {
                entry(&directory(p.parent().ok_or("Choose a source file.")?)?, p)
            } else {
                entry(&root, p)
            }
        })
        .collect::<Result<_, _>>()?;
    paths.sort();
    paths.dedup();
    let all = paths.clone();
    paths.retain(|p| !all.iter().any(|other| other != p && p.starts_with(other)));
    if matches!(action, "new-file" | "new-folder" | "paste") {
        let target = directory(&destination.ok_or("Choose a destination folder.")?)?;
        if !target.starts_with(&root) {
            return Err("The destination is outside this folder.".into());
        }
        if action != "paste" {
            let path = named(&target, &name.ok_or("Enter a name.")?)?;
            if action == "new-folder" {
                fs::create_dir(&path).map_err(|e| e.to_string())?;
            } else {
                fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&path)
                    .map_err(|e| e.to_string())?;
            }
            return Ok(vec![path]);
        }
        if paths.iter().any(|p| p.is_dir() && target.starts_with(p)) {
            return Err("A folder cannot be copied into itself.".into());
        }
        let mut copied = Vec::new();
        for source in paths {
            let path = unique_copy(&target, &source);
            copy_entry(&source, &path)?;
            copied.push(path);
        }
        return Ok(copied);
    }
    match action {
        "rename" if paths.len() == 1 => {
            let path = named(paths[0].parent().unwrap(), &name.ok_or("Enter a name.")?)?;
            fs::rename(&paths[0], &path).map_err(|e| e.to_string())?;
            Ok(vec![path])
        }
        "duplicate" => {
            let mut copied = Vec::new();
            for source in paths {
                let path = unique_copy(source.parent().unwrap(), &source);
                copy_entry(&source, &path)?;
                copied.push(path);
            }
            Ok(copied)
        }
        "trash" => {
            trash::delete_all(&paths)
                .map_err(|e| format!("The items could not be moved to Trash: {e}"))?;
            Ok(vec![])
        }
        _ => Err("This file action is unavailable.".into()),
    }
}
#[tauri::command]
pub async fn workspace_file_action(
    webview: tauri::Webview,
    root: PathBuf,
    action: String,
    paths: Vec<PathBuf>,
    destination: Option<PathBuf>,
    name: Option<String>,
) -> Result<Vec<PathBuf>, String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || act(&root, &action, paths, destination, name))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn workspace_reveal(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    path: PathBuf,
) -> Result<(), String> {
    shell(&webview)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_actions_reject_collisions_and_preserve_copy_contents() {
        let root = std::env::temp_dir().join(format!("muniment-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let file = act(
            &root,
            "new-file",
            vec![],
            Some(root.clone()),
            Some("test.txt".into()),
        )
        .unwrap()
        .remove(0);
        fs::write(&file, "keep").unwrap();
        assert!(act(
            &root,
            "new-file",
            vec![],
            Some(root.clone()),
            Some("test.txt".into())
        )
        .is_err());
        assert!(act(
            &root,
            "rename",
            vec![file.clone()],
            None,
            Some("../escape".into())
        )
        .is_err());
        let copy = act(&root, "duplicate", vec![file.clone()], None, None)
            .unwrap()
            .remove(0);
        assert_eq!(fs::read_to_string(&copy).unwrap(), "keep");
        assert!(act(
            &root,
            "rename",
            vec![file],
            None,
            Some("test copy.txt".into())
        )
        .is_err());
        let folder = act(
            &root,
            "new-folder",
            vec![],
            Some(root.clone()),
            Some("nested".into()),
        )
        .unwrap()
        .remove(0);
        assert!(act(&root, "paste", vec![folder.clone()], Some(folder), None).is_err());
        assert!(act(&root, "duplicate", vec![root.clone()], None, None).is_err());
        let other =
            std::env::temp_dir().join(format!("muniment-copy-source-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&other).unwrap();
        let source = other.join("outside.txt");
        fs::write(&source, "outside").unwrap();
        assert!(act(
            &root,
            "rename",
            vec![source.clone()],
            None,
            Some("renamed.txt".into())
        )
        .is_err());
        let pasted = act(&root, "paste", vec![source], Some(root.clone()), None).unwrap();
        assert_eq!(fs::read_to_string(&pasted[0]).unwrap(), "outside");
        fs::remove_dir_all(other).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
