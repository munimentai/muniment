use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, ExitStatus};

// Keep the receipt open so path changes cannot redirect the runtime exit status.
pub(crate) struct RuntimeChild {
    child: Child,
    receipt: File,
    recorded: bool,
}

impl RuntimeChild {
    pub(super) fn new(child: Child, receipt: File) -> Self {
        Self {
            child,
            receipt,
            recorded: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if let Some(status) = status {
            self.record(status)?;
        }
        Ok(status)
    }

    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.record(status)?;
        Ok(status)
    }

    fn record(&mut self, status: ExitStatus) -> io::Result<()> {
        if !self.recorded {
            self.receipt.seek(SeekFrom::Start(0))?;
            self.receipt.set_len(0)?;
            writeln!(self.receipt, "{}", status.into_raw())?;
            self.recorded = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn receipts_survive_reaping_while_the_owner_stays_alive() {
        for (name, script, code, signal) in [
            ("exit", "read line; exit 42", Some(42), None),
            ("signal", "read line; kill -TERM $$", None, Some(15)),
        ] {
            let path = std::env::temp_dir().join(format!(
                "muniment-runtime-receipt-{}-{name}",
                std::process::id()
            ));
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap();
            let mut process = Command::new("/bin/sh")
                .args(["-c", script])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            let mut input = process.stdin.take().unwrap();
            let mut child = RuntimeChild::new(process, file);
            assert!(child.try_wait().unwrap().is_none());
            assert!(std::fs::read_to_string(&path).unwrap().is_empty());
            writeln!(input, "exit").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(status.code(), code);
            assert_eq!(status.signal(), signal);
            assert!(!std::path::Path::new(&format!("/proc/{}", child.id())).exists());
            assert_eq!(child.wait().unwrap(), status);
            drop(child);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                format!("{}\n", status.into_raw())
            );
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn an_explicit_stop_also_records_the_signal() {
        let path = std::env::temp_dir().join(format!(
            "muniment-runtime-receipt-{}-stop",
            std::process::id()
        ));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let process = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let mut child = RuntimeChild::new(process, file);
        child.kill().unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(9));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "9\n");
        std::fs::remove_file(path).unwrap();
    }
}
