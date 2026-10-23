//! Small text helpers the describers share: humanized names and keys,
//! quoting, truncation, file and host names, counts and durations.

/// Words a label keeps in capitals.
const ACRONYMS: [&str; 21] = [
    "api", "aws", "crm", "csv", "css", "dns", "html", "http", "https", "id", "ids", "json", "llm",
    "mcp", "pdf", "sms", "sql", "ssh", "ui", "uri", "url",
];

/// Splits an identifier into lowercase words: snake case, kebab case, camel
/// case and dotted names all split, so `getUserProfile` and
/// `get_user_profile` both read `get user profile`.
pub fn words(identifier: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous: Option<char> = None;
    let chars: Vec<char> = identifier.chars().collect();
    for (index, &character) in chars.iter().enumerate() {
        if !character.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous = None;
            continue;
        }
        let next_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
        let boundary = match previous {
            Some(before) if character.is_uppercase() => {
                before.is_lowercase()
                    || before.is_ascii_digit()
                    || (before.is_uppercase() && next_lower)
            }
            _ => false,
        };
        if boundary && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.extend(character.to_lowercase());
        previous = Some(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn cased(word: &str) -> String {
    if ACRONYMS.contains(&word) {
        if word == "ids" {
            return "IDs".into();
        }
        return word.to_uppercase();
    }
    word.to_owned()
}

/// An identifier as a phrase in sentence case: `create_issue` reads
/// `Create issue` and `numResults` reads `Number of results`.
pub fn phrase(identifier: &str) -> String {
    let mut parts: Vec<String> = words(identifier).iter().map(|word| cased(word)).collect();
    if parts.first().map(String::as_str) == Some("num") && parts.len() > 1 {
        parts[0] = "number of".into();
    }
    capitalize(&parts.join(" "))
}

/// A phrase that continues a sentence: its first letter lowercase unless
/// the first word is an acronym or a proper name in capitals.
pub fn lower_first(text: &str) -> String {
    let first = text.split_whitespace().next().unwrap_or_default();
    let keeps = first.chars().filter(|c| c.is_uppercase()).count() > 1;
    if keeps {
        return text.to_owned();
    }
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A name as a person reads it: each word capitalized, so `linear`
/// reads `Linear` and `google-drive` reads `Google Drive`.
pub fn title(identifier: &str) -> String {
    words(identifier)
        .iter()
        .map(|word| {
            if ACRONYMS.contains(&word.as_str()) {
                cased(word)
            } else {
                capitalize(word)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first line of `text` with its whitespace folded, at most `max`
/// characters.
pub fn clip(text: &str, max: usize) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() <= max {
        return folded;
    }
    let mut short: String = folded.chars().take(max.saturating_sub(1)).collect();
    short = short.trim_end().to_owned();
    short.push('…');
    short
}

/// Free text in curly quotes, clipped.
pub fn quoted(text: &str, max: usize) -> String {
    format!("\u{201c}{}\u{201d}", clip(text, max))
}

/// Code in backticks, clipped.
pub fn code(text: &str, max: usize) -> String {
    format!("`{}`", clip(text, max).replace('`', "'"))
}

/// The first non-empty line of `text`, clipped.
pub fn first_line(text: &str, max: usize) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| clip(line, max))
        .unwrap_or_default()
}

/// The last part of a path.
pub fn file_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
        .to_owned()
}

/// The host of a URL, without `www.`, or the text as given when it names
/// no host.
pub fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit('@').next().unwrap_or(host);
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}

/// A URL without its scheme, clipped.
pub fn short_url(url: &str, max: usize) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    clip(rest.trim_end_matches('/'), max)
}

/// `1 result` or `8 results`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The plural of a lowercase noun label, for counted lists.
pub fn plural(noun: &str) -> String {
    let noun = noun.trim();
    if noun.ends_with('s') {
        noun.to_owned()
    } else if let Some(stem) = noun
        .strip_suffix('y')
        .filter(|stem| !stem.ends_with(['a', 'e', 'i', 'o', 'u']))
    {
        format!("{stem}ies")
    } else {
        format!("{noun}s")
    }
}

/// The singular of a lowercase plural noun, for a count of one.
pub fn singular(noun: &str) -> String {
    let noun = noun.trim();
    if let Some(stem) = noun.strip_suffix("ies") {
        format!("{stem}y")
    } else if noun.ends_with("ss") {
        noun.to_owned()
    } else {
        noun.strip_suffix('s').unwrap_or(noun).to_owned()
    }
}

/// A duration in seconds: `4.2 s`, `42 s` or `3 min 20 s`.
pub fn duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return String::new();
    }
    if seconds < 10.0 {
        return format!("{:.1} s", (seconds * 10.0).round() / 10.0);
    }
    let total = seconds.round() as u64;
    if total < 60 {
        return format!("{total} s");
    }
    let (hours, minutes, rest) = (total / 3600, total % 3600 / 60, total % 60);
    if hours > 0 {
        format!("{hours} h {minutes} min")
    } else if rest == 0 {
        format!("{minutes} min")
    } else {
        format!("{minutes} min {rest} s")
    }
}

/// `a, b and c`.
pub fn listing(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
