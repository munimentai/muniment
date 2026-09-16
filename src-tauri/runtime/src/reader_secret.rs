//! Where a network source's secret lives: the platform keychain under one
//! service name, an entry per source. A checkout or a test sets
//! `MUNIMENT_READER_SECRET_<SOURCE>` instead, and an empty value there reads
//! as no secret, so a test can prove the unconnected path.

use muniment_core::keyring::Entry;

const SERVICE: &str = "ai.muniment.reader";

fn variable(source: &str) -> String {
    format!("MUNIMENT_READER_SECRET_{}", source.to_ascii_uppercase())
}

/// The secret for a source, or none.
pub fn read(source: &str) -> Option<String> {
    if let Ok(value) = std::env::var(variable(source)) {
        return (!value.is_empty()).then_some(value);
    }
    Entry::new(SERVICE, source)
        .ok()?
        .get_password()
        .ok()
        .filter(|secret| !secret.is_empty())
}

/// Stores the secret for a source, replacing the one there.
pub fn write(source: &str, secret: &str) -> Result<(), String> {
    if std::env::var_os(variable(source)).is_some() {
        std::env::set_var(variable(source), secret);
        return Ok(());
    }
    Entry::new(SERVICE, source)
        .and_then(|entry| entry.set_password(secret))
        .map_err(|error| format!("The keychain refused the secret: {error}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_stands_in_for_the_keychain() {
        std::env::set_var("MUNIMENT_READER_SECRET_TESTSRC", "");
        assert_eq!(read("testsrc"), None);
        write("testsrc", "sk_1").unwrap();
        assert_eq!(read("testsrc").as_deref(), Some("sk_1"));
        write("testsrc", "").unwrap();
        assert_eq!(read("testsrc"), None);
        std::env::remove_var("MUNIMENT_READER_SECRET_TESTSRC");
    }
}
