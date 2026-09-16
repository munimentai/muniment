//! The families installed on this device, read from the platform's font
//! registry so Preferences can offer them by name. Nothing here reads a font
//! file, and the list never leaves the device.

use std::collections::BTreeSet;

/// Every installed family name, sorted, once each, without the platform's
/// hidden system families.
#[tauri::command]
pub fn installed_fonts() -> Vec<String> {
    tidy_families(platform_families())
}

fn tidy_families(names: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut families = Vec::new();
    for name in names {
        let family = family_name(&name);
        if family.is_empty() || family.starts_with('.') || family.chars().any(char::is_control) {
            continue;
        }
        if seen.insert(family.to_lowercase()) {
            families.push(family);
        }
    }
    families.sort_by_key(|family| family.to_lowercase());
    families
}

// The Windows registry names a face, such as `Arial Bold (TrueType)`; the
// other platforms name a family. Both reduce to the family.
fn family_name(name: &str) -> String {
    const STYLES: &[&str] = &[
        "Regular",
        "Bold",
        "Italic",
        "Oblique",
        "Light",
        "Medium",
        "Semibold",
        "SemiBold",
        "Demibold",
        "DemiBold",
        "Black",
        "Heavy",
        "Thin",
        "ExtraLight",
        "ExtraBold",
        "Condensed",
        "Narrow",
        "Book",
    ];
    let mut family = name.trim();
    if let Some(start) = family.find(" (") {
        if family.ends_with(')') {
            family = family[..start].trim_end();
        }
    }
    while let Some((head, tail)) = family.rsplit_once(' ') {
        if !STYLES.contains(&tail) {
            break;
        }
        family = head.trim_end();
    }
    family.to_owned()
}

#[cfg(target_os = "macos")]
fn platform_families() -> Vec<String> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSString};

    #[link(name = "CoreText", kind = "framework")]
    extern "C" {
        fn CTFontManagerCopyAvailableFontFamilyNames() -> *mut NSArray<NSString>;
    }

    // SAFETY: CoreText returns a retained CFArray of CFStrings, toll-free
    // bridged to NSArray<NSString>, and the caller owns that reference.
    let names = unsafe { Retained::from_raw(CTFontManagerCopyAvailableFontFamilyNames()) };
    names
        .map(|names| names.iter().map(|name| name.to_string()).collect())
        .unwrap_or_default()
}

#[cfg(windows)]
fn platform_families() -> Vec<String> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    let path: Vec<u16> = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts\0"
        .encode_utf16()
        .collect();
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: `path` is a terminated wide string and `key` receives the handle.
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) }
        != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let mut families = Vec::new();
    let mut index = 0_u32;
    loop {
        let mut name = [0_u16; 512];
        let mut length = name.len() as u32;
        // SAFETY: `name` holds `length` UTF-16 units and the unused outputs are null.
        let status = unsafe {
            RegEnumValueW(
                key,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status == ERROR_SUCCESS {
            families.push(String::from_utf16_lossy(&name[..length as usize]));
        } else if status != ERROR_MORE_DATA {
            break;
        }
        index += 1;
    }
    // SAFETY: `key` came from a successful RegOpenKeyExW and closes once.
    unsafe { RegCloseKey(key) };
    families
}

#[cfg(target_os = "linux")]
fn platform_families() -> Vec<String> {
    // fontconfig lists each family once per face; the first name on a line is
    // the family, the rest are its other names.
    let Ok(output) = std::process::Command::new("fc-list")
        .args([":", "family"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split(',').next())
        .map(|family| family.trim().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_faces_to_families_once_each_and_sorted() {
        let families = tidy_families(vec![
            "Arial Bold (TrueType)".into(),
            "Arial (TrueType)".into(),
            "arial Italic (TrueType)".into(),
            ".SF NS Text".into(),
            "Noto Sans".into(),
            "IBM Plex Mono Medium".into(),
            "  ".into(),
            "Bad\u{7}Name".into(),
        ]);
        assert_eq!(families, ["Arial", "IBM Plex Mono", "Noto Sans"]);
    }

    #[test]
    fn keeps_a_family_whose_name_ends_in_a_word_that_is_not_a_style() {
        assert_eq!(
            family_name("Source Serif 4 Display"),
            "Source Serif 4 Display"
        );
        assert_eq!(family_name("Helvetica Neue Light"), "Helvetica Neue");
    }

    #[test]
    fn reads_the_platform_registry_without_failing() {
        // The list may be empty on a bare machine, but the read itself completes.
        let _ = installed_fonts();
    }
}
