//! The remote listener's TLS identity and the runtime's id and display name.
//!
//! The first open generates an ECDSA P-256 key and a self-signed certificate.
//! Clients pin the certificate's SHA-256, so the identity is never replaced
//! once written: a missing certificate or key beside an existing identity
//! record fails the open instead of silently changing the fingerprint.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use muniment_attach::{is_valid_display_name, CertificateFingerprint, Id, InstallKind};

const IDENTITY_FILE: &str = "identity.json";
const CERTIFICATE_FILE: &str = "identity-certificate.der";
const KEY_FILE: &str = "identity-key.der";
/// The generic display name of a runtime that announces no name of its own.
/// A stored identity with this name takes the host's descriptive default.
pub const DEFAULT_RUNTIME_NAME: &str = "Muniment runtime";

/// The display name a runtime takes when its host names none: the kind of
/// container it runs in, `<hostname> (server)` on any other headless host,
/// and the hostname on a desktop.
pub fn default_runtime_name(install_kind: InstallKind) -> String {
    default_runtime_name_in(install_kind, Path::new("/"), host_name().as_deref())
}

/// The default name with the container markers read under `root`.
fn default_runtime_name_in(install_kind: InstallKind, root: &Path, host: Option<&str>) -> String {
    // Podman writes /run/.containerenv and Docker writes /.dockerenv. A
    // container's hostname is its generated id, so the kind names it.
    if root.join("run/.containerenv").exists() {
        return "Podman container".to_owned();
    }
    if install_kind == InstallKind::Container || root.join(".dockerenv").exists() {
        return "Docker container".to_owned();
    }
    let host = host
        .map(|host| host.trim().trim_end_matches(".local").trim())
        .filter(|host| !host.is_empty());
    let name = match (install_kind, host) {
        (_, None) => return DEFAULT_RUNTIME_NAME.to_owned(),
        (InstallKind::Desktop, Some(host)) => host.to_owned(),
        (_, Some(host)) => format!("{host} (server)"),
    };
    if is_valid_display_name(&name) {
        name
    } else {
        DEFAULT_RUNTIME_NAME.to_owned()
    }
}

#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0_u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed with it.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0)?;
    String::from_utf8(buffer[..end].to_vec()).ok()
}

#[cfg(not(unix))]
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRecord {
    version: u32,
    runtime_id: String,
    runtime_name: String,
}

pub(super) struct RemoteIdentity {
    pub(super) runtime_id: String,
    pub(super) runtime_name: String,
    pub(super) certificate: Vec<u8>,
    key: Vec<u8>,
    pub(super) fingerprint: CertificateFingerprint,
}

impl RemoteIdentity {
    /// Loads the identity under `directory`, or creates it on first use. A
    /// valid `name` replaces the stored display name. Without one, a new
    /// identity or one that stores the generic name takes `default_name`.
    pub(super) fn load_or_create(
        directory: &Path,
        name: Option<&str>,
        default_name: &str,
    ) -> io::Result<Self> {
        let record_path = directory.join(IDENTITY_FILE);
        let identity = match read_private(&record_path)? {
            Some(bytes) => {
                let record: IdentityRecord = serde_json::from_slice(&bytes)
                    .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                if record.version != 1
                    || Id::new(record.runtime_id.as_str()).is_err()
                    || !is_valid_display_name(&record.runtime_name)
                {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let certificate = read_private(&directory.join(CERTIFICATE_FILE))?
                    .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
                let key = read_private(&directory.join(KEY_FILE))?
                    .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
                Self {
                    runtime_id: record.runtime_id,
                    runtime_name: record.runtime_name,
                    fingerprint: CertificateFingerprint::of_certificate(&certificate),
                    certificate,
                    key,
                }
            }
            None => Self::create(directory, default_name)?,
        };
        let mut identity = identity;
        // The configuration check proves the key signs for the certificate.
        identity
            .server_config()
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let name = name.filter(|name| is_valid_display_name(name)).or_else(|| {
            (identity.runtime_name == DEFAULT_RUNTIME_NAME && is_valid_display_name(default_name))
                .then_some(default_name)
        });
        if let Some(name) = name {
            if name != identity.runtime_name {
                identity.runtime_name = name.to_owned();
                identity.write_record(directory)?;
            }
        }
        Ok(identity)
    }

    fn create(directory: &Path, default_name: &str) -> io::Result<Self> {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
            .map_err(io::Error::other)?;
        let mut params =
            rcgen::CertificateParams::new(Vec::<String>::new()).map_err(io::Error::other)?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, DEFAULT_RUNTIME_NAME);
        let certificate = params.self_signed(&key).map_err(io::Error::other)?;
        let identity = Self {
            runtime_id: uuid::Uuid::new_v4().to_string(),
            runtime_name: if is_valid_display_name(default_name) {
                default_name.to_owned()
            } else {
                DEFAULT_RUNTIME_NAME.to_owned()
            },
            fingerprint: CertificateFingerprint::of_certificate(certificate.der()),
            certificate: certificate.der().to_vec(),
            key: key.serialize_der(),
        };
        write_private(&directory.join(KEY_FILE), &identity.key)?;
        write_private(&directory.join(CERTIFICATE_FILE), &identity.certificate)?;
        // The record is written last, so a partial first run starts over.
        identity.write_record(directory)?;
        Ok(identity)
    }

    fn write_record(&self, directory: &Path) -> io::Result<()> {
        let record = serde_json::to_vec(&IdentityRecord {
            version: 1,
            runtime_id: self.runtime_id.clone(),
            runtime_name: self.runtime_name.clone(),
        })
        .map_err(io::Error::other)?;
        write_private(&directory.join(IDENTITY_FILE), &record)
    }

    /// A TLS 1.3 server configuration with this identity, no session tickets
    /// and no session cache.
    pub(super) fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, rustls::Error> {
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(
                self.certificate.clone(),
            )],
            rustls::pki_types::PrivateKeyDer::Pkcs8(self.key.clone().into()),
        )?;
        config.send_tls13_tickets = 0;
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        Ok(Arc::new(config))
    }

    /// A signer with this identity's key, which signs the runtime's fleet
    /// statements.
    pub(super) fn signer(&self) -> io::Result<IdentitySigner> {
        IdentitySigner::from_pkcs8(&self.key, self.fingerprint.to_hex())
    }
}

/// Signs with a runtime's P-256 identity key, the key behind its pinned TLS
/// certificate. Other members check its signatures against the public key
/// the fleet roster names for the runtime.
pub struct IdentitySigner {
    pair: ring::signature::EcdsaKeyPair,
    certificate_sha256: String,
}

impl std::fmt::Debug for IdentitySigner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentitySigner")
            .field("certificate_sha256", &self.certificate_sha256)
            .finish_non_exhaustive()
    }
}

impl IdentitySigner {
    fn from_pkcs8(key: &[u8], certificate_sha256: String) -> io::Result<Self> {
        let pair = ring::signature::EcdsaKeyPair::from_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            key,
            &ring::rand::SystemRandom::new(),
        )
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        Ok(Self {
            pair,
            certificate_sha256,
        })
    }

    /// A signer with a new key and a made-up certificate digest, for tests
    /// that run members without a listener.
    #[doc(hidden)]
    pub fn generate() -> Self {
        let rng = ring::rand::SystemRandom::new();
        let key = ring::signature::EcdsaKeyPair::generate_pkcs8(
            &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &rng,
        )
        .expect("the system random source works");
        let digest = CertificateFingerprint::of_certificate(key.as_ref()).to_hex();
        Self::from_pkcs8(key.as_ref(), digest).expect("a generated key parses")
    }

    /// The uncompressed public key point.
    pub fn public_key(&self) -> &[u8] {
        use ring::signature::KeyPair as _;
        self.pair.public_key().as_ref()
    }

    /// The SHA-256 of the runtime's certificate, in lowercase hex.
    pub fn certificate_sha256(&self) -> &str {
        &self.certificate_sha256
    }

    /// A fixed-length ECDSA P-256 SHA-256 signature of `message`.
    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, String> {
        self.pair
            .sign(&ring::rand::SystemRandom::new(), message)
            .map(|signature| signature.as_ref().to_vec())
            .map_err(|_| "The identity key could not sign.".to_owned())
    }
}

/// Whether `signature` is the identity key `public_key`'s signature of
/// `message`.
pub fn verify_identity_signature(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, public_key)
        .verify(message, signature)
        .is_ok()
}

/// Creates `directory` and, on Unix, makes it readable by its owner only.
pub(super) fn ensure_private_directory(directory: &Path) -> io::Result<()> {
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Reads a file the runtime wrote for its owner. `None` means it does not
/// exist. On Unix a file another user owns, or that its group or others can
/// read, is refused.
pub(super) fn read_private(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut file, &mut bytes)?;
    Ok(Some(bytes))
}

/// Replaces `path` with `bytes` through a new owner-only file.
pub(super) fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = temporary_path(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let written = options
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .and_then(|()| crate::atomic_file::replace(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::now_v7()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "muniment-remote-identity-{name}-{}",
            uuid::Uuid::now_v7()
        ));
        ensure_private_directory(&directory).unwrap();
        directory
    }

    #[test]
    fn the_identity_is_created_once_and_reloaded_unchanged() {
        let directory = directory("reload");
        let created = RemoteIdentity::load_or_create(&directory, None, "Docker container").unwrap();
        assert!(Id::new(created.runtime_id.as_str()).is_ok());
        assert_eq!(created.runtime_name, "Docker container");
        assert_eq!(
            created.fingerprint,
            CertificateFingerprint::of_certificate(&created.certificate)
        );
        let reloaded =
            RemoteIdentity::load_or_create(&directory, Some("Build box"), "Docker container")
                .unwrap();
        assert_eq!(reloaded.runtime_id, created.runtime_id);
        assert_eq!(reloaded.fingerprint, created.fingerprint);
        assert_eq!(reloaded.runtime_name, "Build box");
        #[cfg(unix)]
        for file in [IDENTITY_FILE, CERTIFICATE_FILE, KEY_FILE] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(directory.join(file))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_stored_generic_name_takes_the_default_and_a_set_name_stays() {
        let directory = directory("default-name");
        RemoteIdentity::load_or_create(&directory, None, DEFAULT_RUNTIME_NAME).unwrap();
        let named = RemoteIdentity::load_or_create(&directory, None, "Docker container").unwrap();
        assert_eq!(named.runtime_name, "Docker container");
        RemoteIdentity::load_or_create(&directory, Some("Build box"), "Docker container").unwrap();
        let kept = RemoteIdentity::load_or_create(&directory, None, "Docker container").unwrap();
        assert_eq!(kept.runtime_name, "Build box");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_default_name_says_what_kind_of_host_runs_the_runtime() {
        let root = directory("markers");
        let host = Some("build-01");
        assert_eq!(
            default_runtime_name_in(InstallKind::Server, &root, host),
            "build-01 (server)"
        );
        assert_eq!(
            default_runtime_name_in(InstallKind::Desktop, &root, Some("studio.local")),
            "studio"
        );
        assert_eq!(
            default_runtime_name_in(InstallKind::Container, &root, host),
            "Docker container"
        );
        assert_eq!(
            default_runtime_name_in(InstallKind::Server, &root, None),
            DEFAULT_RUNTIME_NAME
        );
        assert_eq!(
            default_runtime_name_in(InstallKind::Server, &root, Some(&"x".repeat(200))),
            DEFAULT_RUNTIME_NAME
        );
        std::fs::write(root.join(".dockerenv"), b"").unwrap();
        assert_eq!(
            default_runtime_name_in(InstallKind::Server, &root, host),
            "Docker container"
        );
        std::fs::create_dir_all(root.join("run")).unwrap();
        std::fs::write(root.join("run/.containerenv"), b"").unwrap();
        assert_eq!(
            default_runtime_name_in(InstallKind::Server, &root, host),
            "Podman container"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_signer_signs_with_the_certificate_key() {
        let directory = directory("signer");
        let identity =
            RemoteIdentity::load_or_create(&directory, None, DEFAULT_RUNTIME_NAME).unwrap();
        let signer = identity.signer().unwrap();
        assert_eq!(signer.certificate_sha256(), identity.fingerprint.to_hex());
        let signature = signer.sign(b"statement").unwrap();
        assert!(verify_identity_signature(
            signer.public_key(),
            b"statement",
            &signature
        ));
        assert!(!verify_identity_signature(
            signer.public_key(),
            b"other",
            &signature
        ));
        let other = IdentitySigner::generate();
        assert!(!verify_identity_signature(
            other.public_key(),
            b"statement",
            &signature
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_missing_key_fails_closed_instead_of_changing_the_fingerprint() {
        let directory = directory("missing-key");
        RemoteIdentity::load_or_create(&directory, None, DEFAULT_RUNTIME_NAME).unwrap();
        std::fs::remove_file(directory.join(KEY_FILE)).unwrap();
        assert!(RemoteIdentity::load_or_create(&directory, None, DEFAULT_RUNTIME_NAME).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_key_others_can_read_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let directory = directory("mode");
        RemoteIdentity::load_or_create(&directory, None, DEFAULT_RUNTIME_NAME).unwrap();
        std::fs::set_permissions(
            directory.join(KEY_FILE),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert_eq!(
            RemoteIdentity::load_or_create(&directory, None, DEFAULT_RUNTIME_NAME)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
