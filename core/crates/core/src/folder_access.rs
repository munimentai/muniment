//! Whether the operating system lets the agent runtime's processes read the
//! folder Pi works in.
//!
//! macOS holds a read of the Documents, Desktop or Downloads folder until the
//! user answers its consent prompt, and an app signed ad hoc is asked again
//! after each build. Pi reads its working folder before it answers its first
//! command, so a pending prompt stops Pi without output. The check lists the
//! folder in a child process, as Pi does, so macOS asks on the same terms and
//! the wait has a bound.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderAccess {
    /// The folder could be read, or failed for a reason Pi reports itself.
    Open,
    /// The operating system refused the read.
    Denied,
    /// The read had not finished when the bound ran out.
    Waiting,
}

/// Lists `directory` in a child process and waits `bound` at most, or until
/// `cancelled` is set, for the answer.
pub fn check(directory: &Path, bound: Duration, cancelled: &AtomicBool) -> FolderAccess {
    #[cfg(target_os = "macos")]
    {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::sync::atomic::Ordering;
        use std::time::Instant;
        let child = Command::new("/bin/ls")
            .arg("-a")
            .arg(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            return FolderAccess::Open;
        };
        let deadline = Instant::now() + bound;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stderr = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        let _ = pipe.read_to_string(&mut stderr);
                    }
                    return if !status.success() && stderr.contains("Operation not permitted") {
                        FolderAccess::Denied
                    } else {
                        FolderAccess::Open
                    };
                }
                Ok(None) if Instant::now() < deadline && !cancelled.load(Ordering::SeqCst) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return FolderAccess::Waiting;
                }
                Err(_) => return FolderAccess::Open,
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (directory, bound, cancelled);
        FolderAccess::Open
    }
}

/// The name macOS gives the protected folder that holds `directory`, if any.
/// Logs and messages name the folder, never the path.
pub fn protected_folder(directory: &Path, home: &Path) -> Option<&'static str> {
    ["Documents", "Desktop", "Downloads"]
        .into_iter()
        .find(|name| directory.starts_with(home.join(name)))
}

/// What a run tells the user when macOS holds or refuses Pi's folder.
pub fn failure_reason(access: FolderAccess, folder: Option<&str>) -> String {
    let folder = folder.map_or("the folder this thread works in".to_owned(), |name| {
        format!("your {name} folder")
    });
    match access {
        FolderAccess::Denied => format!(
            "macOS does not let Muniment open {folder}. Turn on Muniment in System Settings > \
             Privacy & Security > Files & Folders, then try again."
        ),
        _ => format!(
            "macOS is asking whether Muniment may open {folder}. Choose Allow in that prompt, \
             then try again."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readable_folder_is_open_within_the_bound() {
        let directory = std::env::temp_dir();
        assert_eq!(
            check(&directory, Duration::from_secs(10), &AtomicBool::new(false)),
            FolderAccess::Open
        );
    }

    #[test]
    fn a_missing_folder_is_left_to_pi() {
        let directory =
            std::env::temp_dir().join(format!("muniment-missing-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            check(&directory, Duration::from_secs(10), &AtomicBool::new(false)),
            FolderAccess::Open
        );
    }

    #[test]
    fn messages_name_the_protected_folder_and_never_the_path() {
        let home = Path::new("/Users/owner");
        let thread = home.join("Documents/muniment/sessions/Untitled-1");
        assert_eq!(protected_folder(&thread, home), Some("Documents"));
        assert_eq!(
            protected_folder(&home.join("Desktop"), home),
            Some("Desktop")
        );
        assert_eq!(protected_folder(&home.join("Muniment"), home), None);
        assert_eq!(
            protected_folder(Path::new("/Users/owner/DocumentsOld"), home),
            None
        );
        let waiting = failure_reason(FolderAccess::Waiting, Some("Documents"));
        assert!(waiting.contains("your Documents folder"), "{waiting}");
        assert!(waiting.contains("Allow"), "{waiting}");
        let denied = failure_reason(FolderAccess::Denied, None);
        assert!(denied.contains("Files & Folders"), "{denied}");
        assert!(!denied.contains("/Users"), "{denied}");
    }
}
