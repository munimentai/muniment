//! macOS attach connection routing.

use std::path::{Path, PathBuf};

use muniment_attach::{decode_frame, Hello};

/// The handler for a new macOS attach connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacosAttachConnectionRoute {
    ApprovalPresenter,
    DesktopClient {
        peer_pid: u32,
    },
    Companion {
        peer_pid: u32,
    },
    /// The `muniment-cli` this bundle ships, run under this runtime, as Pi
    /// runs the record server for an agent run. Its pairing needs no prompt.
    BundledCompanion {
        peer_pid: u32,
    },
}

/// Where the bundle that holds `expected_desktop_executable` ships the CLI:
/// `Contents/Library/LaunchServices/muniment-cli` beside `Contents/MacOS`.
pub fn bundled_cli_executable(expected_desktop_executable: &Path) -> Option<PathBuf> {
    let contents = expected_desktop_executable
        .parent()
        .filter(|parent| parent.file_name() == Some(std::ffi::OsStr::new("MacOS")))?
        .parent()
        .filter(|parent| parent.file_name() == Some(std::ffi::OsStr::new("Contents")))?;
    Some(contents.join("Library/LaunchServices/muniment-cli"))
}

/// Opaque failure from an injected macOS peer read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MacosPeerReadError;

/// Injected boundary around the connected peer process read.
pub trait MacosAttachRouteReader {
    fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError>;

    /// Whether the process that connected satisfies the designated code requirement of
    /// `expected_desktop_executable`. The kernel records the peer's audit token at
    /// connect, and the token's pid version changes on exec, so a process that execs
    /// the desktop binary after it connects never matches.
    fn peer_code_matches(&self, expected_desktop_executable: &Path) -> bool;

    /// Whether the peer closed without sending a frame, as a readiness probe
    /// does. Another runtime's bind connects once to learn whether the
    /// endpoint is live, then closes.
    fn peer_closed(&self) -> bool {
        false
    }

    /// Whether `peer_pid` runs under this process: this process is its parent,
    /// or a parent of one of its parents.
    fn peer_descends_from_this_process(&self, _peer_pid: u32) -> bool {
        false
    }

    /// Returns payload-free results from the code check for this connection.
    fn code_check_diagnostic(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Records a payload-free diagnostic when the peer cannot use desktop routes.
    fn log_companion_fallback(
        &self,
        _peer_pid: u32,
        _path_match: bool,
        _peer_image_path: Option<&Path>,
        _expected_desktop_executable: &Path,
    ) {
    }
}

/// Names the route from the connected peer's code identity.
pub fn name_macos_attach_connection_route(
    reader: &impl MacosAttachRouteReader,
    expected_desktop_executable: &Path,
) -> MacosAttachConnectionRoute {
    let Ok((peer_pid, peer_image_path)) = reader.peer_process() else {
        // A peer that already closed sends nothing to route, so it logs nothing.
        if !reader.peer_closed() {
            reader.log_companion_fallback(0, false, None, expected_desktop_executable);
        }
        return MacosAttachConnectionRoute::Companion { peer_pid: 0 };
    };
    let resolved_paths =
        if peer_image_path.is_absolute() && expected_desktop_executable.is_absolute() {
            peer_image_path
                .canonicalize()
                .ok()
                .zip(expected_desktop_executable.canonicalize().ok())
        } else {
            None
        };
    let path_match = resolved_paths
        .as_ref()
        .is_some_and(|(peer, expected)| peer == expected);
    // Chromium can run an identical signed bundle from a code-sign clone.
    // Paths must resolve, but only the audit-token code check admits the peer.
    if peer_pid != 0
        && resolved_paths.is_some()
        && reader.peer_code_matches(expected_desktop_executable)
    {
        MacosAttachConnectionRoute::DesktopClient { peer_pid }
    } else if peer_pid != 0
        && is_bundled_cli(&peer_image_path, expected_desktop_executable)
        && reader.peer_descends_from_this_process(peer_pid)
        && bundled_cli_executable(expected_desktop_executable)
            .is_some_and(|cli| reader.peer_code_matches(&cli))
    {
        // The CLI's designated requirement names its own identifier, so the
        // desktop's requirement never admits it. Its own requirement does.
        MacosAttachConnectionRoute::BundledCompanion { peer_pid }
    } else {
        // A probe that closed sends nothing to route, so its failed check logs nothing.
        if !reader.peer_closed() {
            reader.log_companion_fallback(
                peer_pid,
                path_match,
                Some(&peer_image_path),
                expected_desktop_executable,
            );
        }
        MacosAttachConnectionRoute::Companion { peer_pid }
    }
}

/// Whether `peer_image_path` resolves to the CLI the desktop's bundle ships.
fn is_bundled_cli(peer_image_path: &Path, expected_desktop_executable: &Path) -> bool {
    peer_image_path.is_absolute()
        && bundled_cli_executable(expected_desktop_executable)
            .and_then(|cli| cli.canonicalize().ok())
            .zip(peer_image_path.canonicalize().ok())
            .is_some_and(|(cli, peer)| cli == peer)
}

/// Names the final route for a desktop-executable peer from its first frame.
pub fn name_macos_desktop_attach_connection_route(
    peer_pid: u32,
    first_frame: &[u8],
) -> MacosAttachConnectionRoute {
    let Ok(Some((hello, consumed))) = decode_frame::<Hello>(first_frame) else {
        return MacosAttachConnectionRoute::Companion { peer_pid };
    };
    if consumed != first_frame.len() {
        return MacosAttachConnectionRoute::Companion { peer_pid };
    }

    match hello.client.kind.as_str() {
        "desktop" => MacosAttachConnectionRoute::ApprovalPresenter,
        "desktop-client" => MacosAttachConnectionRoute::DesktopClient { peer_pid },
        _ => MacosAttachConnectionRoute::Companion { peer_pid },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Bundle {
        root: PathBuf,
        executable: PathBuf,
    }

    impl Bundle {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("muniment-macos-route-{}", uuid::Uuid::new_v4()));
            let executable = root.join("Muniment.app/Contents/MacOS/muniment");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, b"desktop fixture").unwrap();
            Self { root, executable }
        }
    }

    impl Drop for Bundle {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    struct StubRouteReader {
        peer: Result<(u32, PathBuf), MacosPeerReadError>,
        code_matches: bool,
        /// When set, the peer's code satisfies only this path's requirement.
        code_of: Option<PathBuf>,
        descends: bool,
        checked: RefCell<Option<PathBuf>>,
    }

    impl StubRouteReader {
        fn signed(peer: Result<(u32, PathBuf), MacosPeerReadError>) -> Self {
            Self {
                peer,
                code_matches: true,
                code_of: None,
                descends: true,
                checked: RefCell::new(None),
            }
        }
    }

    impl MacosAttachRouteReader for StubRouteReader {
        fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError> {
            self.peer.clone()
        }

        fn peer_code_matches(&self, expected: &Path) -> bool {
            *self.checked.borrow_mut() = Some(expected.to_owned());
            self.code_matches
                && self
                    .code_of
                    .as_ref()
                    .is_none_or(|code_of| code_of == expected)
        }

        fn peer_descends_from_this_process(&self, _: u32) -> bool {
            self.descends
        }
    }

    /// A bundle fixture with the CLI where the bundle ships it.
    fn bundle_with_cli() -> (Bundle, PathBuf) {
        let bundle = Bundle::new();
        let cli = bundled_cli_executable(&bundle.executable).unwrap();
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(&cli, b"cli fixture").unwrap();
        (bundle, cli)
    }

    #[test]
    fn names_the_cli_path_inside_the_desktop_bundle() {
        assert_eq!(
            bundled_cli_executable(Path::new(
                "/Applications/muniment.app/Contents/MacOS/muniment-desktop"
            )),
            Some(PathBuf::from(
                "/Applications/muniment.app/Contents/Library/LaunchServices/muniment-cli"
            ))
        );
        for loose in [
            "/usr/local/bin/muniment-desktop",
            "/x/Contents/bin/muniment",
            "muniment",
        ] {
            assert_eq!(bundled_cli_executable(Path::new(loose)), None, "{loose}");
        }
    }

    #[test]
    fn routes_the_bundled_cli_under_this_runtime_to_the_bundled_companion() {
        let (bundle, cli) = bundle_with_cli();
        let mut reader = StubRouteReader::signed(Ok((42, cli.clone())));
        reader.code_of = Some(cli.clone());

        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::BundledCompanion { peer_pid: 42 }
        );
        // The CLI is held to its own designated requirement, not the desktop's.
        assert_eq!(*reader.checked.borrow(), Some(cli));
    }

    #[test]
    fn routes_the_bundled_cli_to_companion_unless_every_check_passes() {
        let (bundle, cli) = bundle_with_cli();

        // Another process tree ran the bundled CLI, as a terminal or another harness does.
        let mut elsewhere = StubRouteReader::signed(Ok((42, cli.clone())));
        elsewhere.code_of = Some(cli.clone());
        elsewhere.descends = false;
        // The file at the CLI's path does not satisfy the CLI's requirement.
        let mut unsigned = StubRouteReader::signed(Ok((42, cli.clone())));
        unsigned.code_matches = false;
        // A pid of zero names no process to check.
        let mut zero = StubRouteReader::signed(Ok((0, cli.clone())));
        zero.code_of = Some(cli.clone());
        for reader in [elsewhere, unsigned, zero] {
            assert!(matches!(
                name_macos_attach_connection_route(&reader, &bundle.executable),
                MacosAttachConnectionRoute::Companion { .. }
            ));
        }
    }

    #[test]
    fn routes_a_cli_from_another_bundle_to_companion_without_its_code_check() {
        let (bundle, _) = bundle_with_cli();
        let (_other, other_cli) = bundle_with_cli();
        let mut reader = StubRouteReader::signed(Ok((42, other_cli.clone())));
        reader.code_of = Some(other_cli);

        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::Companion { peer_pid: 42 }
        );
        assert_eq!(*reader.checked.borrow(), Some(bundle.executable.clone()));
    }

    #[test]
    fn reports_failed_desktop_checks_without_admitting_the_peer() {
        use std::cell::Cell;

        #[derive(Debug, PartialEq)]
        struct Fallback {
            pid: u32,
            path_match: bool,
            peer: Option<PathBuf>,
            expected: PathBuf,
        }

        struct Reader {
            peer: Result<(u32, PathBuf), MacosPeerReadError>,
            code_matches: bool,
            checked: Cell<bool>,
            fallback: RefCell<Option<Fallback>>,
        }
        impl MacosAttachRouteReader for Reader {
            fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError> {
                self.peer.clone()
            }

            fn peer_code_matches(&self, _: &Path) -> bool {
                self.checked.set(true);
                self.code_matches
            }

            fn log_companion_fallback(
                &self,
                pid: u32,
                path_match: bool,
                peer_image_path: Option<&Path>,
                expected_desktop_executable: &Path,
            ) {
                *self.fallback.borrow_mut() = Some(Fallback {
                    pid,
                    path_match,
                    peer: peer_image_path.map(Path::to_owned),
                    expected: expected_desktop_executable.to_owned(),
                });
            }
        }
        let bundle = Bundle::new();
        let expected = bundle.executable.as_path();
        for (path, code_matches, path_match) in [
            (Some(expected), false, true),
            (
                Some(Path::new("/Applications/Other.app/Contents/MacOS/other")),
                false,
                false,
            ),
            (Some(Path::new("muniment")), true, false),
            (Some(Path::new("")), true, false),
            (None, true, false),
        ] {
            let peer_pid = if path.is_some() { 42 } else { 0 };
            let reader = Reader {
                peer: path
                    .map(|path| (peer_pid, path.to_owned()))
                    .ok_or(MacosPeerReadError),
                code_matches,
                checked: Cell::new(false),
                fallback: RefCell::new(None),
            };
            assert_eq!(
                name_macos_attach_connection_route(&reader, expected),
                MacosAttachConnectionRoute::Companion { peer_pid }
            );
            assert_eq!(reader.checked.get(), path_match);
            assert_eq!(
                *reader.fallback.borrow(),
                Some(Fallback {
                    pid: peer_pid,
                    path_match,
                    peer: path.map(Path::to_owned),
                    expected: expected.to_owned(),
                })
            );
        }
    }

    #[test]
    fn a_probe_that_closed_after_its_pid_read_logs_no_failed_check() {
        struct Reader {
            closed: bool,
            logged: std::cell::Cell<bool>,
        }
        impl MacosAttachRouteReader for Reader {
            fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError> {
                Ok((
                    91743,
                    PathBuf::from("/Applications/Muniment.app/Contents/Library/LaunchServices/muniment-runtime"),
                ))
            }
            fn peer_code_matches(&self, _: &Path) -> bool {
                false
            }
            fn peer_closed(&self) -> bool {
                self.closed
            }
            fn log_companion_fallback(&self, _: u32, _: bool, _: Option<&Path>, _: &Path) {
                self.logged.set(true);
            }
        }
        let bundle = Bundle::new();
        for closed in [true, false] {
            let reader = Reader {
                closed,
                logged: std::cell::Cell::new(false),
            };
            assert_eq!(
                name_macos_attach_connection_route(&reader, &bundle.executable),
                MacosAttachConnectionRoute::Companion { peer_pid: 91743 }
            );
            // Only a peer that stays to talk logs the failed check.
            assert_eq!(reader.logged.get(), !closed);
        }
    }

    #[test]
    fn routes_a_matching_path_without_the_code_signature_to_companion() {
        let bundle = Bundle::new();
        let mut reader = StubRouteReader::signed(Ok((42, bundle.executable.clone())));
        reader.code_matches = false;

        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::Companion { peer_pid: 42 }
        );
        assert_eq!(*reader.checked.borrow(), Some(bundle.executable.clone()));
    }

    #[test]
    fn routes_matching_absolute_image_to_desktop_client() {
        let bundle = Bundle::new();
        let reader = StubRouteReader::signed(Ok((42, bundle.executable.clone())));

        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::DesktopClient { peer_pid: 42 }
        );
        assert_eq!(*reader.checked.borrow(), Some(bundle.executable.clone()));
    }

    #[test]
    fn a_peer_that_closed_routes_to_companion_without_a_fallback_record() {
        struct Closed(std::cell::Cell<bool>, bool);
        impl MacosAttachRouteReader for Closed {
            fn peer_process(&self) -> Result<(u32, PathBuf), MacosPeerReadError> {
                Err(MacosPeerReadError)
            }
            fn peer_closed(&self) -> bool {
                self.1
            }
            fn peer_code_matches(&self, _: &Path) -> bool {
                false
            }
            fn log_companion_fallback(&self, _: u32, _: bool, _: Option<&Path>, _: &Path) {
                self.0.set(true);
            }
        }
        for closed in [true, false] {
            let reader = Closed(std::cell::Cell::new(false), closed);
            assert_eq!(
                name_macos_attach_connection_route(
                    &reader,
                    Path::new("/Applications/Muniment.app")
                ),
                MacosAttachConnectionRoute::Companion { peer_pid: 0 }
            );
            // Only a read that failed on an open connection is recorded.
            assert_eq!(reader.0.get(), !closed);
        }
    }

    #[test]
    fn routes_read_failure_to_companion() {
        let reader = StubRouteReader::signed(Err(MacosPeerReadError));

        assert_eq!(
            name_macos_attach_connection_route(
                &reader,
                Path::new("/Applications/Muniment.app/Contents/MacOS/muniment")
            ),
            MacosAttachConnectionRoute::Companion { peer_pid: 0 }
        );
        assert_eq!(*reader.checked.borrow(), None);
    }

    #[test]
    fn routes_relative_peer_image_to_companion() {
        let bundle = Bundle::new();
        let reader = StubRouteReader::signed(Ok((42, PathBuf::from("muniment"))));

        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::Companion { peer_pid: 42 }
        );
    }

    #[test]
    fn routes_relative_expected_image_to_companion() {
        let bundle = Bundle::new();
        let reader = StubRouteReader::signed(Ok((42, bundle.executable.clone())));

        assert_eq!(
            name_macos_attach_connection_route(&reader, Path::new("muniment")),
            MacosAttachConnectionRoute::Companion { peer_pid: 42 }
        );
    }

    #[test]
    fn routes_an_unrelated_signed_image_to_companion() {
        let bundle = Bundle::new();
        let other = Bundle::new();
        let mut reader = StubRouteReader::signed(Ok((42, other.executable.clone())));
        reader.code_matches = false;

        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::Companion { peer_pid: 42 }
        );
        assert_eq!(*reader.checked.borrow(), Some(bundle.executable.clone()));
    }

    #[test]
    fn routes_a_code_sign_clone_only_when_its_code_matches() {
        let bundle = Bundle::new();
        let clone = bundle.root.join(
            "X/ai.muniment.desktop.code_sign_clone/code_sign_clone.fixture/muniment.app.bundle/Contents/MacOS/muniment",
        );
        std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
        std::fs::copy(&bundle.executable, &clone).unwrap();
        assert_ne!(
            clone.canonicalize().unwrap(),
            bundle.executable.canonicalize().unwrap()
        );

        // The runtime can also run from the clone.
        for (peer, expected) in [(&clone, &bundle.executable), (&bundle.executable, &clone)] {
            for code_matches in [true, false] {
                let mut reader = StubRouteReader::signed(Ok((42, peer.clone())));
                reader.code_matches = code_matches;
                assert_eq!(
                    name_macos_attach_connection_route(&reader, expected),
                    if code_matches {
                        MacosAttachConnectionRoute::DesktopClient { peer_pid: 42 }
                    } else {
                        MacosAttachConnectionRoute::Companion { peer_pid: 42 }
                    }
                );
                assert_eq!(*reader.checked.borrow(), Some(expected.clone()));
            }
        }
    }

    #[test]
    fn routes_a_zero_pid_to_companion_without_a_code_check() {
        let bundle = Bundle::new();
        let reader = StubRouteReader::signed(Ok((0, bundle.executable.clone())));
        assert_eq!(
            name_macos_attach_connection_route(&reader, &bundle.executable),
            MacosAttachConnectionRoute::Companion { peer_pid: 0 }
        );
        assert_eq!(*reader.checked.borrow(), None);
    }

    #[test]
    fn routes_unresolvable_paths_to_companion_without_a_code_check() {
        let bundle = Bundle::new();
        let missing = bundle.root.join("missing");
        for (peer, expected) in [
            (&missing, &bundle.executable),
            (&bundle.executable, &missing),
            (&missing, &missing),
        ] {
            let reader = StubRouteReader::signed(Ok((42, peer.clone())));
            assert_eq!(
                name_macos_attach_connection_route(&reader, expected),
                MacosAttachConnectionRoute::Companion { peer_pid: 42 }
            );
            assert_eq!(*reader.checked.borrow(), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn routes_a_symlinked_bundle_location_only_after_the_code_check() {
        let bundle = Bundle::new();
        let location = bundle.root.join("linked-location");
        std::os::unix::fs::symlink(&bundle.root, &location).unwrap();
        let linked = location.join("Muniment.app/Contents/MacOS/muniment");
        let canonical = bundle.executable.canonicalize().unwrap();
        assert_ne!(linked, canonical);

        for (peer, expected) in [(&canonical, &linked), (&linked, &canonical)] {
            for code_matches in [true, false] {
                let mut reader = StubRouteReader::signed(Ok((42, peer.clone())));
                reader.code_matches = code_matches;
                assert_eq!(
                    name_macos_attach_connection_route(&reader, expected),
                    if code_matches {
                        MacosAttachConnectionRoute::DesktopClient { peer_pid: 42 }
                    } else {
                        MacosAttachConnectionRoute::Companion { peer_pid: 42 }
                    }
                );
                assert_eq!(*reader.checked.borrow(), Some(expected.clone()));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn routes_broken_and_looping_symlinks_to_companion() {
        let bundle = Bundle::new();
        let broken = bundle.root.join("broken");
        let looping = bundle.root.join("looping");
        std::os::unix::fs::symlink(bundle.root.join("missing"), &broken).unwrap();
        std::os::unix::fs::symlink(&looping, &looping).unwrap();

        for path in [&broken, &looping] {
            for (peer, expected) in [
                (path, &bundle.executable),
                (&bundle.executable, path),
                (path, path),
            ] {
                let reader = StubRouteReader::signed(Ok((42, peer.clone())));
                assert_eq!(
                    name_macos_attach_connection_route(&reader, expected),
                    MacosAttachConnectionRoute::Companion { peer_pid: 42 }
                );
                assert_eq!(*reader.checked.borrow(), None);
            }
        }
    }
}
