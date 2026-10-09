// Chromium asks the app for each interface string before it reads its own
// language pack. The browser tab answers from the pack chosen in Preferences,
// so a downloaded pack works on every platform the same way as a bundled one.
// The browser process passes the pack's path to each child process, and the
// child reads it before its sandbox closes file access.
use cef::*;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const SWITCH: &str = "muniment-locale-pack";

pub type Strings = Arc<HashMap<u16, String>>;

/// Reads a version 5 Chromium pack: a 12-byte header, a table of (id, offset)
/// entries with one end sentinel, then an alias table of (id, entry index).
pub fn parse(bytes: &[u8]) -> Result<HashMap<u16, String>, String> {
    let u16_at = |at: usize| {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    if u32_at(0) != Some(5) {
        return Err("The language pack has an unknown format.".into());
    }
    let encoding = bytes[4];
    let (count, aliases) = (
        usize::from(u16_at(8).unwrap_or(0)),
        usize::from(u16_at(10).unwrap_or(0)),
    );
    let entry = |index: usize| Some((u16_at(12 + index * 6)?, u32_at(14 + index * 6)?));
    let text = |index: usize| -> Option<String> {
        let (start, end) = (entry(index)?.1, entry(index + 1)?.1);
        let raw = bytes.get(start..end)?;
        match encoding {
            1 => String::from_utf8(raw.to_vec()).ok(),
            2 => String::from_utf16(
                &raw.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect::<Vec<_>>(),
            )
            .ok(),
            _ => None,
        }
    };
    if entry(count).is_none_or(|(_, end)| end > bytes.len()) {
        return Err("The language pack is truncated.".into());
    }
    let mut strings = HashMap::with_capacity(count + aliases);
    for index in 0..count {
        let id = entry(index).ok_or("The language pack is truncated.")?.0;
        // A pack can hold a compressed data resource. Chromium never asks for it as a string.
        if let Some(value) = text(index) {
            strings.insert(id, value);
        }
    }
    let table = 12 + (count + 1) * 6;
    for alias in 0..aliases {
        let (id, index) = (u16_at(table + alias * 4), u16_at(table + alias * 4 + 2));
        let (Some(id), Some(index)) = (id, index) else {
            return Err("The language pack is truncated.".into());
        };
        if let Some(value) = text(usize::from(index)) {
            strings.insert(id, value);
        }
    }
    Ok(strings)
}

pub fn load(path: &Path) -> Option<Strings> {
    parse(&std::fs::read(path).ok()?).ok().map(Arc::new)
}

/// The pack a child process receives from the browser process.
// The helper binary and the Windows child entry call this, not the macOS or Linux app.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn from_args() -> Option<Strings> {
    let prefix = format!("--{SWITCH}=");
    std::env::args()
        .find_map(|arg| arg.strip_prefix(&prefix).map(PathBuf::from))
        .and_then(|path| load(&path))
}

wrap_resource_bundle_handler! {
    pub struct LocaleStrings {
        strings: Strings,
    }
    impl ResourceBundleHandler {
        fn localized_string(&self, string_id: ::std::os::raw::c_int, string: Option<&mut CefString>) -> ::std::os::raw::c_int {
            let value = u16::try_from(string_id).ok().and_then(|id| self.strings.get(&id));
            match (value, string) {
                (Some(value), Some(string)) => string.try_set(value).into(),
                _ => 0,
            }
        }
    }
}

wrap_app! {
    pub struct LocaleApp {
        strings: Strings,
    }
    impl App {
        fn resource_bundle_handler(&self) -> Option<ResourceBundleHandler> {
            Some(LocaleStrings::new(self.strings.clone()))
        }
    }
}
