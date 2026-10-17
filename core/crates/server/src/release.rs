//! Signed server releases: where the launcher fetches them, and how it
//! verifies and unpacks one.
//!
//! Each release carries `muniment-server-<version>-linux-<arch>.tar.gz` for
//! each architecture, a `SHA256SUMS` file that lists them, and
//! `SHA256SUMS.minisig`, a minisign signature over `SHA256SUMS`. The launcher
//! checks the signature against the release public key first, then the
//! archive's SHA-256 against its line in `SHA256SUMS`, and unpacks only
//! regular files into one flat directory.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use muniment_core::sha2::{Digest, Sha256};

/// RELEASE SIGNING KEY: the base64 line of the minisign public key whose
/// secret key signs releases, the second line of its `.pub` file. The
/// release job refuses to publish when this is empty or does not verify the
/// release it signed. While it is empty, a launcher installs nothing unless
/// `MUNIMENT_UPDATE_PUBLIC_KEY` names a key.
pub const RELEASE_PUBLIC_KEY: &str = "RWRhWqZZgdTwfIOji0I2jBTPsMHRQI8EGTROXXo9ZuRn2aUF1MPXd9XC";

/// Overrides the release public key, for tests and for a mirror that signs
/// its own releases.
pub const PUBLIC_KEY_VARIABLE: &str = "MUNIMENT_UPDATE_PUBLIC_KEY";
/// Overrides the base URL of the release feed. `<feed>/v<version>/<file>`
/// names each file. A `file://` feed reads a local directory.
pub const FEED_VARIABLE: &str = "MUNIMENT_UPDATE_FEED";
/// The public releases of this repository.
pub const DEFAULT_FEED: &str = "https://github.com/munimentai/muniment-core/releases/download";
pub const SUMS_NAME: &str = "SHA256SUMS";
pub const SIGNATURE_NAME: &str = "SHA256SUMS.minisig";
/// The server binary inside an archive.
pub const SERVER_NAME: &str = "muniment-server";

const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_LISTING_BYTES: u64 = 64 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a release did not install. A download error may pass on its own, so
/// the launcher retries it. The others name a release it never installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseError {
    Download(String),
    Signature(String),
    Digest(String),
    Archive(String),
}

impl ReleaseError {
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Download(_))
    }
}

impl std::fmt::Display for ReleaseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Download(detail) => write!(formatter, "the download failed: {detail}"),
            Self::Signature(detail) => write!(formatter, "the signature did not verify: {detail}"),
            Self::Digest(detail) => write!(formatter, "the archive digest did not match: {detail}"),
            Self::Archive(detail) => write!(formatter, "the archive is unusable: {detail}"),
        }
    }
}

/// The archive architecture of this build, as release file names spell it.
pub fn host_arch() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("x86_64")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64")
    } else {
        None
    }
}

/// The archive file name of `version` for `arch`.
pub fn archive_name(version: &str, arch: &str) -> String {
    format!("{SERVER_NAME}-{version}-linux-{arch}.tar.gz")
}

/// The public key releases verify against: `MUNIMENT_UPDATE_PUBLIC_KEY`, or
/// the key this build carries. `None` when neither names one.
pub fn public_key() -> Option<String> {
    std::env::var(PUBLIC_KEY_VARIABLE)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .or_else(|| (!RELEASE_PUBLIC_KEY.is_empty()).then(|| RELEASE_PUBLIC_KEY.to_owned()))
}

/// Where release files come from.
#[derive(Clone, Debug)]
pub struct Feed {
    base: String,
}

impl Feed {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    /// `MUNIMENT_UPDATE_FEED`, or the public releases.
    pub fn from_env() -> Self {
        Self::new(
            &std::env::var(FEED_VARIABLE)
                .ok()
                .filter(|feed| !feed.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_FEED.to_owned()),
        )
    }

    pub fn url(&self, version: &str, name: &str) -> String {
        format!("{}/v{version}/{name}", self.base)
    }

    /// The bytes of one release file.
    pub fn fetch(&self, version: &str, name: &str) -> Result<Vec<u8>, ReleaseError> {
        let limit = if name == SUMS_NAME || name == SIGNATURE_NAME {
            MAX_LISTING_BYTES
        } else {
            MAX_ARCHIVE_BYTES
        };
        let url = self.url(version, name);
        let reader: Box<dyn Read> = match url.strip_prefix("file://") {
            Some(path) => Box::new(
                std::fs::File::open(path)
                    .map_err(|error| ReleaseError::Download(format!("{url}: {error}")))?,
            ),
            None => {
                let response = ureq::AgentBuilder::new()
                    .timeout_connect(CONNECT_TIMEOUT)
                    .timeout_read(READ_TIMEOUT)
                    .build()
                    .get(&url)
                    .call()
                    .map_err(|error| ReleaseError::Download(format!("{url}: {error}")))?;
                Box::new(response.into_reader())
            }
        };
        let mut bytes = Vec::new();
        reader
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ReleaseError::Download(format!("{url}: {error}")))?;
        if bytes.len() as u64 > limit {
            return Err(ReleaseError::Archive(format!(
                "{name} is larger than {limit} bytes"
            )));
        }
        Ok(bytes)
    }
}

/// Checks the signature over `sums`, then the archive's SHA-256 against its
/// line in `sums`.
pub fn verify_release(
    public_key: &str,
    sums: &[u8],
    signature: &[u8],
    archive_name: &str,
    archive: &[u8],
) -> Result<(), ReleaseError> {
    let key = if public_key.contains('\n') {
        minisign_verify::PublicKey::decode(public_key)
    } else {
        minisign_verify::PublicKey::from_base64(public_key)
    }
    .map_err(|error| ReleaseError::Signature(format!("the public key is invalid: {error}")))?;
    let signature = std::str::from_utf8(signature)
        .map_err(|_| ReleaseError::Signature("the signature is not text".into()))
        .and_then(|text| {
            minisign_verify::Signature::decode(text)
                .map_err(|error| ReleaseError::Signature(format!("{error}")))
        })?;
    key.verify(sums, &signature, false)
        .map_err(|error| ReleaseError::Signature(format!("{error}")))?;
    let listed = std::str::from_utf8(sums)
        .map_err(|_| ReleaseError::Digest(format!("{SUMS_NAME} is not text")))?
        .lines()
        .find_map(|line| {
            let (digest, name) = line.split_once(char::is_whitespace)?;
            let name = name.trim_start().trim_start_matches('*');
            (name == archive_name).then(|| digest.to_ascii_lowercase())
        })
        .ok_or_else(|| ReleaseError::Digest(format!("{SUMS_NAME} does not list {archive_name}")))?;
    let actual = hex(&Sha256::digest(archive));
    if actual != listed {
        return Err(ReleaseError::Digest(format!(
            "{archive_name} has SHA-256 {actual}, not {listed}"
        )));
    }
    Ok(())
}

/// Unpacks a verified archive into `destination`, which must be empty. The
/// archive may hold only regular files in its root, and `muniment-server`
/// among them.
pub fn unpack(archive: &[u8], destination: &Path) -> Result<(), ReleaseError> {
    let invalid = |detail: String| ReleaseError::Archive(detail);
    std::fs::create_dir_all(destination)
        .map_err(|error| invalid(format!("{}: {error}", destination.display())))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let mut found_server = false;
    for entry in tar.entries().map_err(|error| invalid(error.to_string()))? {
        let mut entry = entry.map_err(|error| invalid(error.to_string()))?;
        let path = entry.path().map_err(|error| invalid(error.to_string()))?;
        let name = path
            .to_str()
            .map(|name| name.trim_start_matches("./"))
            .filter(|name| is_plain_name(name))
            .ok_or_else(|| invalid(format!("the entry {} is not a plain name", path.display())))?
            .to_owned();
        if !entry.header().entry_type().is_file() {
            return Err(invalid(format!("the entry {name} is not a regular file")));
        }
        let mode = entry.header().mode().unwrap_or(0o644) & 0o755;
        let target = destination.join(&name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| invalid(format!("{name}: {error}")))?;
        std::io::copy(&mut entry, &mut file)
            .map_err(|error| invalid(format!("{name}: {error}")))?;
        file.sync_all()
            .map_err(|error| invalid(format!("{name}: {error}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if name == SERVER_NAME { 0o755 } else { mode };
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))
                .map_err(|error| invalid(format!("{name}: {error}")))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        found_server |= name == SERVER_NAME;
    }
    if !found_server {
        return Err(invalid(format!("the archive holds no {SERVER_NAME}")));
    }
    Ok(())
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minisign key pair made for one test.
    pub(crate) struct TestKey {
        pair: minisign::KeyPair,
    }

    impl TestKey {
        pub(crate) fn new() -> Self {
            Self {
                pair: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
            }
        }

        /// The base64 public key line.
        pub(crate) fn public(&self) -> String {
            self.pair.pk.to_base64()
        }

        pub(crate) fn sign(&self, bytes: &[u8]) -> Vec<u8> {
            minisign::sign(
                None,
                &self.pair.sk,
                std::io::Cursor::new(bytes),
                Some("timestamp:0\tfile:SHA256SUMS"),
                None,
            )
            .unwrap()
            .to_string()
            .into_bytes()
        }
    }

    /// A gzipped tar of `files` in its root.
    pub(crate) fn archive(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, bytes, mode) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_entry_type(tar::EntryType::Regular);
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    pub(crate) fn sums(entries: &[(&str, &[u8])]) -> Vec<u8> {
        entries
            .iter()
            .map(|(name, bytes)| format!("{}  {name}\n", hex(&Sha256::digest(bytes))))
            .collect::<String>()
            .into_bytes()
    }

    fn temporary_directory(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "muniment-release-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    const NAME: &str = "muniment-server-2.0.0-linux-x86_64.tar.gz";

    fn release() -> (TestKey, Vec<u8>, Vec<u8>, Vec<u8>) {
        let key = TestKey::new();
        let archive = archive(&[(SERVER_NAME, b"#!/bin/sh\n", 0o755)]);
        let sums = sums(&[(NAME, &archive), ("other.tar.gz", b"other")]);
        let signature = key.sign(&sums);
        (key, archive, sums, signature)
    }

    #[test]
    fn a_signed_release_verifies() {
        let (key, archive, sums, signature) = release();
        assert_eq!(
            verify_release(&key.public(), &sums, &signature, NAME, &archive),
            Ok(())
        );
        let file = format!("untrusted comment: test key\n{}\n", key.public());
        assert_eq!(
            verify_release(&file, &sums, &signature, NAME, &archive),
            Ok(())
        );
    }

    #[test]
    fn a_tampered_archive_fails_its_digest() {
        let (key, mut archive, sums, signature) = release();
        archive.push(0);
        assert!(matches!(
            verify_release(&key.public(), &sums, &signature, NAME, &archive),
            Err(ReleaseError::Digest(_))
        ));
    }

    #[test]
    fn tampered_sums_fail_the_signature() {
        let (key, archive, sums, signature) = release();
        let mut forged = sums.clone();
        forged.extend_from_slice(b"00  extra.tar.gz\n");
        assert!(matches!(
            verify_release(&key.public(), &forged, &signature, NAME, &archive),
            Err(ReleaseError::Signature(_))
        ));
    }

    #[test]
    fn another_key_fails_the_signature() {
        let (_, archive, sums, signature) = release();
        let other = TestKey::new();
        assert!(matches!(
            verify_release(&other.public(), &sums, &signature, NAME, &archive),
            Err(ReleaseError::Signature(_))
        ));
        // A signature by another key fails against the release key.
        let (key, ..) = release();
        let forged = other.sign(&sums);
        assert!(matches!(
            verify_release(&key.public(), &sums, &forged, NAME, &archive),
            Err(ReleaseError::Signature(_))
        ));
    }

    #[test]
    fn a_missing_or_empty_signature_fails() {
        let (key, archive, sums, _) = release();
        for signature in [&b""[..], b"untrusted comment: x\n"] {
            assert!(matches!(
                verify_release(&key.public(), &sums, signature, NAME, &archive),
                Err(ReleaseError::Signature(_))
            ));
        }
    }

    #[test]
    fn sums_that_omit_the_archive_fail() {
        let (key, archive, ..) = release();
        let sums = sums(&[("other.tar.gz", b"other")]);
        let signature = key.sign(&sums);
        assert!(matches!(
            verify_release(&key.public(), &sums, &signature, NAME, &archive),
            Err(ReleaseError::Digest(_))
        ));
    }

    #[test]
    fn unpack_takes_flat_regular_files_only() {
        let directory = temporary_directory("unpack");
        let good = archive(&[
            (SERVER_NAME, b"server", 0o644),
            ("libonnxruntime.so.1", b"lib", 0o644),
        ]);
        unpack(&good, &directory).unwrap();
        assert_eq!(
            std::fs::read(directory.join(SERVER_NAME)).unwrap(),
            b"server"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(directory.join(SERVER_NAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        std::fs::remove_dir_all(&directory).unwrap();

        let no_server = archive(&[("libonnxruntime.so.1", b"lib", 0o644)]);
        assert!(matches!(
            unpack(&no_server, &directory),
            Err(ReleaseError::Archive(_))
        ));
        std::fs::remove_dir_all(&directory).unwrap();

        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.as_gnu_mut().unwrap().name[..10].copy_from_slice(b"../escaped");
        header.set_cksum();
        builder.append(&header, &b"x"[..]).unwrap();
        let escaping = builder.into_inner().unwrap().finish().unwrap();
        assert!(matches!(
            unpack(&escaping, &directory),
            Err(ReleaseError::Archive(_))
        ));
        assert!(!directory.parent().unwrap().join("escaped").exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn a_file_feed_reads_a_local_directory() {
        let directory = temporary_directory("feed");
        std::fs::create_dir_all(directory.join("v2.0.0")).unwrap();
        std::fs::write(directory.join("v2.0.0").join(SUMS_NAME), b"sums").unwrap();
        let feed = Feed::new(&format!("file://{}/", directory.display()));
        assert_eq!(feed.fetch("2.0.0", SUMS_NAME).unwrap(), b"sums");
        assert!(feed.fetch("2.0.1", SUMS_NAME).unwrap_err().is_transient());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn archive_names_follow_the_release_layout() {
        assert_eq!(
            archive_name("1.15.0", "aarch64"),
            "muniment-server-1.15.0-linux-aarch64.tar.gz"
        );
        assert_eq!(
            Feed::new(DEFAULT_FEED).url("1.15.0", SUMS_NAME),
            "https://github.com/munimentai/muniment-core/releases/download/v1.15.0/SHA256SUMS"
        );
    }
}
