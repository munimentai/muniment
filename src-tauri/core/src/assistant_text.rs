//! Pure byte matching for the secret rules in `assistant-text-v1`.

use std::ops::Range;

/// The greatest complete-match span among this module's four rules.
pub const MAX_SECRET_SPAN_BYTES: usize = 65_536;

/// A secret rule, in its arbitration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretRule {
    Assignment,
    ProviderToken,
    Jwt,
    PemPrivateKey,
}

/// One non-overlapping secret match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretMatch {
    pub range: Range<usize>,
    pub rule: SecretRule,
}

/// The result of scanning one assistant-content buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretScan {
    pub matches: Vec<SecretMatch>,
    /// The first byte a streaming caller must retain.
    pub retain_from: Option<usize>,
    /// The first byte to withhold through the end of the content.
    pub withhold_from: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    No,
    Pending,
    Match(usize),
    OverSpan,
}

type ProviderAlternative = (&'static [u8], usize, usize, fn(u8) -> bool);

const RULES: [SecretRule; 4] = [
    SecretRule::Assignment,
    SecretRule::ProviderToken,
    SecretRule::Jwt,
    SecretRule::PemPrivateKey,
];

/// Scans `bytes` as either complete content or the current streaming buffer.
pub fn scan_secrets(bytes: &[u8], complete: bool) -> SecretScan {
    let mut matches = Vec::new();
    let mut withhold_from = None;
    let mut earliest_pending = None;
    let mut offset = 0;

    while offset < bytes.len() {
        let mut best: Option<(usize, SecretRule)> = None;
        let mut over_span = false;

        for rule in RULES {
            match match_rule(rule, bytes, offset, complete) {
                State::Match(end) => {
                    if best.is_none_or(|(best_end, _)| end > best_end) {
                        best = Some((end, rule));
                    }
                }
                State::Pending => {
                    earliest_pending.get_or_insert(offset);
                }
                State::OverSpan => over_span = true,
                State::No => {}
            }
        }

        if over_span {
            withhold_from = Some(offset);
            break;
        }
        if let Some((end, rule)) = best {
            matches.push(SecretMatch {
                range: offset..end,
                rule,
            });
            offset = end;
            continue;
        }
        offset += 1;
    }

    let retain_from = if complete || withhold_from.is_some() {
        None
    } else {
        Some(utf8_boundary_at_or_before(
            bytes,
            earliest_pending.unwrap_or(bytes.len()),
        ))
    };

    SecretScan {
        matches,
        retain_from,
        withhold_from,
    }
}

fn match_rule(rule: SecretRule, bytes: &[u8], start: usize, complete: bool) -> State {
    match rule {
        SecretRule::Assignment => assignment(bytes, start, complete),
        SecretRule::ProviderToken => provider(bytes, start, complete),
        SecretRule::Jwt => jwt(bytes, start, complete),
        SecretRule::PemPrivateKey => pem(bytes, start, complete),
    }
}

fn assignment(bytes: &[u8], start: usize, complete: bool) -> State {
    const LABELS: [&[u8]; 12] = [
        b"key",
        b"api_key",
        b"apikey",
        b"api-token",
        b"token",
        b"secret",
        b"client_secret",
        b"passwd",
        b"password",
        b"auth",
        b"authorization",
        b"access_token",
    ];
    if start > 0 && is_ident(bytes[start - 1]) {
        return State::No;
    }

    let mut pending = false;
    let mut best = None;
    let mut over_span = false;
    for label in LABELS {
        let available = &bytes[start..];
        let compared = available.len().min(label.len());
        if !available[..compared].eq_ignore_ascii_case(&label[..compared]) {
            continue;
        }
        if available.len() < label.len() {
            pending |= !complete;
            continue;
        }
        let after_label = start + label.len();
        if after_label < bytes.len() && is_ident(bytes[after_label]) {
            continue;
        }
        for gap in 0..=20 {
            let delimiter_at = after_label + gap;
            if delimiter_at > bytes.len() {
                pending |= !complete;
                break;
            }
            if gap > 0 && !is_assignment_gap(bytes[delimiter_at - 1]) {
                break;
            }
            for delimiter in [
                b":=".as_slice(),
                b"=>",
                b"<=",
                b"?=",
                b"||",
                b"=",
                b">",
                b":",
                b",",
            ] {
                let Some(after_delimiter) =
                    literal_end(bytes, delimiter_at, delimiter, complete, &mut pending)
                else {
                    continue;
                };
                for padding in 0..=5 {
                    let value_at = after_delimiter + padding;
                    if value_at > bytes.len() {
                        pending |= !complete;
                        break;
                    }
                    if padding > 0 && !is_assignment_padding(bytes[value_at - 1]) {
                        break;
                    }
                    let mut end = value_at;
                    while end < bytes.len() && is_value(bytes[end]) {
                        end += 1;
                    }
                    let length = end - value_at;
                    if length < 10 {
                        if !complete && end == bytes.len() {
                            pending = true;
                        }
                        continue;
                    }
                    if length > 150 {
                        over_span |= end - start >= 192;
                        continue;
                    }
                    if end == bytes.len() && !complete {
                        pending = true;
                        continue;
                    }
                    if end < bytes.len() && !is_assignment_end(bytes[end]) {
                        continue;
                    }
                    if end - start <= 192 {
                        best = Some(best.map_or(end, |old: usize| old.max(end)));
                    }
                }
            }
        }
    }
    if let Some(end) = best {
        State::Match(end)
    } else if over_span {
        State::OverSpan
    } else if pending {
        State::Pending
    } else {
        State::No
    }
}

fn provider(bytes: &[u8], start: usize, complete: bool) -> State {
    if start > 0 && is_alnum_underscore(bytes[start - 1]) {
        return State::No;
    }
    let alternatives: [ProviderAlternative; 13] = [
        (b"ghp_", 36, 508, is_alnum),
        (b"gho_", 36, 508, is_alnum),
        (b"ghu_", 36, 508, is_alnum),
        (b"ghs_", 36, 508, is_alnum),
        (b"ghr_", 36, 508, is_alnum),
        (b"github_pat_", 82, 501, is_alnum_underscore),
        (b"sk-ant-", 20, 505, is_b64),
        (b"sk-", 20, 509, is_b64),
        (b"xoxb-", 10, 504, is_alnum_hyphen),
        (b"xoxa-", 10, 504, is_alnum_hyphen),
        (b"xoxp-", 10, 504, is_alnum_hyphen),
        (b"xoxr-", 10, 504, is_alnum_hyphen),
        (b"xoxs-", 10, 504, is_alnum_hyphen),
    ];
    let mut pending = false;
    let mut best = None;
    let mut over = false;
    for (prefix, min, max, alphabet) in alternatives {
        provider_alternative(
            bytes,
            start,
            complete,
            prefix,
            min,
            max,
            alphabet,
            &mut pending,
            &mut best,
            &mut over,
        );
    }
    for prefix in [b"sk_live_".as_slice(), b"rk_live_"] {
        provider_alternative(
            bytes,
            start,
            complete,
            prefix,
            16,
            504,
            is_alnum,
            &mut pending,
            &mut best,
            &mut over,
        );
    }
    provider_alternative(
        bytes,
        start,
        complete,
        b"hf_",
        20,
        509,
        is_alnum,
        &mut pending,
        &mut best,
        &mut over,
    );
    provider_alternative(
        bytes,
        start,
        complete,
        b"AKIA",
        16,
        16,
        is_upper_digit,
        &mut pending,
        &mut best,
        &mut over,
    );
    provider_alternative(
        bytes,
        start,
        complete,
        b"ASIA",
        16,
        16,
        is_upper_digit,
        &mut pending,
        &mut best,
        &mut over,
    );
    if over {
        State::OverSpan
    } else if let Some(end) = best {
        State::Match(end)
    } else if pending {
        State::Pending
    } else {
        State::No
    }
}

#[allow(clippy::too_many_arguments)]
fn provider_alternative(
    bytes: &[u8],
    start: usize,
    complete: bool,
    prefix: &[u8],
    min: usize,
    max: usize,
    alphabet: fn(u8) -> bool,
    pending: &mut bool,
    best: &mut Option<usize>,
    over: &mut bool,
) {
    let available = &bytes[start..];
    let compared = available.len().min(prefix.len());
    if available[..compared] != prefix[..compared] {
        return;
    }
    if available.len() < prefix.len() {
        *pending |= !complete;
        return;
    }
    let value_at = start + prefix.len();
    let mut end = value_at;
    while end < bytes.len() && alphabet(bytes[end]) {
        end += 1;
    }
    let length = end - value_at;
    if length < min {
        *pending |= !complete && end == bytes.len();
        return;
    }
    if length > max {
        *over = true;
        return;
    }
    if end == bytes.len() && !complete {
        *pending = true;
        return;
    }
    *best = Some(best.map_or(end, |old| old.max(end)));
}

fn jwt(bytes: &[u8], start: usize, complete: bool) -> State {
    if start > 0 && is_b64(bytes[start - 1]) {
        return State::No;
    }
    let mut at = start;
    for segment in 0..3 {
        let segment_start = at;
        let max = if segment < 2 { 2726 } else { 2724 };
        while at < bytes.len() && is_b64(bytes[at]) && at - segment_start <= max {
            at += 1;
        }
        let length = at - segment_start;
        let min = if segment < 2 { 17 } else { 0 };
        if length > max {
            if segment == 2 {
                while at < bytes.len() && is_b64(bytes[at]) {
                    at += 1;
                }
                if at - start >= 8192 {
                    return State::OverSpan;
                }
            }
            return State::No;
        }
        if length < min {
            return if !complete && at == bytes.len() {
                State::Pending
            } else {
                State::No
            };
        }
        if segment < 2 {
            if at == bytes.len() {
                return if complete { State::No } else { State::Pending };
            }
            if bytes[at] != b'.' {
                return State::No;
            }
            at += 1;
        }
    }
    let mut padding = 0;
    while at < bytes.len() && bytes[at] == b'=' && padding < 2 {
        at += 1;
        padding += 1;
    }
    if at - start > 8192 {
        return State::OverSpan;
    }
    if at < bytes.len() && is_b64(bytes[at]) {
        return if at - start >= 8192 {
            State::OverSpan
        } else {
            State::No
        };
    }
    if at == bytes.len() && !complete {
        return State::Pending;
    }
    State::Match(at)
}

fn pem(bytes: &[u8], start: usize, complete: bool) -> State {
    const PREFIX: &[u8] = b"-----BEGIN ";
    const NAMES: [&[u8]; 6] = [
        b"PRIVATE KEY",
        b"ENCRYPTED PRIVATE KEY",
        b"RSA PRIVATE KEY",
        b"DSA PRIVATE KEY",
        b"EC PRIVATE KEY",
        b"OPENSSH PRIVATE KEY",
    ];
    if start > 0 && bytes[start - 1] != b'\n' {
        return State::No;
    }
    let mut pending = false;
    let Some(after_prefix) = literal_end(bytes, start, PREFIX, complete, &mut pending) else {
        return if pending { State::Pending } else { State::No };
    };
    for name in NAMES {
        let mut name_pending = false;
        let Some(after_name) = literal_end(bytes, after_prefix, name, complete, &mut name_pending)
        else {
            pending |= name_pending;
            continue;
        };
        let mut marker_pending = false;
        let Some(mut body_at) =
            literal_end(bytes, after_name, b"-----", complete, &mut marker_pending)
        else {
            pending |= marker_pending;
            continue;
        };
        if body_at == bytes.len() {
            pending |= !complete;
            continue;
        }
        if bytes[body_at] == b'\n' {
            body_at += 1;
        } else if bytes[body_at] == b'\r' {
            if body_at + 1 == bytes.len() {
                pending |= !complete;
                continue;
            }
            if bytes[body_at + 1] != b'\n' {
                continue;
            }
            body_at += 2;
        } else {
            continue;
        }
        if body_at == bytes.len() {
            return State::OverSpan;
        }
        let end_marker = [b"-----END ".as_slice(), name, b"-----"].concat();
        let mut at = body_at;
        while at < bytes.len() && at - start <= 65_536 {
            if at - body_at > 65_460 {
                return State::OverSpan;
            }
            if bytes[at..].starts_with(&end_marker) && at > body_at {
                let end = at + end_marker.len();
                if end > bytes.len() {
                    break;
                }
                if end == bytes.len() {
                    return if complete {
                        State::Match(end)
                    } else {
                        State::Pending
                    };
                }
                let final_end = if bytes[end] == b'\n' {
                    end + 1
                } else if bytes[end] == b'\r' && end + 1 < bytes.len() && bytes[end + 1] == b'\n' {
                    end + 2
                } else if bytes[end] == b'\r' && end + 1 == bytes.len() && !complete {
                    return State::Pending;
                } else {
                    at += 1;
                    continue;
                };
                if final_end - start <= 65_536 {
                    return State::Match(final_end);
                }
                return State::OverSpan;
            }
            if at > body_at && !complete && end_marker.starts_with(&bytes[at..]) {
                return State::Pending;
            }
            if !is_pem_body(bytes[at])
                || (bytes[at] == b'\r'
                    && (at + 1 == bytes.len() || bytes.get(at + 1) != Some(&b'\n')))
            {
                return if !complete && at + 1 == bytes.len() && bytes[at] == b'\r' {
                    State::Pending
                } else {
                    State::No
                };
            }
            at += 1;
        }
        if at - body_at > 65_460 {
            return State::OverSpan;
        }
        if at - start >= 65_536 {
            return State::OverSpan;
        }
        return State::OverSpan;
    }
    if pending {
        State::Pending
    } else {
        State::No
    }
}

fn literal_end(
    bytes: &[u8],
    at: usize,
    literal: &[u8],
    complete: bool,
    pending: &mut bool,
) -> Option<usize> {
    let available = bytes.len().saturating_sub(at);
    let compared = available.min(literal.len());
    if bytes.get(at..at + compared)? != &literal[..compared] {
        return None;
    }
    if available < literal.len() {
        *pending |= !complete;
        None
    } else {
        Some(at + literal.len())
    }
}

fn utf8_boundary_at_or_before(bytes: &[u8], offset: usize) -> usize {
    std::str::from_utf8(&bytes[..offset])
        .map(|_| offset)
        .unwrap_or_else(|error| error.valid_up_to())
}

fn is_alnum(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}
fn is_alnum_underscore(byte: u8) -> bool {
    is_alnum(byte) || byte == b'_'
}
fn is_alnum_hyphen(byte: u8) -> bool {
    is_alnum(byte) || byte == b'-'
}
fn is_upper_digit(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit()
}
fn is_b64(byte: u8) -> bool {
    is_alnum(byte) || matches!(byte, b'_' | b'-')
}
fn is_ident(byte: u8) -> bool {
    is_b64(byte)
}
fn is_value(byte: u8) -> bool {
    is_alnum(byte) || matches!(byte, b'_' | b'.' | b'/' | b'+' | b'=' | b'-')
}
fn is_assignment_gap(byte: u8) -> bool {
    is_alnum(byte) || matches!(byte, b'_' | b'.' | b' ' | b'\t' | b'-')
}
fn is_assignment_padding(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'=' | b'\'' | b'"' | b'`')
}
fn is_assignment_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'\'' | b'"' | b'`' | b';' | b'\\')
}
fn is_pem_body(byte: u8) -> bool {
    is_alnum(byte) || matches!(byte, b'+' | b'/' | b'=' | b'\n' | b'\r')
}
