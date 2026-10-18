//! Whether the operating system lets the agent runtime's processes read the
//! folder Pi works in.
//!
//! macOS holds a read of the Documents, Desktop or Downloads folder until the
//! user answers its consent prompt, and an app signed ad hoc is asked again
//! after each build. Pi reads its working folder before it answers its first
//! command, so a pending prompt stops Pi without output. The check lists the
//! folder in a child process, as Pi does, so macOS asks on the same terms and
//! the wait has a bound. macOS can ask each executable on its own, and Pi is
//! an executable of its own outside the app, so once Pi is installed the
//! check lists the folder with Pi's executable itself, run as the Bun it is
//! built on (`BUN_BE_BUN`).

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

/// The script Pi's executable runs as Bun: it lists the folder its argument
/// names and says so in `ls`'s words when the operating system refused.
const PI_READ_SCRIPT: &str = "try { require('fs').readdirSync(process.argv[1]) } catch (error) { if (error.code === 'EPERM') { console.error('Operation not permitted'); process.exit(1) } }";

/// Lists `directory` in a child process and waits `bound` at most, or until
/// `cancelled` is set, for the answer.
pub fn check(directory: &Path, bound: Duration, cancelled: &AtomicBool) -> FolderAccess {
    check_as(None, directory, bound, cancelled)
}

/// Lists `directory` like [`check`], with the Pi executable `pi` when one is
/// installed, so the answer is the one Pi's own read gets.
pub fn check_as(
    pi: Option<&Path>,
    directory: &Path,
    bound: Duration,
    cancelled: &AtomicBool,
) -> FolderAccess {
    #[cfg(target_os = "macos")]
    {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::sync::atomic::Ordering;
        use std::time::Instant;
        let mut command = match pi {
            Some(pi) => {
                let mut command = Command::new(pi);
                // Only what Bun needs: the runtime's secrets stay out.
                command.env_clear().env("BUN_BE_BUN", "1");
                for name in ["HOME", "TMPDIR", "PATH"] {
                    if let Some(value) = std::env::var_os(name) {
                        command.env(name, value);
                    }
                }
                command.arg("-e").arg(PI_READ_SCRIPT).arg(directory);
                command
            }
            None => {
                let mut command = Command::new("/bin/ls");
                command.arg("-a").arg(directory);
                command
            }
        };
        let child = crate::child_fds::stdio_only(&mut command)
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
        let _ = (pi, directory, bound, cancelled);
        FolderAccess::Open
    }
}

/// The Pi executable installed under the state directory, which a chat
/// launch runs, if one is installed.
pub fn installed_pi() -> Option<std::path::PathBuf> {
    let state = crate::state_root::state_directory()?;
    let root = crate::chat_profile::ChatProfile::new(state).installed_pi_root();
    crate::sidecar::pi_install::resolve_current(&root).ok()
}

/// Starts reads of `directory` in child processes of this one and returns at
/// once: one by `/bin/ls` and, when `pi` names an installed Pi, one by Pi's
/// executable. macOS tracks folder consent per process that reads, so the
/// runtime that runs Pi, and Pi itself, read its Home while the user looks at
/// the first run, and the prompts those reads raise never wait for the first
/// chat.
pub fn ask_ahead(directory: &Path, pi: Option<std::path::PathBuf>) {
    #[cfg(target_os = "macos")]
    {
        let directory = directory.to_path_buf();
        std::thread::spawn(move || {
            let bound = Duration::from_secs(600);
            let cancelled = AtomicBool::new(false);
            let _ = check(&directory, bound, &cancelled);
            if let Some(pi) = pi {
                let _ = check_as(Some(&pi), &directory, bound, &cancelled);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (directory, pi);
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

    #[cfg(target_os = "macos")]
    #[test]
    fn pi_reads_the_folder_itself_as_bun_and_a_refusal_is_denied() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("muniment-pi-read-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("refused")).unwrap();
        // A Pi that answers only as Bun with a script and the folder.
        let pi = root.join("pi");
        std::fs::write(
            &pi,
            "#!/bin/sh\n[ \"$BUN_BE_BUN\" = 1 ] && [ \"$1\" = -e ] || exit 3\ncase \"$3\" in *refused) echo 'Operation not permitted' >&2; exit 1;; esac\nls -a \"$3\" >/dev/null\n",
        )
        .unwrap();
        std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();
        let bound = Duration::from_secs(10);
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            check_as(Some(&pi), &root, bound, &cancelled),
            FolderAccess::Open
        );
        assert_eq!(
            check_as(Some(&pi), &root.join("refused"), bound, &cancelled),
            FolderAccess::Denied
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn asking_ahead_returns_before_the_read_answers() {
        let started = std::time::Instant::now();
        ask_ahead(&std::env::temp_dir(), None);
        assert!(started.elapsed() < Duration::from_secs(1));
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
