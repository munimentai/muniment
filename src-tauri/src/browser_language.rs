// The browser tab's interface language. The installer carries the most used
// Chromium language packs, and each release publishes the rest in one archive.
// The installer's signed manifest pins that archive's size and SHA-256, so a
// download that differs is refused before any pack is installed.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

const MANIFEST: &str = "chromium-locales.json";
const SETTINGS: &str = "language.json";
const PACKS: &str = "locales";
const RELEASES: &str = "https://github.com/munimentai/muniment/releases/download";
#[cfg(target_os = "macos")]
const PLATFORM: &str = "macos";
#[cfg(windows)]
const PLATFORM: &str = "windows";
#[cfg(target_os = "linux")]
const PLATFORM: &str = "linux";

static ACTIVE: OnceLock<Option<String>> = OnceLock::new();

#[derive(Deserialize)]
struct Manifest {
    sha256: String,
    size: u64,
    bundled: Vec<String>,
    available: Vec<String>,
}

#[derive(Serialize)]
pub struct Language {
    code: String,
    state: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageView {
    selected: Option<String>,
    active: Option<String>,
    languages: Vec<Language>,
    download_size: u64,
}

fn executable_directory() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn manifest_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return Some(executable_directory()?.join("../Resources").join(MANIFEST));
    #[cfg(windows)]
    return Some(executable_directory()?.join(MANIFEST));
    #[cfg(target_os = "linux")]
    return Some(crate::cef_native::cef_directory().ok()?.join(MANIFEST));
}

/// The pack the installer ships for a Chromium locale code such as `pt-BR`.
fn bundled_pack(code: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    return Some(executable_directory()?.join(format!(
        "../Frameworks/Chromium Embedded Framework.framework/Resources/{}/locale.pak",
        macos_folder(code)
    )));
    #[cfg(windows)]
    return Some(
        executable_directory()?
            .join("locales")
            .join(format!("{code}.pak")),
    );
    #[cfg(target_os = "linux")]
    return Some(
        crate::cef_native::cef_directory()
            .ok()?
            .join("locales")
            .join(format!("{code}.pak")),
    );
}

/// macOS keeps each pack in an `.lproj` folder, and `en.lproj` holds en-US.
fn macos_folder(code: &str) -> String {
    format!(
        "{}.lproj",
        if code == "en-US" {
            "en".into()
        } else {
            code.replace('-', "_")
        }
    )
}

/// The archive member that holds a pack.
fn archive_member(code: &str) -> String {
    if PLATFORM == "macos" {
        format!("{}/locale.pak", macos_folder(code))
    } else {
        format!("{code}.pak")
    }
}

fn valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 8
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn read_manifest() -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(manifest_path()?).ok()?).ok()
}

fn read_selection(root: &Path) -> Option<String> {
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join(SETTINGS)).ok()?).ok()?;
    value["language"]
        .as_str()
        .filter(|code| valid_code(code))
        .map(str::to_owned)
}

fn write_selection(root: &Path, code: Option<&str>) -> Result<(), String> {
    let path = root.join(SETTINGS);
    match code {
        None => match std::fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err("The browser language could not be saved.".into())
            }
            _ => Ok(()),
        },
        Some(code) => std::fs::write(&path, serde_json::json!({ "language": code }).to_string())
            .map_err(|_| "The browser language could not be saved.".into()),
    }
}

fn downloaded_pack(root: &Path, code: &str) -> PathBuf {
    root.join(PACKS).join(format!("{code}.pak"))
}

/// The selected language and its pack, read once when the browser starts.
pub fn startup_pack(root: &Path) -> Option<(String, PathBuf)> {
    let code = read_selection(root);
    let pack = code.as_deref().and_then(|code| {
        [Some(downloaded_pack(root, code)), bundled_pack(code)]
            .into_iter()
            .flatten()
            .find(|path| path.is_file())
            .map(|path| (code.to_owned(), path))
    });
    let _ = ACTIVE.set(pack.as_ref().map(|(code, _)| code.clone()));
    pack
}

/// Sites read the chosen language first, then English.
pub fn accept_languages(code: &str) -> String {
    let base = code.split('-').next().unwrap_or(code);
    let mut list = vec![code.to_owned()];
    if base != code {
        list.push(base.to_owned());
    }
    for fallback in ["en-US", "en"] {
        if !list.iter().any(|item| item == fallback) {
            list.push(fallback.to_owned());
        }
    }
    list.join(",")
}

fn view(root: &Path, manifest: Option<&Manifest>) -> LanguageView {
    let mut languages: Vec<Language> = Vec::new();
    if let Some(manifest) = manifest {
        for code in &manifest.bundled {
            languages.push(Language {
                code: code.clone(),
                state: "bundled",
            });
        }
        for code in &manifest.available {
            let state = if downloaded_pack(root, code).is_file() {
                "downloaded"
            } else {
                "available"
            };
            languages.push(Language {
                code: code.clone(),
                state,
            });
        }
    }
    languages.sort_by(|a, b| a.code.cmp(&b.code));
    LanguageView {
        selected: read_selection(root),
        active: ACTIVE.get().cloned().flatten(),
        languages,
        download_size: manifest.map_or(0, |manifest| manifest.size),
    }
}

fn browser_root() -> Result<PathBuf, String> {
    muniment_runtime::adopt_state_directory()
        .map(|state| state.join("browser"))
        .map_err(|_| "The browser data folder is unavailable.".into())
}

fn download(url: &str, manifest: &Manifest) -> Result<Vec<u8>, String> {
    let failed = "The language download failed. Check your connection and try again.";
    let response = ureq::get(url)
        .timeout(Duration::from_secs(180))
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) => "This build has no published language packs.".to_owned(),
            _ => failed.to_owned(),
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(manifest.size + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed.to_owned())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() as u64 != manifest.size || !digest.eq_ignore_ascii_case(&manifest.sha256) {
        return Err("The downloaded language packs do not match this version of Muniment.".into());
    }
    Ok(bytes)
}

/// Installs one pack from the verified archive, after it parses.
fn install(archive: &[u8], code: &str, target: &Path) -> Result<(), String> {
    let damaged = "The language archive is damaged.";
    let member = archive_member(code);
    let mut entries = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in entries.entries().map_err(|_| damaged)? {
        let mut entry = entry.map_err(|_| damaged)?;
        let path = entry.path().map_err(|_| damaged)?;
        if path.strip_prefix(".").unwrap_or(&path) != Path::new(&member) {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|_| damaged)?;
        crate::cef_locale::parse(&bytes)?;
        let directory = target.parent().ok_or(damaged)?;
        std::fs::create_dir_all(directory).map_err(|_| "The language pack could not be saved.")?;
        let partial = target.with_extension("pak.partial");
        std::fs::write(&partial, &bytes)
            .and_then(|_| std::fs::rename(&partial, target))
            .map_err(|_| "The language pack could not be saved.")?;
        return Ok(());
    }
    Err("The language archive has no pack for this language.".into())
}

fn select(root: &Path, version: &str, code: Option<String>) -> Result<LanguageView, String> {
    let manifest = read_manifest();
    if let Some(code) = code.as_deref() {
        let manifest = manifest
            .as_ref()
            .ok_or("This build has no language list.")?;
        if manifest.bundled.iter().any(|item| item == code) {
        } else if manifest.available.iter().any(|item| item == code) {
            let target = downloaded_pack(root, code);
            if !target.is_file() {
                let url = format!(
                    "{RELEASES}/v{version}/muniment-{version}-{PLATFORM}-chromium-locales.tar.gz"
                );
                install(&download(&url, manifest)?, code, &target)?;
            }
        } else {
            return Err("Choose a language from the list.".into());
        }
    }
    write_selection(root, code.as_deref())?;
    Ok(view(root, manifest.as_ref()))
}

#[tauri::command]
pub fn browser_language() -> Result<LanguageView, String> {
    Ok(view(&browser_root()?, read_manifest().as_ref()))
}

#[tauri::command]
pub async fn browser_language_set(
    app: tauri::AppHandle,
    code: Option<String>,
) -> Result<LanguageView, String> {
    let version = app.package_info().version.to_string();
    if code.as_deref().is_some_and(|code| !valid_code(code)) {
        return Err("Choose a language from the list.".into());
    }
    tauri::async_runtime::spawn_blocking(move || select(&browser_root()?, &version, code))
        .await
        .map_err(|_| "The browser language could not be saved.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(strings: &[(u16, &str)], aliases: &[(u16, u16)]) -> Vec<u8> {
        let mut bytes = vec![5, 0, 0, 0, 1, 0, 0, 0];
        bytes.extend((strings.len() as u16).to_le_bytes());
        bytes.extend((aliases.len() as u16).to_le_bytes());
        let mut offset = 12 + (strings.len() + 1) * 6 + aliases.len() * 4;
        for (id, text) in strings {
            bytes.extend(id.to_le_bytes());
            bytes.extend((offset as u32).to_le_bytes());
            offset += text.len();
        }
        bytes.extend(0u16.to_le_bytes());
        bytes.extend((offset as u32).to_le_bytes());
        for (id, index) in aliases {
            bytes.extend(id.to_le_bytes());
            bytes.extend(index.to_le_bytes());
        }
        for (_, text) in strings {
            bytes.extend(text.as_bytes());
        }
        bytes
    }

    fn archive(member: &str, bytes: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("./{member}"), bytes)
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn packs_read_strings_and_aliases() {
        let strings =
            crate::cef_locale::parse(&pack(&[(102, "Arrêter"), (104, "Recharger")], &[(300, 1)]))
                .unwrap();
        assert_eq!(strings[&102], "Arrêter");
        assert_eq!(strings[&300], "Recharger");
        assert!(crate::cef_locale::parse(&[4, 0, 0, 0]).is_err());
        assert!(crate::cef_locale::parse(&pack(&[(1, "a")], &[])[..14]).is_err());
        assert!(crate::cef_locale::parse(&pack(&[(1, "abc")], &[])[..20]).is_err());
        // A compressed data resource is not a string, and the rest still load.
        let mut compressed = pack(&[(1, "\u{1f}"), (2, "Stoppen")], &[]);
        let at = compressed.len() - "Stoppen".len() - 1;
        compressed[at] = 0x8b;
        let strings = crate::cef_locale::parse(&compressed).unwrap();
        assert_eq!((strings.get(&1), strings[&2].as_str()), (None, "Stoppen"));
    }

    #[test]
    fn a_verified_archive_installs_only_the_chosen_pack() {
        let directory =
            std::env::temp_dir().join(format!("muniment-language-{}", uuid::Uuid::now_v7()));
        let target = directory.join(PACKS).join("nl.pak");
        let bytes = pack(&[(102, "Stoppen")], &[]);
        install(&archive(&archive_member("nl"), &bytes), "nl", &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert!(install(
            &archive(&archive_member("nl"), &bytes),
            "sv",
            &directory.join("sv.pak")
        )
        .is_err());
        assert!(install(
            &archive(&archive_member("nl"), b"not a pack"),
            "nl",
            &target
        )
        .is_err());
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn selection_round_trips_and_lists_the_manifest() {
        let root = std::env::temp_dir().join(format!("muniment-language-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(root.join(PACKS)).unwrap();
        std::fs::write(downloaded_pack(&root, "nl"), b"").unwrap();
        write_selection(&root, Some("nl")).unwrap();
        let manifest = Manifest {
            sha256: String::new(),
            size: 9,
            bundled: vec!["fr".into()],
            available: vec!["nl".into(), "sv".into()],
        };
        let view = view(&root, Some(&manifest));
        assert_eq!(view.selected.as_deref(), Some("nl"));
        let states: Vec<_> = view
            .languages
            .iter()
            .map(|language| (language.code.as_str(), language.state))
            .collect();
        assert_eq!(
            states,
            [("fr", "bundled"), ("nl", "downloaded"), ("sv", "available")]
        );
        write_selection(&root, None).unwrap();
        assert_eq!(read_selection(&root), None);
        assert!(!valid_code("../fr"));
        assert_eq!(accept_languages("pt-BR"), "pt-BR,pt,en-US,en");
        assert_eq!(accept_languages("en-US"), "en-US,en");
        let _ = std::fs::remove_dir_all(root);
    }
}
