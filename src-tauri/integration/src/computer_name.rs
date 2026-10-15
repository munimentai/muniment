//! The name this computer shows other devices: in a pairing link it shares
//! and in the device list of a runtime it pairs with.

/// The host name without its `.local` suffix, or `Desktop` when it is not a
/// valid display name.
pub fn computer_name() -> String {
    let name = host_name().unwrap_or_default();
    let name = name.trim().trim_end_matches(".local").trim();
    if muniment_core::attach::is_valid_display_name(name) {
        name.to_owned()
    } else {
        "Desktop".to_owned()
    }
}

#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0_u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed with it.
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0)?;
    String::from_utf8(buffer[..end].to_vec()).ok()
}

#[cfg(not(unix))]
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_computer_name_is_a_valid_display_name() {
        assert!(muniment_core::attach::is_valid_display_name(
            &super::computer_name()
        ));
    }
}
