use muniment_core::assistant_text::{scan, Rule};

fn token(prefix: &str, body: char, length: usize) -> String {
    format!("{prefix}{}", body.to_string().repeat(length))
}

fn pem(name: &str, line_ending: &str, body: &str) -> String {
    format!("-----BEGIN {name}-----{line_ending}{body}-----END {name}-----")
}

#[test]
fn matches_each_provider_alternative() {
    let cases = [
        ("ghp_", 'a', 36),
        ("gho_", 'a', 36),
        ("ghu_", 'a', 36),
        ("ghs_", 'a', 36),
        ("ghr_", 'a', 36),
        ("github_pat_", '_', 82),
        ("sk-", '_', 20),
        ("sk-ant-", '-', 20),
        ("xoxb-", '-', 10),
        ("xoxa-", '-', 10),
        ("xoxp-", '-', 10),
        ("xoxr-", '-', 10),
        ("xoxs-", '-', 10),
        ("sk_live_", 'a', 16),
        ("rk_live_", 'a', 16),
        ("hf_", 'a', 20),
        ("AKIA", 'A', 16),
        ("ASIA", '0', 16),
    ];
    for (prefix, body, length) in cases {
        let value = token(prefix, body, length);
        let result = scan(&value, true);
        assert_eq!(result.matches.len(), 1, "{prefix}");
        assert_eq!(result.matches[0].range, 0..value.len(), "{prefix}");
        assert_eq!(result.matches[0].rule, Rule::SecretProviderToken);
    }
}

#[test]
fn rejects_near_misses() {
    assert!(scan(&token("sk-", 'a', 19), true).matches.is_empty());
    assert!(scan(&format!("x{}", token("hf_", 'a', 20)), true)
        .matches
        .is_empty());
    assert!(scan("github_pat_aaaaaaaaaa!aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", true).matches.is_empty());
}

#[test]
fn accepts_each_length_ceiling_with_a_boundary_byte() {
    let cases = [
        ("ghp_", 'a', 508),
        ("github_pat_", '_', 501),
        ("sk-", '_', 509),
        ("sk-ant-", '-', 505),
        ("xoxb-", '-', 504),
        ("sk_live_", 'a', 504),
        ("hf_", 'a', 509),
        ("AKIA", 'A', 16),
    ];
    for (prefix, body, length) in cases {
        let value = format!("{}!", token(prefix, body, length));
        assert_eq!(
            scan(&value, true).matches[0].range,
            0..value.len() - 1,
            "{prefix}"
        );
    }
}

#[test]
fn overlapping_openai_and_anthropic_alternatives_make_one_match() {
    let value = token("sk-ant-", 'a', 20);
    let result = scan(&value, true);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].range, 0..value.len());
}

#[test]
fn reports_complete_and_incomplete_retention() {
    let content = format!("{}é", "x".repeat(65_536));
    let incomplete = scan(&content, false);
    assert!(incomplete.retention_offset > 0);
    assert!(content.is_char_boundary(incomplete.retention_offset));
    assert!(content.len() - incomplete.retention_offset <= 65_536);
    assert_eq!(scan(&content, true).retention_offset, content.len());
}

#[test]
fn withholds_an_over_span_candidate() {
    let content = format!("before {} after", token("sk-", 'a', 510));
    let result = scan(&content, true);
    assert_eq!(result.withhold_from, Some(7));
    assert!(result.matches.is_empty());
}

#[test]
fn returns_ordered_non_overlapping_ranges() {
    let first = token("hf_", 'a', 20);
    let second = token("AKIA", 'A', 16);
    let content = format!("{first} {second}");
    let result = scan(&content, true);
    assert_eq!(result.matches[0].range, 0..first.len());
    assert_eq!(result.matches[1].range, first.len() + 1..content.len());
}

#[test]
fn matches_each_pem_private_key_header() {
    let names = [
        "PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "RSA PRIVATE KEY",
        "DSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
    ];
    for name in names {
        let value = pem(name, "\n", "YQ==\n");
        let content = format!("before\n{value}\nafter");
        let result = scan(&content, true);
        assert_eq!(result.matches.len(), 1, "{name}");
        assert_eq!(result.matches[0].range, 7..7 + value.len(), "{name}");
        assert_eq!(result.matches[0].rule, Rule::SecretPemPrivateKey);
        assert_eq!(result.withhold_from, None);
    }
}

#[test]
fn accepts_crlf_pem_lines_and_excludes_the_final_line_ending() {
    let value = pem("PRIVATE KEY", "\r\n", "YQ==\r\n");
    let content = format!("{value}\r\nafter");
    let result = scan(&content, true);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].range, 0..value.len());
}

#[test]
fn enforces_pem_body_length_bounds() {
    let empty = pem("PRIVATE KEY", "\n", "");
    assert!(scan(&empty, true).matches.is_empty());

    let maximum = pem("ENCRYPTED PRIVATE KEY", "\r\n", &"A".repeat(65_460));
    let result = scan(&maximum, true);
    assert_eq!(result.matches[0].range, 0..maximum.len());

    let too_long = pem("PRIVATE KEY", "\n", &"A".repeat(65_461));
    assert!(scan(&too_long, true).matches.is_empty());
    assert_eq!(scan(&too_long, true).withhold_from, None);
}

#[test]
fn rejects_invalid_pem_private_keys() {
    let lone_cr = pem("PRIVATE KEY", "\n", "YQ==\r");
    let mismatched_end = "-----BEGIN PRIVATE KEY-----\nYQ==\n-----END RSA PRIVATE KEY-----";
    let without_line_boundary = format!("x{}", pem("PRIVATE KEY", "\n", "YQ==\n"));
    for value in [lone_cr.as_str(), mismatched_end, &without_line_boundary] {
        let result = scan(value, true);
        assert!(result.matches.is_empty(), "{value:?}");
        assert_eq!(result.withhold_from, None, "{value:?}");
    }
}

#[test]
fn incomplete_pem_private_key_waits_without_withholding() {
    let value = "-----BEGIN PRIVATE KEY-----\nYQ==\n-----END PRIVATE";
    for complete in [false, true] {
        let result = scan(value, complete);
        assert!(result.matches.is_empty());
        assert_eq!(result.withhold_from, None);
    }
}

#[test]
fn pem_private_key_contains_provider_token_as_one_match() {
    let value = pem("PRIVATE KEY", "\n", "AKIAAAAAAAAAAAAAAAAA\n");
    let result = scan(&value, true);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].range, 0..value.len());
    assert_eq!(result.matches[0].rule, Rule::SecretPemPrivateKey);
}
