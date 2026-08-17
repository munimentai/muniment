//! Installed runtime executable replacement watch.

use muniment_core::attach::{evaluate_quiesce, DrainState, RuntimeActivityRegistry};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc::Sender, Arc};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExecutableIdentity {
    device: u64,
    inode: u64,
}

impl ExecutableIdentity {
    fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

pub(crate) struct UpgradeWatch {
    pub path: PathBuf,
    pub poll_interval: Duration,
}

impl UpgradeWatch {
    pub(crate) fn start(
        self,
        drain_state: DrainState,
        activity: RuntimeActivityRegistry,
        stopped: Arc<AtomicBool>,
        refresh_pending: Arc<AtomicBool>,
        listener_stop: Sender<()>,
    ) -> io::Result<std::thread::JoinHandle<()>> {
        let initial_identity = ExecutableIdentity::read(&self.path)?;
        Ok(std::thread::spawn(move || loop {
            if stopped.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(self.poll_interval);
            if !refresh_pending.load(Ordering::Acquire) {
                let replaced = ExecutableIdentity::read(&self.path)
                    .map(|identity| identity != initial_identity)
                    .unwrap_or(true);
                if replaced {
                    drain_state.set();
                    refresh_pending.store(true, Ordering::Release);
                }
            }
            if refresh_pending.load(Ordering::Acquire)
                && evaluate_quiesce(activity.snapshot()).is_ok()
            {
                let _ = listener_stop.send(());
                break;
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn replacement_drains_and_waits_for_quiesce() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "muniment-upgrade-watch-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let watched = directory.join("runtime");
        File::create(&watched).unwrap();
        let drain = DrainState::new();
        let activity = RuntimeActivityRegistry::new();
        let blocker = activity.mark_active_run();
        let stopped = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(AtomicBool::new(false));
        let (exit_tx, exit_rx) = mpsc::channel();
        let thread = UpgradeWatch {
            path: watched.clone(),
            poll_interval: Duration::from_millis(5),
        }
        .start(
            drain.clone(),
            activity,
            stopped,
            Arc::clone(&pending),
            exit_tx,
        )
        .unwrap();

        let replacement = directory.join("replacement");
        File::create(&replacement).unwrap();
        fs::rename(&replacement, &watched).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !drain.is_set() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(drain.is_set());
        assert!(pending.load(Ordering::Acquire));
        assert!(exit_rx.try_recv().is_err());

        File::create(directory.join("second-replacement")).unwrap();
        fs::rename(directory.join("second-replacement"), &watched).unwrap();
        assert!(exit_rx.try_recv().is_err());
        drop(blocker);
        exit_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        thread.join().unwrap();
        fs::remove_dir_all(directory).unwrap();
    }
}
