//! The paired runtimes this app keeps: one owner-only record file per runtime,
//! which holds its device credential, and one index that names each runtime,
//! remembers how to reach it over SSH, and holds the selected runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use muniment_core::attach::{is_valid_display_name, RemoteRuntimeRecord};
use serde::{Deserialize, Serialize};

/// The id the app gives its own runtime.
pub(crate) const LOCAL_RUNTIME_ID: &str = "local";
const INDEX_FILE: &str = "runtimes.json";

/// How the app reaches a runtime through an SSH local forward.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SshTarget {
    pub(crate) host: String,
    pub(crate) command: String,
    pub(crate) remote_port: u16,
}

/// What the index keeps about one paired runtime. It holds no secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeEntry {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ssh: Option<SshTarget>,
    /// The version and install kind from the runtime's last welcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) install_kind: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RuntimeIndex {
    /// The selected paired runtime. `None` selects this computer's runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) selected: Option<String>,
    #[serde(default)]
    pub(crate) runtimes: BTreeMap<String, RuntimeEntry>,
}

/// One paired runtime as the registry lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PairedRuntime {
    pub(crate) id: String,
    pub(crate) entry: RuntimeEntry,
    pub(crate) scopes: BTreeSet<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Registry {
    directory: PathBuf,
}

/// The canonical form of a runtime id, which also names its record file, or
/// `None` when the text is not a runtime id.
pub(crate) fn canonical_id(id: &str) -> Option<String> {
    uuid::Uuid::parse_str(id)
        .ok()
        .map(|id| id.hyphenated().to_string())
}

impl Registry {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// The registry under the app's state directory.
    pub(crate) fn open_default() -> io::Result<Self> {
        muniment_runtime::profile_directory()
            .map(|profile| Self::new(profile.join("runtimes")))
            .map_err(|_| io::Error::from(io::ErrorKind::NotFound))
    }

    pub(crate) fn record_path(&self, id: &str) -> io::Result<PathBuf> {
        canonical_id(id)
            .map(|id| self.directory.join(format!("{id}.json")))
            .ok_or_else(|| io::ErrorKind::InvalidInput.into())
    }

    /// Reads the index. A missing or unreadable index lists no runtime.
    pub(crate) fn index(&self) -> RuntimeIndex {
        std::fs::read(self.directory.join(INDEX_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_index(&self, index: &RuntimeIndex) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(index).map_err(io::Error::other)?;
        write_owner_only(&self.directory.join(INDEX_FILE), &bytes)
    }

    fn update<T>(&self, change: impl FnOnce(&mut RuntimeIndex) -> io::Result<T>) -> io::Result<T> {
        let mut index = self.index();
        let result = change(&mut index)?;
        self.save_index(&index)?;
        Ok(result)
    }

    /// The paired runtimes whose record still loads, in name order.
    pub(crate) fn list(&self) -> Vec<PairedRuntime> {
        let index = self.index();
        let mut runtimes: Vec<PairedRuntime> = index
            .runtimes
            .into_iter()
            .filter_map(|(id, entry)| {
                let record = self.record(&id).ok()?;
                Some(PairedRuntime {
                    id,
                    entry,
                    scopes: record.scopes,
                })
            })
            .collect();
        runtimes.sort_by(|left, right| {
            left.entry
                .name
                .to_lowercase()
                .cmp(&right.entry.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id))
        });
        runtimes
    }

    pub(crate) fn entry(&self, id: &str) -> Option<RuntimeEntry> {
        self.index().runtimes.get(&canonical_id(id)?).cloned()
    }

    pub(crate) fn record(&self, id: &str) -> io::Result<RemoteRuntimeRecord> {
        RemoteRuntimeRecord::load(&self.record_path(id)?)
    }

    /// Saves a newly paired runtime. Pairing a runtime again replaces its
    /// record and keeps the name the user gave it. A record that names a
    /// paired runtime's id with another certificate is refused with
    /// `AlreadyExists`: a runtime keeps its certificate, so the link points
    /// somewhere else.
    pub(crate) fn add(
        &self,
        record: &RemoteRuntimeRecord,
        ssh: Option<SshTarget>,
        version: Option<String>,
        install_kind: Option<String>,
    ) -> io::Result<String> {
        let id = canonical_id(record.runtime_id.as_str())
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let path = self.record_path(&id)?;
        if path.exists() && RemoteRuntimeRecord::load(&path)?.fingerprint != record.fingerprint {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        record.save(&path)?;
        self.update(|index| {
            let name = index
                .runtimes
                .get(&id)
                .map_or_else(|| record.runtime_name.clone(), |entry| entry.name.clone());
            index.runtimes.insert(
                id.clone(),
                RuntimeEntry {
                    name,
                    ssh,
                    version,
                    install_kind,
                },
            );
            Ok(id.clone())
        })
    }

    pub(crate) fn rename(&self, id: &str, name: &str) -> io::Result<()> {
        let name = name.trim();
        if !is_valid_display_name(name) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let id = canonical_id(id).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.update(|index| {
            let entry = index
                .runtimes
                .get_mut(&id)
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            entry.name = name.to_owned();
            Ok(())
        })
    }

    /// Records what the runtime's welcome said about itself.
    pub(crate) fn observe(
        &self,
        id: &str,
        version: Option<String>,
        install_kind: Option<String>,
    ) -> io::Result<()> {
        let Some(id) = canonical_id(id) else {
            return Ok(());
        };
        let index = self.index();
        let Some(entry) = index.runtimes.get(&id) else {
            return Ok(());
        };
        if entry.version == version && entry.install_kind == install_kind {
            return Ok(());
        }
        self.update(|index| {
            if let Some(entry) = index.runtimes.get_mut(&id) {
                entry.version = version;
                entry.install_kind = install_kind;
            }
            Ok(())
        })
    }

    /// Forgets a runtime and deletes its credential file. Removing the
    /// selected runtime selects this computer.
    pub(crate) fn remove(&self, id: &str) -> io::Result<()> {
        let id = canonical_id(id).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        match std::fs::remove_file(self.record_path(&id)?) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        self.update(|index| {
            index.runtimes.remove(&id);
            if index.selected.as_deref() == Some(id.as_str()) {
                index.selected = None;
            }
            Ok(())
        })
    }

    /// The selected paired runtime, or `None` for this computer. A selection
    /// whose record no longer loads selects this computer.
    pub(crate) fn selected(&self) -> Option<String> {
        let selected = self.index().selected?;
        self.record(&selected).ok().map(|_| selected)
    }

    pub(crate) fn select(&self, id: &str) -> io::Result<()> {
        let selected = if id == LOCAL_RUNTIME_ID {
            None
        } else {
            let id =
                canonical_id(id).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
            self.record(&id)?;
            Some(id)
        };
        self.update(|index| {
            if selected
                .as_ref()
                .is_some_and(|id| !index.runtimes.contains_key(id))
            {
                return Err(io::ErrorKind::NotFound.into());
            }
            index.selected = selected;
            Ok(())
        })
    }
}

/// Writes `bytes` through a new owner-only file that replaces `path`.
fn write_owner_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::now_v7()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_core::attach::{CertificateFingerprint, DeviceCredential, Id};

    pub(crate) const RUNTIME_ID: &str = "018f0000-0000-7000-8000-000000000042";

    pub(crate) fn record(id: &str, name: &str) -> RemoteRuntimeRecord {
        RemoteRuntimeRecord {
            runtime_id: Id::new(id).unwrap(),
            runtime_name: name.into(),
            endpoints: vec!["192.0.2.20:7443".into()],
            fingerprint: CertificateFingerprint::of_certificate(b"der"),
            credential: DeviceCredential {
                device_id: Id::new("018f0000-0000-7000-8000-000000000043").unwrap(),
                secret: "cd".repeat(32),
            },
            scopes: BTreeSet::from(["thread.read".to_owned(), "runtime.admin".to_owned()]),
        }
    }

    fn registry() -> (Registry, PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("muniment-runtimes-{}", uuid::Uuid::now_v7()));
        (Registry::new(directory.clone()), directory)
    }

    #[test]
    fn a_paired_runtime_is_listed_renamed_selected_and_removed() {
        let (registry, directory) = registry();
        assert!(registry.list().is_empty());
        assert_eq!(registry.selected(), None);

        let id = registry
            .add(
                &record(RUNTIME_ID, "Build box"),
                None,
                Some("1.2.0".into()),
                Some("server".into()),
            )
            .unwrap();
        assert_eq!(id, RUNTIME_ID);
        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].entry.name, "Build box");
        assert_eq!(listed[0].entry.version.as_deref(), Some("1.2.0"));
        assert!(listed[0].scopes.contains("runtime.admin"));

        registry.rename(RUNTIME_ID, "  Studio  ").unwrap();
        assert_eq!(registry.entry(RUNTIME_ID).unwrap().name, "Studio");
        assert!(registry.rename(RUNTIME_ID, " ").is_err());
        assert!(registry.rename(RUNTIME_ID, "a\nb").is_err());

        // Pairing again keeps the user's name and replaces the record.
        registry
            .add(&record(RUNTIME_ID, "Build box"), None, None, None)
            .unwrap();
        assert_eq!(registry.entry(RUNTIME_ID).unwrap().name, "Studio");

        registry.select(RUNTIME_ID).unwrap();
        assert_eq!(registry.selected().as_deref(), Some(RUNTIME_ID));
        registry.select(LOCAL_RUNTIME_ID).unwrap();
        assert_eq!(registry.selected(), None);
        registry.select(RUNTIME_ID).unwrap();

        let path = registry.record_path(RUNTIME_ID).unwrap();
        assert!(path.exists());
        registry.remove(RUNTIME_ID).unwrap();
        assert!(!path.exists());
        assert!(registry.list().is_empty());
        assert_eq!(registry.selected(), None);
        assert_eq!(registry.index().selected, None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_known_runtime_id_with_another_certificate_is_refused() {
        let (registry, directory) = registry();
        let id = "018f0000-0000-7000-8000-000000000051";
        registry.add(&record(id, "Studio"), None, None, None).unwrap();
        // Pairing the same runtime again with its own certificate is allowed.
        registry.add(&record(id, "Studio"), None, None, None).unwrap();
        let mut impostor = record(id, "Studio");
        impostor.endpoints = vec!["198.51.100.9:7443".into()];
        impostor.fingerprint = CertificateFingerprint::of_certificate(b"other der");
        assert_eq!(
            registry
                .add(&impostor, None, None, None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        let kept = registry.record(id).unwrap();
        assert_eq!(kept.endpoints, vec!["192.0.2.20:7443".to_owned()]);
        assert_eq!(kept.fingerprint, CertificateFingerprint::of_certificate(b"der"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn selection_refuses_an_unknown_runtime_and_ids_that_are_not_uuids() {
        let (registry, directory) = registry();
        assert!(registry.select(RUNTIME_ID).is_err());
        assert!(registry.select("../escape").is_err());
        assert!(registry.record_path("../escape").is_err());
        assert!(registry.remove("../escape").is_err());
        assert_eq!(
            canonical_id("018F0000-0000-7000-8000-000000000042").as_deref(),
            Some(RUNTIME_ID)
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_selection_whose_record_is_gone_selects_this_computer() {
        let (registry, directory) = registry();
        registry
            .add(&record(RUNTIME_ID, "Box"), None, None, None)
            .unwrap();
        registry.select(RUNTIME_ID).unwrap();
        std::fs::remove_file(registry.record_path(RUNTIME_ID).unwrap()).unwrap();
        assert_eq!(registry.selected(), None);
        assert!(registry.list().is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_index_holds_no_secret_and_every_file_is_owner_only() {
        let (registry, directory) = registry();
        registry
            .add(
                &record(RUNTIME_ID, "Box"),
                Some(SshTarget {
                    host: "box".into(),
                    command: "muniment-server".into(),
                    remote_port: 7443,
                }),
                None,
                None,
            )
            .unwrap();
        let index = std::fs::read_to_string(directory.join(INDEX_FILE)).unwrap();
        assert!(!index.contains(&"cd".repeat(32)));
        assert!(index.contains("\"host\": \"box\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for entry in std::fs::read_dir(&directory).unwrap() {
                let mode = entry.unwrap().metadata().unwrap().permissions().mode();
                assert_eq!(mode & 0o077, 0);
            }
        }
        registry
            .observe(RUNTIME_ID, Some("2.0.0".into()), Some("container".into()))
            .unwrap();
        let entry = registry.entry(RUNTIME_ID).unwrap();
        assert_eq!(entry.version.as_deref(), Some("2.0.0"));
        assert_eq!(entry.install_kind.as_deref(), Some("container"));
        assert_eq!(entry.ssh.unwrap().remote_port, 7443);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
