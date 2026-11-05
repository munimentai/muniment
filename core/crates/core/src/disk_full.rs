//! Plain words for a write that fails because the disk is full.

use std::io;

/// What a person reads when a write fails because the disk is full.
pub const DISK_FULL_MESSAGE: &str = "The disk is full. Free some space, then try again.";

/// A storage error that can say whether the disk was full.
pub trait DiskFull {
    /// Whether the disk or the user's quota had no space left.
    fn is_disk_full(&self) -> bool;
}

impl DiskFull for io::Error {
    fn is_disk_full(&self) -> bool {
        matches!(
            self.kind(),
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded
        )
    }
}

impl DiskFull for rusqlite::Error {
    fn is_disk_full(&self) -> bool {
        self.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull)
    }
}

/// The disk-full message for a full disk, else `text`.
pub fn or_message(error: &impl DiskFull, text: &str) -> String {
    if error.is_disk_full() {
        DISK_FULL_MESSAGE.into()
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_disk_reads_as_plain_words() {
        #[cfg(unix)]
        let full = io::Error::from_raw_os_error(28);
        #[cfg(windows)]
        let full = io::Error::from_raw_os_error(112);
        assert_eq!(or_message(&full, "The save failed."), DISK_FULL_MESSAGE);
        let quota = io::Error::from(io::ErrorKind::QuotaExceeded);
        assert_eq!(or_message(&quota, "The save failed."), DISK_FULL_MESSAGE);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(or_message(&denied, "The save failed."), "The save failed.");
        let sqlite = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
            None,
        );
        assert_eq!(or_message(&sqlite, "Unreadable."), DISK_FULL_MESSAGE);
    }
}
