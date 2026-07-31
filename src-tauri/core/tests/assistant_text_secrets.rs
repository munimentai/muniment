use muniment_core::assistant_text::{scan, Rule};

fn token(prefix: &str, body: char, length: usize) -> String {
    format!("{prefix}{}", body.to_string().repeat(length))
}

fn pem(name: &str, line_ending: &str, body: &str) -> String {
    format!("-----BEGIN {name}-----{line_ending}{body}-----END {name}-----{line_ending}")
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
    let content = format!("{}é", "x".repeat(600));
    let incomplete = scan(&content, false);
    assert!(content.is_char_boundary(incomplete.retention_offset));
    assert_eq!(incomplete.retention_offset, 0);
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
fn matches_each_pem_private_key_name() {
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
        let result = scan(&value, true);
        assert_eq!(result.matches.len(), 1, "{name}");
        assert_eq!(result.matches[0].range, 0..value.len(), "{name}");
        assert_eq!(result.matches[0].rule, Rule::SecretPemPrivateKey);
    }
}

#[test]
fn matches_pem_with_crlf_lines_after_a_lf_boundary() {
    let key = pem("PRIVATE KEY", "\r\n", "YQ==\r\nYg==\r\n");
    let content = format!("before\n{key}after");
    let result = scan(&content, true);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].range, 7..7 + key.len());
}

#[test]
fn rejects_pem_without_a_lf_boundary_or_with_a_different_end_name() {
    let key = pem("PRIVATE KEY", "\n", "YQ==\n");
    assert!(scan(&format!("x{key}"), true).matches.is_empty());

    let mismatched = key.replace("END PRIVATE KEY", "END RSA PRIVATE KEY");
    assert!(scan(&mismatched, true).matches.is_empty());
}

#[test]
fn rejects_a_lone_cr_in_a_pem_body() {
    let value = pem("PRIVATE KEY", "\n", "YQ==\rYg==\n");
    let result = scan(&value, true);
    assert!(result.matches.is_empty());
    assert_eq!(result.withhold_from, None);
}

#[test]
fn rejects_an_empty_pem_body_and_waits_for_an_incomplete_end_line() {
    let empty = pem("PRIVATE KEY", "\n", "");
    assert!(scan(&empty, true).matches.is_empty());

    let without_final_line_ending = pem("PRIVATE KEY", "\n", "YQ==\n")
        .trim_end_matches('\n')
        .to_owned();
    assert!(scan(&without_final_line_ending, false).matches.is_empty());
    assert_eq!(scan(&without_final_line_ending, true).matches.len(), 1);
}

#[test]
fn withholds_an_over_span_pem_candidate() {
    let begin = "-----BEGIN PRIVATE KEY-----\n";
    let content = format!("before\n{begin}{}", "a".repeat(65_461));
    let result = scan(&content, true);
    assert!(result.matches.is_empty());
    assert_eq!(result.withhold_from, Some(7));
}

#[test]
fn incomplete_pem_waits_inside_its_span_and_retains_the_maximum_suffix() {
    let begin = "-----BEGIN PRIVATE KEY-----\n";
    let content = format!("é{}{begin}YQ==\n", "x".repeat(65_534));
    let result = scan(&content, false);
    assert!(result.matches.is_empty());
    assert_eq!(result.withhold_from, None);
    assert!(content.is_char_boundary(result.retention_offset));
    assert_eq!(content.len() - result.retention_offset, 65_535);

    let scalar_overlap = format!("é{}", "x".repeat(65_534));
    let overlap_result = scan(&scalar_overlap, false);
    assert_eq!(overlap_result.retention_offset, 0);
    assert!(scalar_overlap.is_char_boundary(overlap_result.retention_offset));
}
