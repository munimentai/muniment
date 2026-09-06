use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::sidecar::pi_install::{PiArtifactDescriptor, PI_CANDIDATE_ARTIFACT};

pub fn pi_agent_directory(home: &Path, agent_directory: Option<&OsStr>) -> PathBuf {
    match agent_directory.filter(|value| !value.is_empty()) {
        Some(value) if value == "~" => home.to_owned(),
        Some(value) => match value.to_str() {
            Some(value) if value.starts_with("~/") => home.join(&value[2..]),
            _ => PathBuf::from(value),
        },
        None => home.join(".pi").join("agent"),
    }
}

pub fn prepare_pi_settings(artifact: PiArtifactDescriptor) -> io::Result<()> {
    if artifact.version != PI_CANDIDATE_ARTIFACT.version {
        return Ok(());
    }
    let home = std::env::home_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Cannot locate the Pi home directory.",
        )
    })?;
    let agent_directory = std::env::var_os("PI_CODING_AGENT_DIR");
    let directory = pi_agent_directory(&home, agent_directory.as_deref());
    store_pi_settings(&directory.join("settings.json"), artifact)
}

struct SettingsLock(PathBuf);

impl Drop for SettingsLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn lock_settings(path: &Path) -> io::Result<SettingsLock> {
    // Pi uses proper-lockfile with realpath: false. Local provider writes use the same directory lock.
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    let path = PathBuf::from(name);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match fs::create_dir(&path) {
            Ok(()) => return Ok(SettingsLock(path)),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn store_pi_settings(path: &Path, artifact: PiArtifactDescriptor) -> io::Result<()> {
    if artifact.version != PI_CANDIDATE_ARTIFACT.version {
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let _lock = lock_settings(path)?;
    let mut settings: Map<String, Value> = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error),
    };
    let original = settings.clone();
    merge_pi_settings(&mut settings, artifact);
    if settings == original {
        return Ok(());
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(&settings)?)?;
        file.sync_all()?;
        crate::atomic_file::replace(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// The candidate owns packages and defaultTools and preserves all other settings.
pub fn merge_pi_settings(settings: &mut Map<String, Value>, artifact: PiArtifactDescriptor) {
    if artifact.version != PI_CANDIDATE_ARTIFACT.version {
        return;
    }
    settings.insert(
        "packages".into(),
        json!([
            "npm:pi-web-access@0.28.0",
            "npm:pi-subagents@0.65.1",
            "npm:pi-background-tasks@2.5.0",
            "npm:pi-mcp-adapter@2.32.1"
        ]),
    );
    // v0.85.1: packages/coding-agent/src/core/tools/index.ts:96-105, allToolNames.
    settings.insert(
        "defaultTools".into(),
        json!([
            "read",
            "bash",
            "powershell",
            "edit",
            "write",
            "grep",
            "find",
            "ls"
        ]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::pi_install::PI_ARTIFACT;

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("muniment-pi-settings-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn persists_candidate_settings_and_keeps_provider_and_foreign_keys() {
        let root = temporary_directory();
        let path = root.join("agent/settings.json");
        store_pi_settings(&path, PI_ARTIFACT).unwrap();
        assert!(!path.exists());
        store_pi_settings(&path, PI_CANDIDATE_ARTIFACT).unwrap();
        let rendered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(rendered["packages"].as_array().unwrap().len(), 4);
        assert_eq!(rendered["defaultTools"].as_array().unwrap().len(), 8);
        fs::write(
            &path,
            br#"{"defaultProvider":"ollama","foreign":{"nested":42}}"#,
        )
        .unwrap();
        store_pi_settings(&path, PI_CANDIDATE_ARTIFACT).unwrap();
        let bytes = fs::read(&path).unwrap();
        let rendered: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(rendered["defaultProvider"], "ollama");
        assert_eq!(rendered["foreign"], json!({"nested": 42}));
        store_pi_settings(&path, PI_CANDIDATE_ARTIFACT).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        store_pi_settings(&path, PI_ARTIFACT).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_settings_without_overwriting_or_leaving_a_lock() {
        let root = temporary_directory();
        let path = root.join("settings.json");
        for invalid in ["", "null", "[]", "42", "{", "{\"foreign\":true,}"] {
            fs::write(&path, invalid).unwrap();
            assert!(store_pi_settings(&path, PI_CANDIDATE_ARTIFACT).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
            assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        }
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(store_pi_settings(&path, PI_CANDIDATE_ARTIFACT).is_err());
        assert!(path.is_dir());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_pis_bom_prefixed_settings() {
        let root = temporary_directory();
        let path = root.join("settings.json");
        fs::write(&path, "\u{feff}{\"defaultProvider\":\"ollama\"}").unwrap();
        store_pi_settings(&path, PI_CANDIDATE_ARTIFACT).unwrap();
        let rendered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(rendered["defaultProvider"], "ollama");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_settings_after_the_other_writer_releases_pis_lock() {
        let root = temporary_directory();
        let path = root.join("settings.json");
        let lock = lock_settings(&path).unwrap();
        let writer_path = path.clone();
        let (started, wait) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            started.send(()).unwrap();
            store_pi_settings(&writer_path, PI_CANDIDATE_ARTIFACT).unwrap();
        });
        wait.recv().unwrap();
        fs::write(
            &path,
            br#"{"defaultProvider":"ollama","foreign":"concurrent"}"#,
        )
        .unwrap();
        drop(lock);
        writer.join().unwrap();
        let rendered: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(rendered["foreign"], "concurrent");
        assert_eq!(rendered["defaultProvider"], "ollama");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn candidate_renders_exact_packages_and_full_registry() {
        let mut settings = Map::new();
        merge_pi_settings(&mut settings, PI_CANDIDATE_ARTIFACT);
        let rendered = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&rendered).unwrap(),
            json!({
                "packages": [
                    "npm:pi-web-access@0.28.0",
                    "npm:pi-subagents@0.65.1",
                    "npm:pi-background-tasks@2.5.0",
                    "npm:pi-mcp-adapter@2.32.1"
                ],
                "defaultTools": ["read", "bash", "powershell", "edit", "write", "grep", "find", "ls"]
            })
        );
    }

    #[test]
    fn production_renders_neither_key() {
        let mut settings = Map::new();
        settings.insert("defaultProvider".into(), json!("ollama"));
        merge_pi_settings(&mut settings, PI_ARTIFACT);
        assert_eq!(
            serde_json::to_value(settings).unwrap(),
            json!({"defaultProvider": "ollama"})
        );
    }

    #[test]
    fn preserves_foreign_keys_and_replaces_only_candidate_owned_values() {
        let original = json!({
            "defaultProvider": "ollama",
            "defaultModel": "local-model",
            "foreign": {"nested": [null, true, 42]},
            "packages": ["npm:pi-web-access@0.1.0"],
            "defaultTools": ["read"]
        });
        let mut settings = original.as_object().unwrap().clone();
        merge_pi_settings(&mut settings, PI_ARTIFACT);
        assert_eq!(Value::Object(settings.clone()), original);
        merge_pi_settings(&mut settings, PI_CANDIDATE_ARTIFACT);
        for key in ["defaultProvider", "defaultModel", "foreign"] {
            assert_eq!(settings[key], original[key]);
        }
        let merged = settings.clone();
        merge_pi_settings(&mut settings, PI_CANDIDATE_ARTIFACT);
        assert_eq!(settings, merged);
        assert_eq!(settings["packages"].as_array().unwrap().len(), 4);
        assert_eq!(settings["defaultTools"].as_array().unwrap().len(), 8);
    }
}
