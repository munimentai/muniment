//! Pinned llama.cpp descriptors and manifest-verified extraction core.
//! Download and publication lifecycle intentionally live outside this module.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const LLAMA_SERVER_RELEASE: &str = "b10068";
pub const LLAMA_SERVER_RELEASE_BASE: &str =
    "https://github.com/ggml-org/llama.cpp/releases/download/b10068";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlamaRuntimeDescriptor {
    pub revision: &'static str,
    pub archive: &'static str,
    pub byte_size: u64,
    pub sha256: &'static str,
    pub top_level: &'static str,
    pub executable: &'static str,
}

macro_rules! descriptor {
    ($name:ident, $archive:literal, $size:literal, $hash:literal, $exe:literal) => {
        pub const $name: LlamaRuntimeDescriptor = LlamaRuntimeDescriptor {
            revision: LLAMA_SERVER_RELEASE,
            archive: $archive,
            byte_size: $size,
            sha256: $hash,
            top_level: "llama-b10068",
            executable: $exe,
        };
    };
}
descriptor!(
    LLAMA_SERVER_LINUX_X64,
    "llama-b10068-bin-ubuntu-x64.tar.gz",
    16_066_558,
    "6bf3d20de562e4df230f1a7c54fb7a06a80c7ff40f5311c953e8255744be4eb2",
    "llama-b10068/llama-server"
);
descriptor!(
    LLAMA_SERVER_WINDOWS_X64,
    "llama-b10068-bin-win-cpu-x64.zip",
    18_007_324,
    "01d5f30876acfb4a0be59396710f450213495c7181d8fbcce2fad045835ceb89",
    "llama-b10068/llama-server.exe"
);
descriptor!(
    LLAMA_SERVER_MACOS_ARM64,
    "llama-b10068-bin-macos-arm64.tar.gz",
    10_603_591,
    "13aa2d40c76ad1dcb8ebeec5f0d2814bf3b2f84a66935c7d4dc6f7cca8e38d68",
    "llama-b10068/llama-server"
);
descriptor!(
    LLAMA_SERVER_MACOS_X64,
    "llama-b10068-bin-macos-x64.tar.gz",
    10_876_051,
    "73a63a0fdcfd8d0625fe20aa8f2af62e3d6437c6380b46129ca1a9abacbde0d5",
    "llama-b10068/llama-server"
);

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const LLAMA_SERVER_ARTIFACT: LlamaRuntimeDescriptor = LLAMA_SERVER_LINUX_X64;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const LLAMA_SERVER_ARTIFACT: LlamaRuntimeDescriptor = LLAMA_SERVER_WINDOWS_X64;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const LLAMA_SERVER_ARTIFACT: LlamaRuntimeDescriptor = LLAMA_SERVER_MACOS_ARM64;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub const LLAMA_SERVER_ARTIFACT: LlamaRuntimeDescriptor = LLAMA_SERVER_MACOS_X64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestEntry {
    Directory,
    File { byte_size: u64, sha256: String },
    Symlink { target: PathBuf },
}
pub type RuntimeManifest = BTreeMap<PathBuf, ManifestEntry>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeArchiveError {
    InvalidDescriptor,
    Io,
    WrongSize,
    DigestMismatch,
    UnsupportedArchive,
    UnsafeArchive,
    ManifestMismatch,
    InvalidExecutable,
}
impl std::fmt::Display for RuntimeArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "llama-server runtime verification failed: {self:?}")
    }
}
impl std::error::Error for RuntimeArchiveError {}

#[derive(Debug)]
struct Entry {
    path: PathBuf,
    kind: Kind,
    mode: Option<u32>,
}
#[derive(Debug)]
enum Kind {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

pub fn verify_archive(
    path: &Path,
    descriptor: &LlamaRuntimeDescriptor,
) -> Result<(), RuntimeArchiveError> {
    validate_descriptor(descriptor)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| RuntimeArchiveError::Io)?;
    if !metadata.file_type().is_file() || metadata.len() != descriptor.byte_size {
        return Err(RuntimeArchiveError::WrongSize);
    }
    let mut digest = Sha256::new();
    std::io::copy(
        &mut File::open(path).map_err(|_| RuntimeArchiveError::Io)?,
        &mut digest,
    )
    .map_err(|_| RuntimeArchiveError::Io)?;
    if format!("{:x}", digest.finalize()) != descriptor.sha256 {
        return Err(RuntimeArchiveError::DigestMismatch);
    }
    Ok(())
}

/// Verifies archive identity before parsing it, derives the only admitted
/// manifest, and extracts to a path which must not already exist.
pub fn extract_verified_archive(
    archive: &Path,
    destination: &Path,
    descriptor: &LlamaRuntimeDescriptor,
) -> Result<RuntimeManifest, RuntimeArchiveError> {
    verify_archive(archive, descriptor)?;
    if fs::symlink_metadata(destination).is_ok() {
        return Err(RuntimeArchiveError::UnsafeArchive);
    }
    let entries = read_archive(archive, descriptor)?;
    let manifest = manifest_for(&entries, descriptor)?;
    fs::create_dir(destination).map_err(|_| RuntimeArchiveError::Io)?;
    if let Err(error) = materialize(destination, &entries, &manifest)
        .and_then(|_| verify_extracted_tree(destination, &manifest, descriptor))
    {
        let _ = fs::remove_dir_all(destination);
        return Err(error);
    }
    Ok(manifest)
}

/// Rechecks every entry without following links and rejects missing or extra
/// entries, changed bytes or targets, special files, and unsafe link chains.
pub fn verify_extracted_tree(
    root: &Path,
    manifest: &RuntimeManifest,
    descriptor: &LlamaRuntimeDescriptor,
) -> Result<(), RuntimeArchiveError> {
    validate_descriptor(descriptor)?;
    validate_links(manifest, descriptor)?;
    if !fs::symlink_metadata(root)
        .map_err(|_| RuntimeArchiveError::ManifestMismatch)?
        .file_type()
        .is_dir()
    {
        return Err(RuntimeArchiveError::ManifestMismatch);
    }
    let mut actual = RuntimeManifest::new();
    walk(root, Path::new(""), &mut actual)?;
    if actual != *manifest {
        return Err(RuntimeArchiveError::ManifestMismatch);
    }
    let metadata = fs::symlink_metadata(root.join(descriptor.executable))
        .map_err(|_| RuntimeArchiveError::InvalidExecutable)?;
    if !metadata.file_type().is_file() {
        return Err(RuntimeArchiveError::InvalidExecutable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(RuntimeArchiveError::InvalidExecutable);
        }
    }
    Ok(())
}

fn validate_descriptor(descriptor: &LlamaRuntimeDescriptor) -> Result<(), RuntimeArchiveError> {
    let component = |value: &str| {
        !value.is_empty()
            && value != "."
            && value != ".."
            && !value.contains('/')
            && !value.contains('\\')
    };
    if !component(descriptor.revision)
        || !component(descriptor.archive)
        || !component(descriptor.top_level)
        || descriptor.byte_size == 0
        || descriptor.sha256.len() != 64
        || !descriptor
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !safe_path(Path::new(descriptor.executable), descriptor.top_level)
    {
        return Err(RuntimeArchiveError::InvalidDescriptor);
    }
    Ok(())
}

fn read_archive(
    path: &Path,
    descriptor: &LlamaRuntimeDescriptor,
) -> Result<Vec<Entry>, RuntimeArchiveError> {
    if descriptor.archive.ends_with(".tar.gz") {
        read_tar(path)
    } else if descriptor.archive.ends_with(".zip") {
        read_zip(path)
    } else {
        Err(RuntimeArchiveError::UnsupportedArchive)
    }
}

fn read_tar(path: &Path) -> Result<Vec<Entry>, RuntimeArchiveError> {
    let decoder =
        flate2::read::GzDecoder::new(File::open(path).map_err(|_| RuntimeArchiveError::Io)?);
    let mut archive = tar::Archive::new(decoder);
    let mut entries = Vec::new();
    for item in archive
        .entries()
        .map_err(|_| RuntimeArchiveError::UnsafeArchive)?
    {
        let mut item = item.map_err(|_| RuntimeArchiveError::UnsafeArchive)?;
        let path = item
            .path()
            .map_err(|_| RuntimeArchiveError::UnsafeArchive)?
            .into_owned();
        let mode = item.header().mode().ok();
        let kind = if item.header().entry_type().is_dir() {
            Kind::Directory
        } else if item.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            item.read_to_end(&mut bytes)
                .map_err(|_| RuntimeArchiveError::UnsafeArchive)?;
            Kind::File(bytes)
        } else if item.header().entry_type().is_symlink() {
            Kind::Symlink(
                item.link_name()
                    .map_err(|_| RuntimeArchiveError::UnsafeArchive)?
                    .ok_or(RuntimeArchiveError::UnsafeArchive)?
                    .into_owned(),
            )
        } else {
            return Err(RuntimeArchiveError::UnsafeArchive);
        };
        entries.push(Entry { path, kind, mode });
    }
    Ok(entries)
}

fn read_zip(path: &Path) -> Result<Vec<Entry>, RuntimeArchiveError> {
    let mut archive = zip::ZipArchive::new(File::open(path).map_err(|_| RuntimeArchiveError::Io)?)
        .map_err(|_| RuntimeArchiveError::UnsafeArchive)?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut item = archive
            .by_index(index)
            .map_err(|_| RuntimeArchiveError::UnsafeArchive)?;
        let path = item
            .enclosed_name()
            .ok_or(RuntimeArchiveError::UnsafeArchive)?;
        let mode = item.unix_mode();
        let file_type = mode.unwrap_or(0) & 0o170000;
        let kind = if item.is_dir() {
            Kind::Directory
        } else if file_type == 0o120000 {
            let mut bytes = Vec::new();
            item.read_to_end(&mut bytes)
                .map_err(|_| RuntimeArchiveError::UnsafeArchive)?;
            Kind::Symlink(PathBuf::from(
                std::str::from_utf8(&bytes).map_err(|_| RuntimeArchiveError::UnsafeArchive)?,
            ))
        } else if item.is_file() && (file_type == 0 || file_type == 0o100000) {
            let mut bytes = Vec::new();
            item.read_to_end(&mut bytes)
                .map_err(|_| RuntimeArchiveError::UnsafeArchive)?;
            Kind::File(bytes)
        } else {
            return Err(RuntimeArchiveError::UnsafeArchive);
        };
        entries.push(Entry { path, kind, mode });
    }
    Ok(entries)
}

fn safe_path(path: &Path, top: &str) -> bool {
    !path.as_os_str().is_empty()
        && !path.to_string_lossy().contains('\\')
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && path.components().next() == Some(Component::Normal(top.as_ref()))
}

fn manifest_for(
    entries: &[Entry],
    descriptor: &LlamaRuntimeDescriptor,
) -> Result<RuntimeManifest, RuntimeArchiveError> {
    let mut result = RuntimeManifest::new();
    let mut folded = BTreeSet::new();
    for entry in entries {
        if !safe_path(&entry.path, descriptor.top_level)
            || !folded.insert(entry.path.to_string_lossy().to_lowercase())
        {
            return Err(RuntimeArchiveError::UnsafeArchive);
        }
        for parent in entry
            .path
            .ancestors()
            .skip(1)
            .filter(|path| !path.as_os_str().is_empty())
        {
            match result.entry(parent.to_owned()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(ManifestEntry::Directory);
                }
                std::collections::btree_map::Entry::Occupied(slot)
                    if !matches!(slot.get(), ManifestEntry::Directory) =>
                {
                    return Err(RuntimeArchiveError::UnsafeArchive)
                }
                _ => {}
            }
        }
        let value = match &entry.kind {
            Kind::Directory => ManifestEntry::Directory,
            Kind::File(bytes) => ManifestEntry::File {
                byte_size: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            },
            Kind::Symlink(target) => ManifestEntry::Symlink {
                target: target.clone(),
            },
        };
        match result.insert(entry.path.clone(), value.clone()) {
            None => {}
            Some(ManifestEntry::Directory) if value == ManifestEntry::Directory => {}
            Some(_) => return Err(RuntimeArchiveError::UnsafeArchive),
        }
    }
    validate_links(&result, descriptor)?;
    if !matches!(
        result.get(Path::new(descriptor.executable)),
        Some(ManifestEntry::File { .. })
    ) {
        return Err(RuntimeArchiveError::InvalidExecutable);
    }
    Ok(result)
}

fn resolve(base: &Path, target: &Path, top: &str) -> Option<PathBuf> {
    if target.is_absolute()
        || target.as_os_str().is_empty()
        || target.to_string_lossy().contains('\\')
    {
        return None;
    }
    let mut parts: Vec<_> = base
        .components()
        .filter_map(|part| {
            if let Component::Normal(value) = part {
                Some(value.to_owned())
            } else {
                None
            }
        })
        .collect();
    for part in target.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Normal(value) => parts.push(value.to_owned()),
            _ => return None,
        }
    }
    let path: PathBuf = parts.into_iter().collect();
    (path.components().next() == Some(Component::Normal(top.as_ref()))).then_some(path)
}

fn validate_links(
    manifest: &RuntimeManifest,
    descriptor: &LlamaRuntimeDescriptor,
) -> Result<(), RuntimeArchiveError> {
    for (path, entry) in manifest {
        if let ManifestEntry::Symlink { target } = entry {
            let mut current = resolve(
                path.parent().unwrap_or(Path::new("")),
                target,
                descriptor.top_level,
            )
            .ok_or(RuntimeArchiveError::UnsafeArchive)?;
            let mut visited = BTreeSet::from([path.clone()]);
            loop {
                if !visited.insert(current.clone()) {
                    return Err(RuntimeArchiveError::UnsafeArchive);
                }
                match manifest.get(&current) {
                    Some(ManifestEntry::File { .. }) => break,
                    Some(ManifestEntry::Symlink { target }) => {
                        current = resolve(
                            current.parent().unwrap_or(Path::new("")),
                            target,
                            descriptor.top_level,
                        )
                        .ok_or(RuntimeArchiveError::UnsafeArchive)?
                    }
                    _ => return Err(RuntimeArchiveError::UnsafeArchive),
                }
            }
        }
    }
    Ok(())
}

fn materialize(
    root: &Path,
    entries: &[Entry],
    manifest: &RuntimeManifest,
) -> Result<(), RuntimeArchiveError> {
    for (path, entry) in manifest {
        if matches!(entry, ManifestEntry::Directory) {
            fs::create_dir_all(root.join(path)).map_err(|_| RuntimeArchiveError::Io)?;
        }
    }
    for entry in entries {
        if let Kind::File(bytes) = &entry.kind {
            let path = root.join(&entry.path);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| RuntimeArchiveError::Io)?;
            file.write_all(bytes).map_err(|_| RuntimeArchiveError::Io)?;
            #[cfg(unix)]
            if let Some(mode) = entry.mode {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777))
                    .map_err(|_| RuntimeArchiveError::Io)?;
            }
        }
    }
    for entry in entries {
        if let Kind::Symlink(target) = &entry.kind {
            create_symlink(target, &root.join(&entry.path))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, path: &Path) -> Result<(), RuntimeArchiveError> {
    std::os::unix::fs::symlink(target, path).map_err(|_| RuntimeArchiveError::Io)
}
#[cfg(windows)]
fn create_symlink(target: &Path, path: &Path) -> Result<(), RuntimeArchiveError> {
    std::os::windows::fs::symlink_file(target, path).map_err(|_| RuntimeArchiveError::Io)
}

fn walk(
    root: &Path,
    relative: &Path,
    result: &mut RuntimeManifest,
) -> Result<(), RuntimeArchiveError> {
    for item in
        fs::read_dir(root.join(relative)).map_err(|_| RuntimeArchiveError::ManifestMismatch)?
    {
        let item = item.map_err(|_| RuntimeArchiveError::ManifestMismatch)?;
        let path = relative.join(item.file_name());
        let metadata =
            fs::symlink_metadata(item.path()).map_err(|_| RuntimeArchiveError::ManifestMismatch)?;
        let value = if metadata.file_type().is_dir() {
            ManifestEntry::Directory
        } else if metadata.file_type().is_file() {
            let mut digest = Sha256::new();
            let byte_size = std::io::copy(
                &mut File::open(item.path()).map_err(|_| RuntimeArchiveError::ManifestMismatch)?,
                &mut digest,
            )
            .map_err(|_| RuntimeArchiveError::ManifestMismatch)?;
            ManifestEntry::File {
                byte_size,
                sha256: format!("{:x}", digest.finalize()),
            }
        } else if metadata.file_type().is_symlink() {
            ManifestEntry::Symlink {
                target: fs::read_link(item.path())
                    .map_err(|_| RuntimeArchiveError::ManifestMismatch)?,
            }
        } else {
            return Err(RuntimeArchiveError::ManifestMismatch);
        };
        let descend = matches!(value, ManifestEntry::Directory);
        result.insert(path.clone(), value);
        if descend {
            walk(root, &path, result)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "muniment-llama-runtime-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    enum Fixture<'a> {
        File(&'a str, &'a [u8], u32),
        Directory(&'a str),
        Link(&'a str, &'a str),
    }

    fn archive(root: &Path, entries: &[Fixture<'_>]) -> (PathBuf, LlamaRuntimeDescriptor) {
        let path = root.join("fixture.tar.gz");
        let output = File::create(&path).unwrap();
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            output,
            flate2::Compression::default(),
        ));
        for entry in entries {
            let mut header = tar::Header::new_gnu();
            match entry {
                Fixture::File(path, bytes, mode) => {
                    header.set_size(bytes.len() as u64);
                    header.set_mode(*mode);
                    header.set_cksum();
                    tar.append_data(&mut header, path, *bytes).unwrap();
                }
                Fixture::Directory(path) => {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                    header.set_mode(0o755);
                    header.set_cksum();
                    tar.append_data(&mut header, path, std::io::empty())
                        .unwrap();
                }
                Fixture::Link(path, target) => {
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    header.set_mode(0o777);
                    header.set_link_name(target).unwrap();
                    header.set_cksum();
                    tar.append_data(&mut header, path, std::io::empty())
                        .unwrap();
                }
            }
        }
        let encoder = tar.into_inner().unwrap();
        encoder.finish().unwrap();
        let bytes = fs::read(&path).unwrap();
        let hash = Box::leak(format!("{:x}", Sha256::digest(&bytes)).into_boxed_str());
        (
            path,
            LlamaRuntimeDescriptor {
                revision: "fixture",
                archive: "fixture.tar.gz",
                byte_size: bytes.len() as u64,
                sha256: hash,
                top_level: "llama-b10068",
                executable: "llama-b10068/llama-server",
            },
        )
    }

    fn zip_archive(root: &Path) -> (PathBuf, LlamaRuntimeDescriptor) {
        let path = root.join("fixture.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        zip.add_directory("llama-b10068/", options).unwrap();
        zip.start_file("llama-b10068/llama-server.exe", options)
            .unwrap();
        zip.write_all(b"server").unwrap();
        zip.finish().unwrap();
        let bytes = fs::read(&path).unwrap();
        let hash = Box::leak(format!("{:x}", Sha256::digest(&bytes)).into_boxed_str());
        (
            path,
            LlamaRuntimeDescriptor {
                revision: "fixture",
                archive: "fixture.zip",
                byte_size: bytes.len() as u64,
                sha256: hash,
                top_level: "llama-b10068",
                executable: "llama-b10068/llama-server.exe",
            },
        )
    }

    #[test]
    fn descriptors_match_adr_0014_and_native_selection() {
        assert_eq!(LLAMA_SERVER_LINUX_X64.byte_size, 16_066_558);
        assert_eq!(
            LLAMA_SERVER_WINDOWS_X64.executable,
            "llama-b10068/llama-server.exe"
        );
        assert_eq!(LLAMA_SERVER_MACOS_ARM64.revision, "b10068");
        assert_eq!(LLAMA_SERVER_MACOS_X64.top_level, "llama-b10068");
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(LLAMA_SERVER_ARTIFACT, LLAMA_SERVER_LINUX_X64);
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        assert_eq!(LLAMA_SERVER_ARTIFACT, LLAMA_SERVER_WINDOWS_X64);
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        assert_eq!(LLAMA_SERVER_ARTIFACT, LLAMA_SERVER_MACOS_ARM64);
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        assert_eq!(LLAMA_SERVER_ARTIFACT, LLAMA_SERVER_MACOS_X64);
    }

    #[test]
    fn extracts_zip_using_the_same_manifest_contract() {
        let temp = Temp::new("zip");
        let (archive, descriptor) = zip_archive(&temp.0);
        let tree = temp.0.join("tree");
        let manifest = extract_verified_archive(&archive, &tree, &descriptor).unwrap();
        assert!(matches!(
            manifest.get(Path::new("llama-b10068/llama-server.exe")),
            Some(ManifestEntry::File { byte_size: 6, .. })
        ));
        verify_extracted_tree(&tree, &manifest, &descriptor).unwrap();
    }

    #[test]
    fn extracts_files_then_links_and_reverification_detects_changes() {
        let temp = Temp::new("valid");
        let (archive, descriptor) = archive(
            &temp.0,
            &[
                Fixture::Directory("llama-b10068"),
                Fixture::File("llama-b10068/llama-server", b"server", 0o755),
                Fixture::File("llama-b10068/lib.so.1", b"library", 0o644),
                Fixture::Link("llama-b10068/lib.so", "lib.so.1"),
            ],
        );
        let tree = temp.0.join("tree");
        let manifest = extract_verified_archive(&archive, &tree, &descriptor).unwrap();
        assert_eq!(
            fs::read_link(tree.join("llama-b10068/lib.so")).unwrap(),
            Path::new("lib.so.1")
        );
        verify_extracted_tree(&tree, &manifest, &descriptor).unwrap();
        fs::write(tree.join("llama-b10068/lib.so.1"), b"changed").unwrap();
        assert_eq!(
            verify_extracted_tree(&tree, &manifest, &descriptor),
            Err(RuntimeArchiveError::ManifestMismatch)
        );
    }

    #[test]
    fn rejects_bad_identity_and_unsafe_or_incomplete_manifests() {
        let temp = Temp::new("unsafe");
        let (valid, descriptor) = archive(
            &temp.0,
            &[Fixture::File("llama-b10068/llama-server", b"server", 0o755)],
        );
        let mut wrong_size = descriptor;
        wrong_size.byte_size += 1;
        assert_eq!(
            verify_archive(&valid, &wrong_size),
            Err(RuntimeArchiveError::WrongSize)
        );
        let mut wrong_hash = descriptor;
        wrong_hash.sha256 = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            verify_archive(&valid, &wrong_hash),
            Err(RuntimeArchiveError::DigestMismatch)
        );
        let mut invalid = descriptor;
        invalid.executable = "../llama-server";
        assert_eq!(
            verify_archive(&valid, &invalid),
            Err(RuntimeArchiveError::InvalidDescriptor)
        );

        for (name, entries) in [
            (
                "dangling",
                vec![
                    Fixture::File("llama-b10068/llama-server", b"server", 0o755),
                    Fixture::Link("llama-b10068/lib.so", "missing"),
                ],
            ),
            (
                "escaping",
                vec![
                    Fixture::File("llama-b10068/llama-server", b"server", 0o755),
                    Fixture::Link("llama-b10068/lib.so", "../../outside"),
                ],
            ),
            (
                "case",
                vec![
                    Fixture::File("llama-b10068/llama-server", b"server", 0o755),
                    Fixture::File("llama-b10068/LLAMA-SERVER", b"other", 0o755),
                ],
            ),
        ] {
            let nested = Temp::new(name);
            let (path, descriptor) = archive(&nested.0, &entries);
            assert_eq!(
                extract_verified_archive(&path, &nested.0.join("tree"), &descriptor),
                Err(RuntimeArchiveError::UnsafeArchive),
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_non_executable_server_and_extra_tree_entries() {
        let temp = Temp::new("executable");
        let (path, descriptor) = archive(
            &temp.0,
            &[Fixture::File("llama-b10068/llama-server", b"server", 0o644)],
        );
        assert_eq!(
            extract_verified_archive(&path, &temp.0.join("tree"), &descriptor),
            Err(RuntimeArchiveError::InvalidExecutable)
        );

        let (path, descriptor) = archive(
            &temp.0,
            &[Fixture::File("llama-b10068/llama-server", b"server", 0o755)],
        );
        let tree = temp.0.join("tree-two");
        let manifest = extract_verified_archive(&path, &tree, &descriptor).unwrap();
        fs::write(tree.join("llama-b10068/extra"), b"extra").unwrap();
        assert_eq!(
            verify_extracted_tree(&tree, &manifest, &descriptor),
            Err(RuntimeArchiveError::ManifestMismatch)
        );
    }
}
