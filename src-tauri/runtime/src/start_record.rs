use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const FAILURE_LIMIT: usize = 5;
pub(crate) const FAILURE_WINDOW: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartDecision {
    Run,
    StopRestartLoop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Start {
    timestamp_millis: u64,
}

#[derive(Default)]
struct StartRecord {
    failures: Vec<u64>,
    needs_attention: bool,
}

pub(crate) fn record_start(
    state_directory: impl AsRef<Path>,
    record_name: &str,
) -> io::Result<(StartDecision, Start)> {
    record_start_at(state_directory.as_ref(), record_name, SystemTime::now())
}

pub(crate) fn record_orderly_exit(
    state_directory: impl AsRef<Path>,
    record_name: &str,
    start: Start,
) -> io::Result<()> {
    let path = state_directory.as_ref().join(record_name);
    let mut record = read_record(&path)?;
    if let Some(index) = record
        .failures
        .iter()
        .rposition(|timestamp| *timestamp == start.timestamp_millis)
    {
        record.failures.remove(index);
    }
    write_record(&path, &record)
}

pub(crate) fn record_failed_exit(
    state_directory: impl AsRef<Path>,
    record_name: &str,
    start: Start,
) -> io::Result<StartDecision> {
    let path = state_directory.as_ref().join(record_name);
    let mut record = read_record(&path)?;
    let decision = if record.failures.contains(&start.timestamp_millis)
        && record.failures.len() >= FAILURE_LIMIT
    {
        record.failures.clear();
        record.needs_attention = true;
        StartDecision::StopRestartLoop
    } else {
        StartDecision::Run
    };
    write_record(&path, &record)?;
    Ok(decision)
}

fn record_start_at(
    state_directory: &Path,
    record_name: &str,
    now: SystemTime,
) -> io::Result<(StartDecision, Start)> {
    let timestamp_millis = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "system time is invalid"))?
        .as_millis()
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "system time is invalid"))?;
    record_start_millis(state_directory, record_name, timestamp_millis)
}

pub(crate) fn record_start_millis(
    state_directory: &Path,
    record_name: &str,
    timestamp_millis: u64,
) -> io::Result<(StartDecision, Start)> {
    fs::create_dir_all(state_directory)?;
    let path = state_directory.join(record_name);
    let mut record = read_record(&path)?;
    let window_millis: u64 = FAILURE_WINDOW.as_millis().try_into().unwrap();
    record.failures.retain(|failure| {
        *failure <= timestamp_millis && timestamp_millis - *failure < window_millis
    });
    let decision = if record.failures.len() >= FAILURE_LIMIT {
        record.failures.clear();
        record.needs_attention = true;
        StartDecision::StopRestartLoop
    } else {
        record.failures.push(timestamp_millis);
        StartDecision::Run
    };
    write_record(&path, &record)?;
    Ok((decision, Start { timestamp_millis }))
}

fn read_record(path: &Path) -> io::Result<StartRecord> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(StartRecord::default()),
        Err(error) => return Err(error),
    };
    let mut record = StartRecord::default();
    for line in contents.lines() {
        if line == "needs_attention=true" {
            record.needs_attention = true;
        } else if let Some(timestamp) = line.strip_prefix("failure=") {
            if let Ok(timestamp) = timestamp.parse() {
                record.failures.push(timestamp);
            }
        }
    }
    Ok(record)
}

fn write_record(path: &Path, record: &StartRecord) -> io::Result<()> {
    let temporary_path = temporary_path(path);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary_path)?;
    if record.needs_attention {
        writeln!(file, "needs_attention=true")?;
    }
    for timestamp in &record.failures {
        writeln!(file, "failure={timestamp}")?;
    }
    file.sync_all()?;
    muniment_core::atomic_file::replace(&temporary_path, path)
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension("tmp")
}
