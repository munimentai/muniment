use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

// A FUSE AppImage cannot provide a working setuid helper. Reuse only the
// helper installed by our DEB, never an arbitrary PATH executable.
// Chromium checks the sandbox API version before it uses the helper.
pub fn select(bundled: &Path) -> PathBuf {
    select_installed(bundled, Path::new("/usr/lib/muniment/cef/chrome-sandbox"))
}

fn privileged(uid: u32, mode: u32) -> bool {
    uid == 0 && mode & 0o7777 == 0o4755
}

fn select_installed(bundled: &Path, installed: &Path) -> PathBuf {
    let trusted = std::fs::symlink_metadata(installed)
        .is_ok_and(|m| m.is_file() && privileged(m.uid(), m.mode()));
    if trusted {
        return installed.to_path_buf();
    }
    // Chromium can still use its user-namespace sandbox on supporting hosts.
    bundled.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn rejects_unprivileged_and_writable_helpers() {
        assert!(privileged(0, 0o104755));
        for (uid, mode) in [(1000, 0o4755), (0, 0o0755), (0, 0o4775), (0, 0o4777)] {
            assert!(!privileged(uid, mode));
        }
    }

    #[test]
    fn selects_only_a_regular_privileged_installed_helper() {
        let dir = std::env::temp_dir().join(format!("muniment-sandbox-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let bundled = dir.join("bundled");
        let installed = dir.join("installed");
        std::fs::write(&bundled, b"current helper").unwrap();
        assert_eq!(select_installed(&bundled, &installed), bundled);
        std::fs::write(&installed, b"current helper").unwrap();
        std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o4755)).unwrap();
        let expected = if std::fs::metadata(&installed).unwrap().uid() == 0 {
            &installed
        } else {
            &bundled
        };
        assert_eq!(select_installed(&bundled, &installed), *expected);
        std::fs::remove_file(&installed).unwrap();
        symlink(&bundled, &installed).unwrap();
        assert_eq!(select_installed(&bundled, &installed), bundled);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
