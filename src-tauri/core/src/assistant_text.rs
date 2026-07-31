//! Pure scanning for the `assistant-text-v1` assistant reply rules.

use std::ops::Range;

const MAX_IMPLEMENTED_SPAN: usize = 65_536;
const MAX_PEM_BODY: usize = 65_460;

const PEM_NAMES: [&[u8]; 6] = [
    b"PRIVATE KEY",
    b"ENCRYPTED PRIVATE KEY",
    b"RSA PRIVATE KEY",
    b"DSA PRIVATE KEY",
    b"EC PRIVATE KEY",
    b"OPENSSH PRIVATE KEY",
];

/// A rule in the `assistant-text-v1` rule set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A provider credential such as a GitHub or OpenAI token.
    SecretProviderToken,
    /// A PEM-encoded private key.
    SecretPemPrivateKey,
}

/// One non-overlapping byte range selected by the scanner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub range: Range<usize>,
    pub rule: Rule,
}

/// The result of scanning the content available so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub matches: Vec<Match>,
    /// The first byte a streaming caller must retain for the next scan.
    pub retention_offset: usize,
    /// The first byte to withhold through the end after an over-span candidate.
    pub withhold_from: Option<usize>,
}

#[derive(Clone, Copy)]
struct Alternative {
    prefix: &'static [u8],
    min_body: usize,
    max_body: usize,
    alphabet: fn(u8) -> bool,
}

const ALTERNATIVES: [Alternative; 15] = [
    alternative(b"ghp_", 36, 508, is_alnum),
    alternative(b"gho_", 36, 508, is_alnum),
    alternative(b"ghu_", 36, 508, is_alnum),
    alternative(b"ghs_", 36, 508, is_alnum),
    alternative(b"ghr_", 36, 508, is_alnum),
    alternative(b"github_pat_", 82, 501, is_alnum_underscore),
    alternative(b"sk-", 20, 509, is_b64),
    alternative(b"sk-ant-", 20, 505, is_b64),
    alternative(b"xoxb-", 10, 504, is_alnum_hyphen),
    alternative(b"xoxa-", 10, 504, is_alnum_hyphen),
    alternative(b"xoxp-", 10, 504, is_alnum_hyphen),
    alternative(b"xoxr-", 10, 504, is_alnum_hyphen),
    alternative(b"xoxs-", 10, 504, is_alnum_hyphen),
    alternative(b"sk_live_", 16, 504, is_alnum),
    alternative(b"rk_live_", 16, 504, is_alnum),
];

const fn alternative(
    prefix: &'static [u8],
    min_body: usize,
    max_body: usize,
    alphabet: fn(u8) -> bool,
) -> Alternative {
    Alternative {
        prefix,
        min_body,
        max_body,
        alphabet,
    }
}

/// Scans assistant reply content under the `assistant-text-v1` rules.
///
/// `complete` states that no later bytes can extend the supplied content.
pub fn scan(content: &str, complete: bool) -> Scan {
    let bytes = content.as_bytes();
    let mut matches = Vec::new();
    let mut withhold_from = None;
    let mut start = 0;

    while start < bytes.len() {
        {
            let mut selected = None;
            let mut over_span = false;
            if has_start_boundary(bytes, start) {
                for alternative in alternatives_at(bytes, start) {
                    match scan_alternative(bytes, start, alternative, complete) {
                        Candidate::Matched(end) => {
                            select_match(&mut selected, end, Rule::SecretProviderToken)
                        }
                        Candidate::OverSpan => over_span = true,
                        Candidate::None => {}
                    }
                }
            }
            if has_line_start(bytes, start) {
                match scan_pem(bytes, start, complete) {
                    Candidate::Matched(end) => {
                        select_match(&mut selected, end, Rule::SecretPemPrivateKey)
                    }
                    Candidate::OverSpan => over_span = true,
                    Candidate::None => {}
                }
            }
            if over_span {
                withhold_from = Some(start);
                break;
            }
            if let Some((end, rule)) = selected {
                matches.push(Match {
                    range: start..end,
                    rule,
                });
                start = end;
                continue;
            }
        }
        start += 1;
    }

    let retention_offset = if complete {
        content.len()
    } else {
        scalar_boundary_at_or_before(
            content,
            content.len().saturating_sub(MAX_IMPLEMENTED_SPAN - 1),
        )
    };
    Scan {
        matches,
        retention_offset,
        withhold_from,
    }
}

fn select_match(selected: &mut Option<(usize, Rule)>, end: usize, rule: Rule) {
    if selected.is_none_or(|(current_end, _)| end > current_end) {
        *selected = Some((end, rule));
    }
}

fn scan_pem(bytes: &[u8], start: usize, complete: bool) -> Candidate {
    const BEGIN: &[u8] = b"-----BEGIN ";
    if !bytes[start..].starts_with(BEGIN) {
        return Candidate::None;
    }
    let after_begin = &bytes[start + BEGIN.len()..];
    let Some(name) = PEM_NAMES.into_iter().find(|name| {
        after_begin.starts_with(name)
            && after_begin.get(name.len()..name.len() + 5) == Some(b"-----")
    }) else {
        return Candidate::None;
    };
    let header_end = start + BEGIN.len() + name.len() + 5;
    let Some(body_start) = line_ending_end(bytes, header_end) else {
        return Candidate::None;
    };
    let end_line = [b"-----END ".as_slice(), name, b"-----"].concat();
    let mut cursor = body_start;

    loop {
        if bytes[cursor..].starts_with(&end_line) {
            let line_end = cursor + end_line.len();
            let match_end = if line_end == bytes.len() && complete {
                line_end
            } else if line_end == bytes.len() {
                return Candidate::OverSpan;
            } else if let Some(after_ending) = line_ending_end(bytes, line_end) {
                after_ending
            } else if !complete
                && bytes.get(line_end) == Some(&b'\r')
                && line_end + 1 == bytes.len()
            {
                return Candidate::OverSpan;
            } else {
                return Candidate::None;
            };
            if cursor == body_start {
                return Candidate::None;
            }
            if match_end - start <= MAX_IMPLEMENTED_SPAN {
                return Candidate::Matched(match_end);
            }
            return Candidate::OverSpan;
        }
        if !complete && end_line.starts_with(&bytes[cursor..]) {
            return Candidate::OverSpan;
        }
        if cursor == bytes.len() || cursor - body_start >= MAX_PEM_BODY {
            return Candidate::OverSpan;
        }
        match bytes[cursor] {
            b'\r' if bytes.get(cursor + 1) == Some(&b'\n') => cursor += 2,
            b'\r' if !complete && cursor + 1 == bytes.len() => return Candidate::OverSpan,
            b'\n' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => cursor += 1,
            _ => return Candidate::None,
        }
    }
}

fn line_ending_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) == Some(&b'\n') {
        Some(start + 1)
    } else if bytes.get(start..start + 2) == Some(b"\r\n") {
        Some(start + 2)
    } else {
        None
    }
}

fn has_line_start(bytes: &[u8], start: usize) -> bool {
    start == 0 || bytes[start - 1] == b'\n'
}

fn alternatives_at(bytes: &[u8], start: usize) -> impl Iterator<Item = Alternative> + '_ {
    ALTERNATIVES
        .into_iter()
        .chain([
            alternative(b"hf_", 20, 509, is_alnum),
            alternative(b"AKIA", 16, 16, is_upper_alnum),
            alternative(b"ASIA", 16, 16, is_upper_alnum),
        ])
        .filter(move |alternative| bytes[start..].starts_with(alternative.prefix))
}

enum Candidate {
    Matched(usize),
    OverSpan,
    None,
}

fn scan_alternative(
    bytes: &[u8],
    start: usize,
    alternative: Alternative,
    complete: bool,
) -> Candidate {
    let body_start = start + alternative.prefix.len();
    let mut end = body_start;
    while end < bytes.len()
        && end - body_start < alternative.max_body
        && (alternative.alphabet)(bytes[end])
    {
        end += 1;
    }
    let body_len = end - body_start;
    if body_len == alternative.max_body
        && (end < bytes.len() && (alternative.alphabet)(bytes[end])
            || end == bytes.len() && !complete)
    {
        return Candidate::OverSpan;
    }
    if end == bytes.len() && !complete {
        return Candidate::None;
    }
    if body_len >= alternative.min_body {
        Candidate::Matched(end)
    } else {
        Candidate::None
    }
}

fn has_start_boundary(bytes: &[u8], start: usize) -> bool {
    start == 0 || !is_alnum_underscore(bytes[start - 1])
}

fn scalar_boundary_at_or_before(content: &str, mut offset: usize) -> usize {
    while !content.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

const fn is_alnum(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

const fn is_alnum_underscore(byte: u8) -> bool {
    is_alnum(byte) || byte == b'_'
}

const fn is_b64(byte: u8) -> bool {
    is_alnum_underscore(byte) || byte == b'-'
}

const fn is_alnum_hyphen(byte: u8) -> bool {
    is_alnum(byte) || byte == b'-'
}

const fn is_upper_alnum(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_boundary_moves_before_a_partial_scalar() {
        let content = format!("{}é", "x".repeat(510));
        assert_eq!(scalar_boundary_at_or_before(&content, 511), 510);
    }

    #[test]
    fn incomplete_candidate_waits_for_its_terminator() {
        let content = format!("sk-{}", "a".repeat(20));
        assert!(scan(&content, false).matches.is_empty());
        assert_eq!(scan(&content, true).matches[0].range, 0..content.len());
    }

    #[test]
    fn arbitration_keeps_the_longer_match_then_provider_token_on_a_tie() {
        let mut selected = None;
        select_match(&mut selected, 20, Rule::SecretProviderToken);
        select_match(&mut selected, 30, Rule::SecretPemPrivateKey);
        assert_eq!(selected, Some((30, Rule::SecretPemPrivateKey)));

        select_match(&mut selected, 30, Rule::SecretProviderToken);
        assert_eq!(selected, Some((30, Rule::SecretPemPrivateKey)));

        let mut tie = None;
        select_match(&mut tie, 30, Rule::SecretProviderToken);
        select_match(&mut tie, 30, Rule::SecretPemPrivateKey);
        assert_eq!(tie, Some((30, Rule::SecretProviderToken)));
    }
}
