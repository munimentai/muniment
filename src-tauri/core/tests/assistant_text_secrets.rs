use muniment_core::assistant_text::{scan_secrets, SecretRule};

fn only_match(text: &[u8], rule: SecretRule) {
    let result = scan_secrets(text, true);
    assert_eq!(result.matches.len(), 1, "{result:?}");
    assert_eq!(result.matches[0].range, 0..text.len());
    assert_eq!(result.matches[0].rule, rule);
    assert_eq!(result.retain_from, None);
    assert_eq!(result.withhold_from, None);
}

fn pem_line(action: &str, name: &str, ending: &str) -> String {
    format!("-----{action} {name}-----{ending}")
}

#[test]
fn matches_assignment_and_rejects_near_misses() {
    let assignment = b"API_KEY := 'abcdefghij'";
    let result = scan_secrets(assignment, true);
    assert_eq!(result.matches[0].range, 0..assignment.len() - 1);
    assert_eq!(result.matches[0].rule, SecretRule::Assignment);
    assert!(scan_secrets(b"xapi_key=abcdefghij", true)
        .matches
        .is_empty());
    assert!(scan_secrets(b"api_key=abcdefghi", true).matches.is_empty());
    assert!(scan_secrets(b"api_key=abcdefghij!", true)
        .matches
        .is_empty());

    let maximum = format!("key{}={}", ".".repeat(20), "a".repeat(150));
    assert_eq!(maximum.len(), 174);
    only_match(maximum.as_bytes(), SecretRule::Assignment);
    assert!(scan_secrets(format!("{maximum}a").as_bytes(), true)
        .matches
        .is_empty());

    let at_ceiling = format!("authorization{}:='''''{}", ".".repeat(20), "a".repeat(152));
    assert_eq!(at_ceiling.len(), 192);
    assert_eq!(
        scan_secrets(at_ceiling.as_bytes(), false).withhold_from,
        Some(0)
    );
    assert_eq!(
        scan_secrets(format!("{at_ceiling}a").as_bytes(), false).withhold_from,
        Some(0)
    );
}

#[test]
fn matches_every_provider_family_and_boundaries() {
    let mut tokens = vec![
        format!("github_pat_{}", "a_".repeat(41)),
        format!("sk-{}", "a".repeat(20)),
        format!("sk-ant-{}", "a".repeat(20)),
        format!("sk_live_{}", "a".repeat(16)),
        format!("rk_live_{}", "a".repeat(16)),
        format!("hf_{}", "a".repeat(20)),
        format!("AKIA{}", "A1".repeat(8)),
        format!("ASIA{}", "A1".repeat(8)),
    ];
    for kind in b"pousr" {
        tokens.push(format!("gh{}_{}", *kind as char, "a".repeat(36)));
    }
    for kind in b"baprs" {
        tokens.push(format!("xox{}-{}", *kind as char, "a-".repeat(5)));
    }
    for token in tokens {
        only_match(token.as_bytes(), SecretRule::ProviderToken);
    }
    assert!(
        scan_secrets(format!("_sk-{}", "a".repeat(20)).as_bytes(), true)
            .matches
            .is_empty()
    );
    assert!(
        scan_secrets(format!("sk-{}", "a".repeat(19)).as_bytes(), true)
            .matches
            .is_empty()
    );

    only_match(
        format!("sk-{}", "a".repeat(509)).as_bytes(),
        SecretRule::ProviderToken,
    );
    assert_eq!(
        scan_secrets(format!("sk-{}", "a".repeat(510)).as_bytes(), true).withhold_from,
        Some(0)
    );
}

#[test]
fn matches_jwt_limits_and_rejects_near_misses() {
    only_match(
        format!("{}.{}.", "a".repeat(17), "b".repeat(17)).as_bytes(),
        SecretRule::Jwt,
    );

    let padded = format!("{}.{}.c===", "a".repeat(17), "b".repeat(17));
    let padded_result = scan_secrets(padded.as_bytes(), true);
    assert_eq!(padded_result.matches[0].range, 0..padded.len() - 1);
    assert_eq!(padded_result.withhold_from, None);

    let invalid_suffix = format!("{}.{}.c==d", "a".repeat(17), "b".repeat(17));
    let invalid_result = scan_secrets(invalid_suffix.as_bytes(), true);
    assert!(invalid_result.matches.is_empty());
    assert_eq!(invalid_result.withhold_from, None);

    let before_ceiling = format!(
        "{}.{}.{}",
        "a".repeat(2726),
        "b".repeat(2726),
        "c".repeat(2737)
    );
    assert_eq!(before_ceiling.len(), 8191);
    assert_eq!(
        scan_secrets(before_ceiling.as_bytes(), false).withhold_from,
        None
    );
    let at_ceiling = format!("{before_ceiling}c");
    assert_eq!(at_ceiling.len(), 8192);
    assert_eq!(
        scan_secrets(at_ceiling.as_bytes(), false).withhold_from,
        Some(0)
    );
    assert_eq!(
        scan_secrets(format!("{at_ceiling}c").as_bytes(), false).withhold_from,
        Some(0)
    );
    assert!(scan_secrets(
        format!("{}.{}.", "a".repeat(16), "b".repeat(17)).as_bytes(),
        true
    )
    .matches
    .is_empty());
    assert!(scan_secrets(
        format!("_{}.{}.", "a".repeat(2726), "b".repeat(17)).as_bytes(),
        true
    )
    .matches
    .is_empty());
    only_match(
        format!(
            "{}.{}.{}==",
            "a".repeat(2726),
            "b".repeat(2726),
            "c".repeat(2724)
        )
        .as_bytes(),
        SecretRule::Jwt,
    );
}

#[test]
fn matches_pem_names_line_endings_and_span_boundary() {
    for name in [
        "PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "RSA PRIVATE KEY",
        "DSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
    ] {
        let pem = format!(
            "{}YQ==\r\n{}",
            pem_line("BEGIN", name, "\r\n"),
            pem_line("END", name, "")
        );
        only_match(pem.as_bytes(), SecretRule::PemPrivateKey);
    }
    let prefixed = format!(
        "x{}YQ==\n{}",
        pem_line("BEGIN", "PRIVATE KEY", "\n"),
        pem_line("END", "PRIVATE KEY", "")
    );
    assert!(scan_secrets(prefixed.as_bytes(), true).matches.is_empty());
    let public = format!(
        "{}YQ==\n{}",
        pem_line("BEGIN", "PUBLIC KEY", "\n"),
        pem_line("END", "PUBLIC KEY", "")
    );
    assert!(scan_secrets(public.as_bytes(), true).matches.is_empty());

    let prefix = pem_line("BEGIN", "PRIVATE KEY", "\n");
    let suffix = pem_line("END", "PRIVATE KEY", "");
    let pem = format!("{prefix}{}{suffix}", "A".repeat(65_460));
    only_match(pem.as_bytes(), SecretRule::PemPrivateKey);
    assert_eq!(
        scan_secrets(format!("{prefix}{}", "A".repeat(65_461)).as_bytes(), false).withhold_from,
        Some(0)
    );

    let ceiling_prefix = pem_line("BEGIN", "ENCRYPTED PRIVATE KEY", "\r\n");
    let ceiling_suffix = pem_line("END", "ENCRYPTED PRIVATE KEY", "\r\n");
    let ceiling = format!("{ceiling_prefix}{}{ceiling_suffix}", "A".repeat(65_460));
    assert_eq!(ceiling.len(), 65_536);
    only_match(ceiling.as_bytes(), SecretRule::PemPrivateKey);
    assert_eq!(
        scan_secrets(
            format!("{ceiling_prefix}{}{ceiling_suffix}", "A".repeat(65_461)).as_bytes(),
            false
        )
        .withhold_from,
        Some(0)
    );
}

#[test]
fn longest_match_wins_and_rule_order_breaks_a_tie() {
    let text = format!("token=sk-{}", "a".repeat(20));
    let result = scan_secrets(text.as_bytes(), true);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].rule, SecretRule::Assignment);
    assert_eq!(result.matches[0].range, 0..text.len());

    let token = format!("sk-{}", "a".repeat(20));
    let result = scan_secrets(token.as_bytes(), true);
    assert_eq!(result.matches[0].rule, SecretRule::ProviderToken);
}

#[test]
fn incomplete_scan_retains_the_earliest_pending_candidate() {
    assert_eq!(scan_secrets(b"ordinary text ", false).retain_from, Some(14));
    assert_eq!(scan_secrets(b"ordinary sk", false).retain_from, Some(9));
    assert_eq!(
        scan_secrets(b"sk-aaaaaaaaaaaaaaaaaaaa ", false).retain_from,
        Some(24)
    );

    let boundary = b"ordinary \xc3";
    assert_eq!(scan_secrets(boundary, false).retain_from, Some(9));
    assert_eq!(scan_secrets(b"sk-", true).retain_from, None);
}

#[test]
fn missing_bounded_terminators_withhold_the_remainder() {
    let provider = format!("before sk-{}", "a".repeat(510));
    assert_eq!(
        scan_secrets(provider.as_bytes(), false).withhold_from,
        Some(7)
    );

    let pem = format!(
        "x\n{}{}",
        pem_line("BEGIN", "PRIVATE KEY", "\n"),
        "A".repeat(65_536)
    );
    assert_eq!(scan_secrets(pem.as_bytes(), false).withhold_from, Some(2));

    let short_pem = format!("x\n{}YQ==", pem_line("BEGIN", "PRIVATE KEY", "\n"));
    assert_eq!(
        scan_secrets(short_pem.as_bytes(), false).withhold_from,
        Some(2)
    );
    assert_eq!(
        scan_secrets(short_pem.as_bytes(), true).withhold_from,
        Some(2)
    );
}
