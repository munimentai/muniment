//! The credential store hosts share: secrets named by a service and an entry
//! name. The desktop keeps them in the platform keychain. A host without one,
//! such as a server or a container, keeps them in owner-only files through
//! [`FileCredentialStore`]. The runtime and the router gateway both read and
//! write through [`CredentialStore`].

use std::path::{Path, PathBuf};

/// Secrets a host keeps, each named by a service and an entry name.
pub trait CredentialStore: Send + Sync {
    /// The stored secret, or none.
    fn read(&self, service: &str, name: &str) -> Option<String>;

    /// The stored secret, none when the store holds none, or the store's
    /// refusal. A caller never replaces a secret the store refused to read.
    fn lookup(&self, service: &str, name: &str) -> Result<Option<String>, CredentialRefusal> {
        Ok(self.read(service, name))
    }

    /// Stores a secret, replacing the one there. The error names the store's refusal.
    fn write(&self, service: &str, name: &str, secret: &str) -> Result<(), String>;

    /// Removes a secret. Removing a secret that is not there succeeds.
    fn remove(&self, service: &str, name: &str) -> Result<(), String>;

    /// The entries the store refused and holds back, each with its refusal.
    fn refusals(&self) -> Vec<RefusedEntry> {
        Vec::new()
    }

    /// The person asked to try again: the store forgets every refusal, so
    /// the next read of each entry asks the system again.
    fn retry_refused(&self) {}
}

/// One entry a store refused and holds back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedEntry {
    pub service: String,
    pub name: String,
    pub refusal: CredentialRefusal,
}

/// Why a store answered no secret for an entry it may hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialRefusal {
    /// The person declined the system's access prompt, or the store may not
    /// show one now. Another read shows the prompt again.
    Declined(String),
    /// The store failed. The text names the failure.
    Failed(String),
}

impl CredentialRefusal {
    /// The sentence that names the refusal.
    pub fn message(&self) -> &str {
        match self {
            Self::Declined(message) | Self::Failed(message) => message,
        }
    }
}

/// The first wait after a store failure.
pub const FIRST_REFUSAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30);
/// The longest wait after store failures in a row.
pub const MAX_REFUSAL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// The wait after `failures` store failures in a row: 30 seconds, doubling
/// up to half an hour.
pub fn refusal_backoff(failures: u32) -> std::time::Duration {
    let doublings = failures.saturating_sub(1).min(16);
    FIRST_REFUSAL_BACKOFF
        .saturating_mul(1 << doublings)
        .min(MAX_REFUSAL_BACKOFF)
}

/// What a store remembers of the entries the system refused, so a refused
/// entry never raises the system prompt in a loop and never takes a new
/// secret over the one the store holds. A decline holds back every entry of
/// its service until the person retries or the process ends. Any other
/// failure holds back its entry for a backoff that doubles with each failure
/// in a row.
#[derive(Debug, Default)]
pub struct RefusalMemory {
    entries: std::sync::Mutex<std::collections::BTreeMap<(String, String), HeldRefusal>>,
}

#[derive(Clone, Debug)]
struct HeldRefusal {
    refusal: CredentialRefusal,
    failures: u32,
    retry_at: Option<std::time::Instant>,
}

impl RefusalMemory {
    fn entries(
        &self,
    ) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<(String, String), HeldRefusal>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The refusal that still holds for an entry at `now`, or none when the
    /// system may hear a request for it.
    pub fn held(
        &self,
        service: &str,
        name: &str,
        now: std::time::Instant,
    ) -> Option<CredentialRefusal> {
        let entries = self.entries();
        if let Some(held) = entries
            .get(&(service.to_owned(), name.to_owned()))
            .filter(|held| held.retry_at.is_none_or(|at| now < at))
        {
            return Some(held.refusal.clone());
        }
        // A declined prompt holds back every entry of its service, so a
        // service with many entries raises the prompt once, not once each.
        entries
            .iter()
            .find(|((held_service, _), held)| {
                held_service == service && matches!(held.refusal, CredentialRefusal::Declined(_))
            })
            .map(|(_, held)| held.refusal.clone())
    }

    /// Whether the system refused an entry and no answer came since. A
    /// store never writes over such an entry.
    pub fn refused(&self, service: &str, name: &str) -> bool {
        self.entries()
            .contains_key(&(service.to_owned(), name.to_owned()))
    }

    /// Records the system's refusal of an entry at `now`.
    pub fn note(
        &self,
        service: &str,
        name: &str,
        refusal: &CredentialRefusal,
        now: std::time::Instant,
    ) {
        let mut entries = self.entries();
        let key = (service.to_owned(), name.to_owned());
        let failures = entries
            .get(&key)
            .map_or(0, |held| held.failures)
            .saturating_add(1);
        let retry_at = match refusal {
            CredentialRefusal::Declined(_) => None,
            CredentialRefusal::Failed(_) => Some(now + refusal_backoff(failures)),
        };
        entries.insert(
            key,
            HeldRefusal {
                refusal: refusal.clone(),
                failures,
                retry_at,
            },
        );
    }

    /// The system answered for an entry: its refusal goes.
    pub fn answered(&self, service: &str, name: &str) {
        self.entries()
            .remove(&(service.to_owned(), name.to_owned()));
    }

    /// Every refusal the memory holds.
    pub fn list(&self) -> Vec<RefusedEntry> {
        self.entries()
            .iter()
            .map(|((service, name), held)| RefusedEntry {
                service: service.clone(),
                name: name.clone(),
                refusal: held.refusal.clone(),
            })
            .collect()
    }

    /// The person asked to try again: every refusal goes.
    pub fn clear(&self) {
        self.entries().clear();
    }
}

/// The directory under a host's state root that [`FileCredentialStore`]
/// keeps secrets in.
pub const CREDENTIALS_DIRECTORY_NAME: &str = "credentials";

/// The prefix of the environment variables that override stored secrets.
pub const VARIABLE_PREFIX: &str = "MUNIMENT_SECRET_";

/// Secrets in owner-only files, with read-only overrides from the environment,
/// for a host without a keychain.
///
/// The secret `name` of `service` lives in the file `<directory>/<service>/<name>`
/// with mode 0600, under directories with mode 0700. In each of the two parts,
/// a byte other than an ASCII letter, a digit, `.`, `_` or `-` is written as
/// `%` and two hex digits, and so is a leading `.`, so a name never leaves its
/// service's directory and no file is hidden. One trailing newline in a file
/// is not part of the secret, so a file written by hand reads as typed.
///
/// The variable [`credential_variable`] names overrides the file: a read
/// answers its value, and a write refuses while it is set. An empty file or
/// an empty variable reads as no secret.
pub struct FileCredentialStore {
    directory: Option<PathBuf>,
}

impl FileCredentialStore {
    /// A store whose files live under `directory`.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: Some(directory.into()),
        }
    }

    /// A store with no directory: it reads the environment alone and refuses
    /// every write.
    pub fn environment_only() -> Self {
        Self { directory: None }
    }

    fn path(&self, service: &str, name: &str) -> Option<PathBuf> {
        let directory = self.directory.as_ref()?;
        if service.is_empty() || name.is_empty() {
            return None;
        }
        Some(
            directory
                .join(file_component(service))
                .join(file_component(name)),
        )
    }
}

/// The environment variable that overrides the secret `name` of `service`:
/// `MUNIMENT_SECRET_`, then the service, `_` and the name, upper-cased, with
/// every character other than an ASCII letter or digit written as `_`. The
/// reader's `ai.muniment.reader` secret `stripe` is
/// `MUNIMENT_SECRET_AI_MUNIMENT_READER_STRIPE`.
pub fn credential_variable(service: &str, name: &str) -> String {
    let part = |value: &str| -> String {
        value
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect()
    };
    format!("{VARIABLE_PREFIX}{}_{}", part(service), part(name))
}

fn file_component(value: &str) -> String {
    let mut component = String::with_capacity(value.len());
    for (index, byte) in value.bytes().enumerate() {
        let plain = byte.is_ascii_alphanumeric()
            || matches!(byte, b'_' | b'-')
            || (byte == b'.' && index > 0);
        if plain {
            component.push(char::from(byte));
        } else {
            component.push_str(&format!("%{byte:02X}"));
        }
    }
    component
}

fn create_private_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        // A directory that already existed keeps its mode, so set it here.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

impl CredentialStore for FileCredentialStore {
    fn read(&self, service: &str, name: &str) -> Option<String> {
        if let Some(value) = std::env::var_os(credential_variable(service, name)) {
            return value.into_string().ok().filter(|value| !value.is_empty());
        }
        let mut secret = std::fs::read_to_string(self.path(service, name)?).ok()?;
        if secret.ends_with('\n') {
            secret.pop();
            if secret.ends_with('\r') {
                secret.pop();
            }
        }
        (!secret.is_empty()).then_some(secret)
    }

    fn write(&self, service: &str, name: &str, secret: &str) -> Result<(), String> {
        let variable = credential_variable(service, name);
        if std::env::var_os(&variable).is_some() {
            return Err(format!(
                "{variable} sets this secret, so the credential store keeps it."
            ));
        }
        let path = self
            .path(service, name)
            .ok_or("The credential store has no directory for this secret.")?;
        let refused =
            |error: std::io::Error| format!("The credential store refused the secret: {error}.");
        let parent = path
            .parent()
            .expect("a secret's path names its service directory");
        if let Some(directory) = &self.directory {
            create_private_directory(directory).map_err(refused)?;
        }
        create_private_directory(parent).map_err(refused)?;
        crate::config::write_private(&path, secret.as_bytes()).map_err(refused)
    }

    fn remove(&self, service: &str, name: &str) -> Result<(), String> {
        let variable = credential_variable(service, name);
        if std::env::var_os(&variable).is_some() {
            return Err(format!(
                "{variable} sets this secret, so the credential store keeps it."
            ));
        }
        let Some(path) = self.path(service, name) else {
            return Ok(());
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("The credential store kept the secret: {error}.")),
        }
    }
}

#[cfg(test)]
mod refusal_memory_tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn declined() -> CredentialRefusal {
        CredentialRefusal::Declined("User canceled the operation.".into())
    }

    fn failed() -> CredentialRefusal {
        CredentialRefusal::Failed("The keychain is not available.".into())
    }

    #[test]
    fn a_decline_holds_until_a_retry_however_long() {
        let memory = RefusalMemory::default();
        let now = Instant::now();
        assert_eq!(memory.held("s", "a", now), None);
        memory.note("s", "a", &declined(), now);
        assert_eq!(memory.held("s", "a", now), Some(declined()));
        assert_eq!(
            memory.held("s", "a", now + Duration::from_secs(365 * 24 * 3600)),
            Some(declined())
        );
        assert!(memory.refused("s", "a"));
        // The decline holds back the service's other entries, which a write
        // may still create, and no other service.
        assert_eq!(memory.held("s", "b", now), Some(declined()));
        assert!(!memory.refused("s", "b"));
        assert_eq!(memory.held("t", "a", now), None);
        assert_eq!(memory.list().len(), 1);
        memory.clear();
        assert_eq!(memory.held("s", "a", now), None);
        assert!(!memory.refused("s", "a"));
    }

    #[test]
    fn a_failure_backs_off_doubling_to_a_cap_and_an_answer_forgets_it() {
        let memory = RefusalMemory::default();
        let now = Instant::now();
        memory.note("s", "a", &failed(), now);
        assert!(memory
            .held("s", "a", now + Duration::from_secs(29))
            .is_some());
        assert!(memory
            .held("s", "a", now + Duration::from_secs(30))
            .is_none());
        // The wait is over, but the entry still may not be written over.
        assert!(memory.refused("s", "a"));
        memory.note("s", "a", &failed(), now);
        assert!(memory
            .held("s", "a", now + Duration::from_secs(59))
            .is_some());
        assert!(memory
            .held("s", "a", now + Duration::from_secs(60))
            .is_none());
        for _ in 0..40 {
            memory.note("s", "a", &failed(), now);
        }
        assert!(memory
            .held("s", "a", now + MAX_REFUSAL_BACKOFF - Duration::from_secs(1))
            .is_some());
        assert!(memory.held("s", "a", now + MAX_REFUSAL_BACKOFF).is_none());
        memory.answered("s", "a");
        memory.note("s", "a", &failed(), now);
        assert!(memory.held("s", "a", now + FIRST_REFUSAL_BACKOFF).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "muniment-credentials-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_secret_round_trips_through_an_owner_only_file() {
        let root = temporary_directory("files");
        let store = FileCredentialStore::new(root.join("credentials"));
        assert_eq!(store.read("test.files", "first"), None);
        store.write("test.files", "first", "sk_old").unwrap();
        store.write("test.files", "first", "sk_new").unwrap();
        assert_eq!(store.read("test.files", "first").as_deref(), Some("sk_new"));
        let path = root.join("credentials").join("test.files").join("first");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "sk_new");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
            assert_eq!(mode(&root.join("credentials")), 0o700);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_file_written_by_hand_reads_without_its_trailing_newline() {
        let root = temporary_directory("hand");
        let store = FileCredentialStore::new(&root);
        std::fs::create_dir_all(root.join("test.hand")).unwrap();
        std::fs::write(root.join("test.hand").join("unix"), "sk_1\n").unwrap();
        std::fs::write(root.join("test.hand").join("dos"), "sk_2\r\n").unwrap();
        std::fs::write(root.join("test.hand").join("empty"), "\n").unwrap();
        assert_eq!(store.read("test.hand", "unix").as_deref(), Some("sk_1"));
        assert_eq!(store.read("test.hand", "dos").as_deref(), Some("sk_2"));
        assert_eq!(store.read("test.hand", "empty"), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_name_never_leaves_its_service_directory() {
        let root = temporary_directory("names");
        let store = FileCredentialStore::new(root.join("credentials"));
        store.write("..", "../escape", "sk_1").unwrap();
        store.write("test.names", "a/b c", "sk_2").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("credentials").join("%2E.").join("%2E.%2Fescape"))
                .unwrap(),
            "sk_1"
        );
        assert_eq!(store.read("test.names", "a/b c").as_deref(), Some("sk_2"));
        assert!(root
            .join("credentials")
            .join("test.names")
            .join("a%2Fb%20c")
            .is_file());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        assert!(store.write("test.names", "", "sk_3").is_err());
        assert_eq!(store.read("", "name"), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_variable_name_upper_cases_and_replaces_other_characters() {
        assert_eq!(
            credential_variable("ai.muniment.reader", "stripe"),
            "MUNIMENT_SECRET_AI_MUNIMENT_READER_STRIPE"
        );
        assert_eq!(
            credential_variable("svc-1", "a.b/c"),
            "MUNIMENT_SECRET_SVC_1_A_B_C"
        );
    }

    #[test]
    fn the_environment_overrides_the_file_and_refuses_writes() {
        let root = temporary_directory("environment");
        let store = FileCredentialStore::new(&root);
        store.write("test.environment", "key", "from-file").unwrap();
        let variable = credential_variable("test.environment", "key");
        std::env::set_var(&variable, "from-environment");
        assert_eq!(
            store.read("test.environment", "key").as_deref(),
            Some("from-environment")
        );
        let refusal = store
            .write("test.environment", "key", "changed")
            .unwrap_err();
        assert!(refusal.contains(&variable), "{refusal}");
        assert!(!refusal.contains("from-environment"), "{refusal}");
        std::env::set_var(&variable, "");
        assert_eq!(store.read("test.environment", "key"), None);
        std::env::remove_var(&variable);
        assert_eq!(
            store.read("test.environment", "key").as_deref(),
            Some("from-file")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_removed_secret_reads_as_none() {
        let root = temporary_directory("remove");
        let store = FileCredentialStore::new(&root);
        store.write("test.remove", "key", "sk_1").unwrap();
        store.remove("test.remove", "key").unwrap();
        assert_eq!(store.read("test.remove", "key"), None);
        store.remove("test.remove", "key").unwrap();
        let variable = credential_variable("test.remove", "pinned");
        std::env::set_var(&variable, "sk_env");
        assert!(store.remove("test.remove", "pinned").is_err());
        std::env::remove_var(&variable);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_store_without_a_directory_reads_the_environment_alone() {
        let store = FileCredentialStore::environment_only();
        let variable = credential_variable("test.nodirectory", "key");
        assert_eq!(store.read("test.nodirectory", "key"), None);
        assert!(store.write("test.nodirectory", "key", "sk_1").is_err());
        std::env::set_var(&variable, "sk_env");
        assert_eq!(
            store.read("test.nodirectory", "key").as_deref(),
            Some("sk_env")
        );
        std::env::remove_var(&variable);
    }
}
