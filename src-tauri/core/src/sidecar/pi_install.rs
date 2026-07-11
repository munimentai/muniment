//! Pinned Pi archive acquisition, safe extraction, and current/previous publication.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::model_install::{
    install_model, AvailableSpace, InstallCancellation, InstallLock, ModelInstallError,
};

pub const PI_RELEASE_BASE: &str = "https://github.com/earendil-works/pi/releases/download/v0.73.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PiArtifactDescriptor {
    pub version: &'static str,
    pub archive: &'static str,
    pub byte_size: u64,
    pub sha256: &'static str,
    pub executable: &'static str,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const PI_ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    version: "0.73.1",
    archive: "pi-linux-x64.tar.gz",
    byte_size: 45_540_364,
    sha256: "00f0db9e93f6ba33deb1bb4d75b4eafede9fa5379b635a908cf967d5b37e366d",
    executable: "pi",
};
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub const PI_ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    version: "0.73.1",
    archive: "pi-linux-arm64.tar.gz",
    byte_size: 44_095_594,
    sha256: "f47455b6a7ff6e43752a37c7c0a08b8054efd82cae1efc06d007c94f06a56318",
    executable: "pi",
};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const PI_ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    version: "0.73.1",
    archive: "pi-darwin-arm64.tar.gz",
    byte_size: 28_567_469,
    sha256: "c64f501cad8fa0a581257dc9e878e1b2f351f295d0d85d573fe8d7967bfb1bee",
    executable: "pi",
};
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub const PI_ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    version: "0.73.1",
    archive: "pi-darwin-x64.tar.gz",
    byte_size: 31_000_715,
    sha256: "e59fded1f79fbc7b12e263bf43d1e358af598f6fb3c4d4f58e16d1c5ebe6b2b5",
    executable: "pi",
};
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const PI_ARTIFACT: PiArtifactDescriptor = PiArtifactDescriptor {
    version: "0.73.1",
    archive: "pi-windows-x64.zip",
    byte_size: 48_225_045,
    sha256: "8bdb8e612a4b820f939a524652709b167ac5f1d4d1bba25988a631bff0bbe80b",
    executable: "pi.exe",
};

#[derive(Debug, Clone)]
pub struct PiDownloadRequest {
    url: String,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub deadline: Duration,
}
impl PiDownloadRequest {
    pub fn url(&self) -> &str {
        &self.url
    }
}
pub struct PiDownloadResponse<R> {
    pub status: u16,
    pub body: R,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiTransportError {
    Transient,
    Unavailable,
    Rejected,
}
pub trait PiDownloadTransport {
    type Body: Read;
    fn download(
        &mut self,
        request: &PiDownloadRequest,
    ) -> Result<PiDownloadResponse<Self::Body>, PiTransportError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiInstallError {
    InvalidStage,
    Download,
    WrongSize,
    DigestMismatch,
    UnsafeArchive,
    Persistence,
    NotInstalled,
}
impl std::fmt::Display for PiInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pi installation failed: {:?}", self)
    }
}
impl std::error::Error for PiInstallError {}
pub type CoordinatedPiInstallError = ModelInstallError<PiInstallError, PiInstallError>;

pub fn install_pi<
    T: PiDownloadTransport,
    C: InstallCancellation,
    L: InstallLock,
    S: AvailableSpace,
>(
    root: &Path,
    install_id: &str,
    transport: &mut T,
    cancellation: &C,
    lock: &mut L,
    space: &mut S,
) -> Result<PathBuf, CoordinatedPiInstallError> {
    if !safe_component(install_id) {
        return Err(ModelInstallError::Acquisition(PiInstallError::InvalidStage));
    }
    let stage = root.join("staging").join(install_id);
    install_model(
        lock,
        space,
        cancellation,
        || Ok(PI_ARTIFACT.byte_size),
        || acquire_stage(&stage, transport),
        |stage| publish_stage(root, &stage),
    )
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
}

fn acquire_stage<T: PiDownloadTransport>(
    stage: &Path,
    transport: &mut T,
) -> Result<PathBuf, PiInstallError> {
    if stage.file_name().is_none() || stage.exists() {
        return Err(PiInstallError::InvalidStage);
    }
    fs::create_dir_all(stage).map_err(|_| PiInstallError::Persistence)?;
    let archive = stage.join(PI_ARTIFACT.archive);
    let request = PiDownloadRequest {
        url: format!("{PI_RELEASE_BASE}/{}", PI_ARTIFACT.archive),
        connect_timeout: Duration::from_secs(10),
        read_timeout: Duration::from_secs(30),
        deadline: Duration::from_secs(30 * 60),
    };
    let response = transport
        .download(&request)
        .map_err(|_| PiInstallError::Download)?;
    if response.status != 200 {
        return Err(PiInstallError::Download);
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive)
        .map_err(|_| PiInstallError::Persistence)?;
    let mut limited = response.body.take(PI_ARTIFACT.byte_size + 1);
    std::io::copy(&mut limited, &mut output).map_err(|_| PiInstallError::Download)?;
    output.sync_all().map_err(|_| PiInstallError::Persistence)?;
    verify_archive(&archive)?;
    extract_archive(&archive, stage)?;
    verify_executable(stage)?;
    Ok(stage.to_owned())
}

pub fn verify_archive(path: &Path) -> Result<(), PiInstallError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PiInstallError::Persistence)?;
    if !metadata.file_type().is_file() || metadata.len() != PI_ARTIFACT.byte_size {
        return Err(PiInstallError::WrongSize);
    }
    let mut file = File::open(path).map_err(|_| PiInstallError::Persistence)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).map_err(|_| PiInstallError::Persistence)?;
    if format!("{:x}", hash.finalize()) != PI_ARTIFACT.sha256 {
        return Err(PiInstallError::DigestMismatch);
    }
    Ok(())
}

fn safe_name(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
        && path.components().count() == 1
}
fn extract_archive(archive: &Path, stage: &Path) -> Result<(), PiInstallError> {
    #[cfg(windows)]
    {
        let mut zip =
            zip::ZipArchive::new(File::open(archive).map_err(|_| PiInstallError::Persistence)?)
                .map_err(|_| PiInstallError::UnsafeArchive)?;
        if zip.len() != 1 {
            return Err(PiInstallError::UnsafeArchive);
        }
        let mut entry = zip.by_index(0).map_err(|_| PiInstallError::UnsafeArchive)?;
        let name = Path::new(entry.name());
        if !safe_name(name) || name != Path::new(PI_ARTIFACT.executable) || entry.is_dir() {
            return Err(PiInstallError::UnsafeArchive);
        }
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.join(PI_ARTIFACT.executable))
            .map_err(|_| PiInstallError::Persistence)?;
        std::io::copy(&mut entry, &mut out).map_err(|_| PiInstallError::Persistence)?;
    }
    #[cfg(not(windows))]
    {
        let decoder = flate2::read::GzDecoder::new(
            File::open(archive).map_err(|_| PiInstallError::Persistence)?,
        );
        let mut tar = tar::Archive::new(decoder);
        let mut count = 0;
        for item in tar.entries().map_err(|_| PiInstallError::UnsafeArchive)? {
            let mut entry = item.map_err(|_| PiInstallError::UnsafeArchive)?;
            let path = entry.path().map_err(|_| PiInstallError::UnsafeArchive)?;
            if !safe_name(&path)
                || path.as_ref() != Path::new(PI_ARTIFACT.executable)
                || !entry.header().entry_type().is_file()
            {
                return Err(PiInstallError::UnsafeArchive);
            }
            count += 1;
            if count > 1 {
                return Err(PiInstallError::UnsafeArchive);
            }
            entry
                .unpack(stage.join(PI_ARTIFACT.executable))
                .map_err(|_| PiInstallError::Persistence)?;
        }
        if count != 1 {
            return Err(PiInstallError::UnsafeArchive);
        }
    }
    Ok(())
}

fn verify_executable(root: &Path) -> Result<PathBuf, PiInstallError> {
    let path = root.join(PI_ARTIFACT.executable);
    let metadata = fs::symlink_metadata(&path).map_err(|_| PiInstallError::Persistence)?;
    if !metadata.file_type().is_file() {
        return Err(PiInstallError::UnsafeArchive);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).map_err(|_| PiInstallError::Persistence)?;
    }
    Ok(path)
}

fn publish_stage(root: &Path, stage: &Path) -> Result<PathBuf, PiInstallError> {
    verify_archive(&stage.join(PI_ARTIFACT.archive))?;
    verify_executable(stage)?;
    let revisions = root.join("revisions");
    fs::create_dir_all(&revisions).map_err(|_| PiInstallError::Persistence)?;
    let destination = revisions.join(PI_ARTIFACT.version);
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|_| PiInstallError::Persistence)?;
    }
    fs::rename(stage, &destination).map_err(|_| PiInstallError::Persistence)?;
    if let Ok(current) = fs::read_to_string(root.join("current")) {
        fs::write(root.join("previous"), current).map_err(|_| PiInstallError::Persistence)?;
    }
    fs::write(root.join("current"), format!("{}\n", PI_ARTIFACT.version))
        .map_err(|_| PiInstallError::Persistence)?;
    resolve_current(root)
}

pub fn resolve_current(root: &Path) -> Result<PathBuf, PiInstallError> {
    resolve_pointer(root, "current").or_else(|_| resolve_pointer(root, "previous"))
}
fn resolve_pointer(root: &Path, pointer: &str) -> Result<PathBuf, PiInstallError> {
    let version =
        fs::read_to_string(root.join(pointer)).map_err(|_| PiInstallError::NotInstalled)?;
    if version.trim() != PI_ARTIFACT.version {
        return Err(PiInstallError::NotInstalled);
    }
    let revision = root.join("revisions").join(version.trim());
    verify_archive(&revision.join(PI_ARTIFACT.archive))?;
    verify_executable(&revision)
}
