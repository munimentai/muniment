//! Reads a shell command before it runs: whether it only reads, whether it
//! only creates, moves, copies or removes files, and which paths it reads and
//! writes. Anything the reader cannot follow counts as a command that may do
//! anything, so it asks.
//!
//! The read-only command core, the options that withdraw a core word's read
//! claim, the `sed` and `awk` proofs and the wrapper floor are adapted from
//! `@gotgenes/pi-permission-system` (MIT License, Copyright (c) 2026 MasuRii
//! and Christopher D. Lasher).

use std::path::{Component, Path, PathBuf};

/// Commands that read and change nothing, for any arguments, apart from the
/// options [`withdraws`] names. A word counts only as a bare basename.
pub const READ_ONLY_CORE: [&str; 26] = [
    ":", "awk", "basename", "cat", "cd", "diff", "dirname", "echo", "egrep", "false", "fd",
    "fgrep", "find", "grep", "head", "ls", "pwd", "realpath", "rg", "sed", "sort", "stat", "tail",
    "true", "wc", "which",
];

/// `git` subcommands that only read the repository, unless an option in
/// [`GIT_WRITING_OPTIONS`] follows.
const GIT_READS: [&str; 7] = [
    "status",
    "log",
    "diff",
    "show",
    "rev-parse",
    "ls-files",
    "blame",
];
const GIT_WRITING_OPTIONS: [&str; 3] = ["--output", "--ext-diff", "--textconv"];

/// Commands that run another command the gate would never see: inline
/// shells, `eval`, and the indirection wrappers.
pub const WRAPPERS: [&str; 30] = [
    "bash",
    "sh",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "csh",
    "tcsh",
    "eval",
    "exec",
    "source",
    ".",
    "sudo",
    "doas",
    "env",
    "xargs",
    "time",
    "nohup",
    "timeout",
    "nice",
    "parallel",
    "rust-parallel",
    "rush",
    "setsid",
    "stdbuf",
    "watch",
    "flock",
    "command",
    "builtin",
    "su",
];

/// The file operations `edit_auto` runs inside the working folders, and the
/// options each may carry. An option outside its list makes the command an
/// ordinary one.
const FILE_OPS: [(&str, &str); 5] = [
    ("mkdir", "pv"),
    ("touch", "acm"),
    ("mv", "finv"),
    ("cp", "finvrRpa"),
    ("rm", "fivrRd"),
];

/// What a command does, as far as the reader can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellKind {
    /// Every part only reads.
    ReadOnly,
    /// Every part reads or creates, moves, copies, removes or writes the
    /// files in `ShellReading::writes`, and nothing else.
    FileOps,
    /// A part may do anything: why the reader gave up.
    Other(&'static str),
}

/// One command's reading: its kind and the absolute paths it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellReading {
    pub kind: ShellKind,
    /// Paths the command only reads.
    pub reads: Vec<PathBuf>,
    /// Paths the command writes, creates or removes.
    pub writes: Vec<PathBuf>,
    /// Paths whose direction the reader could not prove. They count as
    /// both read and written.
    pub unknown: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Word {
    text: String,
    /// The shell decides part of its value: a variable expansion.
    computed: bool,
    /// An unquoted glob or brace character, so the word may become several.
    glob: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Word(Word),
    /// `|`, `||`, `&&`, `;` or `&`.
    Separator(&'static str),
    /// An output (`true`) or input redirect to the next word.
    Redirect(bool),
    /// A file-descriptor copy such as `2>&1`, which names no file.
    Duplicate,
}

/// Splits a command into words, separators and redirects. `None` means the
/// reader cannot follow it: a command substitution, a subshell or group, a
/// here-document, a process substitution or an unclosed quote.
fn tokenize(command: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = command.chars().collect();
    let mut tokens = Vec::new();
    let mut word: Option<Word> = None;
    let mut index = 0;
    let flush = |word: &mut Option<Word>, tokens: &mut Vec<Token>| {
        if let Some(word) = word.take() {
            tokens.push(Token::Word(word));
        }
    };
    while index < chars.len() {
        let char = chars[index];
        let next = chars.get(index + 1).copied();
        match char {
            ' ' | '\t' => flush(&mut word, &mut tokens),
            '\n' => {
                flush(&mut word, &mut tokens);
                tokens.push(Token::Separator(";"));
            }
            '#' if word.is_none() => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
                continue;
            }
            '\'' => {
                let current = word.get_or_insert_with(Word::default);
                index += 1;
                while index < chars.len() && chars[index] != '\'' {
                    current.text.push(chars[index]);
                    index += 1;
                }
                if index >= chars.len() {
                    return None;
                }
            }
            '"' => {
                let current = word.get_or_insert_with(Word::default);
                index += 1;
                loop {
                    let char = *chars.get(index)?;
                    match char {
                        '"' => break,
                        '`' => return None,
                        '$' if chars.get(index + 1) == Some(&'(') => return None,
                        '$' => {
                            current.computed = true;
                            current.text.push('$');
                        }
                        '\\' => {
                            let escaped = *chars.get(index + 1)?;
                            if !matches!(escaped, '"' | '$' | '`' | '\\' | '\n') {
                                current.text.push('\\');
                            }
                            current.text.push(escaped);
                            index += 1;
                        }
                        char => current.text.push(char),
                    }
                    index += 1;
                }
            }
            '\\' => {
                let escaped = next?;
                if escaped != '\n' {
                    word.get_or_insert_with(Word::default).text.push(escaped);
                }
                index += 1;
            }
            '`' => return None,
            '$' if next == Some('(') => return None,
            '$' => {
                let current = word.get_or_insert_with(Word::default);
                current.computed = true;
                current.text.push('$');
            }
            '(' | ')' => return None,
            '{' | '}' if word.is_none() => return None,
            '{' | '}' | '*' | '?' | '[' => {
                let current = word.get_or_insert_with(Word::default);
                current.glob = true;
                current.text.push(char);
            }
            '|' => {
                flush(&mut word, &mut tokens);
                match next {
                    Some('|') => {
                        tokens.push(Token::Separator("||"));
                        index += 1;
                    }
                    Some('&') => {
                        tokens.push(Token::Separator("|"));
                        index += 1;
                    }
                    _ => tokens.push(Token::Separator("|")),
                }
            }
            ';' => {
                if next == Some(';') {
                    return None;
                }
                flush(&mut word, &mut tokens);
                tokens.push(Token::Separator(";"));
            }
            '&' => {
                flush(&mut word, &mut tokens);
                match next {
                    Some('&') => {
                        tokens.push(Token::Separator("&&"));
                        index += 1;
                    }
                    Some('>') => {
                        index += 1;
                        if chars.get(index + 1) == Some(&'>') {
                            index += 1;
                        }
                        tokens.push(Token::Redirect(true));
                    }
                    _ => tokens.push(Token::Separator("&")),
                }
            }
            '>' | '<' => {
                // Digits right before the operator name a file descriptor.
                if word
                    .as_ref()
                    .is_some_and(|word| !word.text.chars().all(|c| c.is_ascii_digit()))
                {
                    flush(&mut word, &mut tokens);
                }
                word = None;
                let output = char == '>';
                if next == Some('(') {
                    return None;
                }
                if output {
                    if matches!(next, Some('>') | Some('|')) {
                        index += 1;
                    }
                } else {
                    match (next, chars.get(index + 2).copied()) {
                        (Some('<'), Some('<')) => index += 2,
                        (Some('<'), _) | (Some('>'), _) => return None,
                        _ => {}
                    }
                }
                if chars.get(index + 1) == Some(&'&') {
                    index += 2;
                    while chars
                        .get(index)
                        .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                    {
                        index += 1;
                    }
                    tokens.push(Token::Duplicate);
                    continue;
                }
                tokens.push(Token::Redirect(output));
            }
            char => word.get_or_insert_with(Word::default).text.push(char),
        }
        index += 1;
    }
    flush(&mut word, &mut tokens);
    Some(tokens)
}

/// One simple command: its words, the separator after it, and its redirect
/// targets.
#[derive(Debug, Default)]
struct Segment {
    words: Vec<Word>,
    outputs: Vec<Word>,
    inputs: Vec<Word>,
    after: &'static str,
}

fn segments(tokens: Vec<Token>) -> Option<Vec<Segment>> {
    let mut segments = Vec::new();
    let mut current = Segment::default();
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            Token::Word(word) => current.words.push(word),
            Token::Separator(separator) => {
                current.after = separator;
                segments.push(std::mem::take(&mut current));
            }
            Token::Redirect(output) => {
                let Some(Token::Word(target)) = tokens.next() else {
                    return None;
                };
                if output {
                    current.outputs.push(target);
                } else {
                    current.inputs.push(target);
                }
            }
            Token::Duplicate => {}
        }
    }
    segments.push(current);
    Some(segments)
}

/// The absolute, lexically normalized path a word names from `cwd`. `~` and
/// `~/…` name the user's home; `~user` names nothing this reader proves.
pub fn resolve(word: &str, cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let joined = if word == "~" {
        home?.to_path_buf()
    } else if let Some(rest) = word.strip_prefix("~/") {
        home?.join(rest)
    } else if word.starts_with('~') {
        return None;
    } else {
        cwd.join(word)
    };
    let mut resolved = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            component => resolved.push(component),
        }
    }
    Some(resolved)
}

/// Whether `word` is an option that withdraws a guarded word's read claim:
/// a long stem or any abbreviation of it, a short letter anywhere in a
/// single-dash cluster, or a single-dash long option with any suffix.
fn withdraws(word: &str, long: &[&str], short: &str, dash_words: &[&str]) -> bool {
    let name = word.split('=').next().unwrap_or_default();
    if name.starts_with("--") && name.len() > 2 {
        return long.iter().any(|stem| stem.starts_with(name));
    }
    if dash_words.iter().any(|prefix| word.starts_with(prefix)) {
        return true;
    }
    word.starts_with('-')
        && !word.starts_with("--")
        && word.len() > 1
        && word[1..].chars().any(|letter| short.contains(letter))
}

/// A computed or leading-glob argument may arrive as any option.
fn may_be_option(word: &Word) -> bool {
    word.computed || (word.glob && word.text.starts_with(['*', '?', '[', '{']))
}

/// Whether a `sed` script uses only commands that print, delete, quit or
/// substitute without writing or running anything.
fn sed_script_reads(script: &str) -> bool {
    script.split(['\n', ';']).map(str::trim).all(|command| {
        let mut rest = command;
        // Addresses: line numbers, `$`, `/regex/`, ranges, steps and `!`.
        loop {
            let trimmed = rest.trim_start();
            if let Some(after) = trimmed.strip_prefix('/') {
                let Some(end) = after.find('/') else {
                    return false;
                };
                if after[..end].contains(['[', '\\']) {
                    return false;
                }
                rest = &after[end + 1..];
            } else if trimmed.starts_with(|c: char| c.is_ascii_digit() || "$,~+!".contains(c)) {
                rest = &trimmed[1..];
            } else {
                rest = trimmed;
                break;
            }
        }
        if rest.is_empty() {
            return true;
        }
        if let Some(substitution) = rest.strip_prefix('s').or(rest.strip_prefix('y')) {
            let Some(body) = substitution.strip_prefix('/') else {
                return false;
            };
            if body.contains(['[', '\\']) {
                return false;
            }
            let parts: Vec<&str> = body.split('/').collect();
            return parts.len() == 3
                && parts[2]
                    .chars()
                    .all(|flag| "gpiIm0123456789".contains(flag));
        }
        rest.len() == 1 && "pPdDqQnN=lhHgGx".contains(rest)
    })
}

/// Whether a `sed` command line is proven to only read.
fn sed_reads(arguments: &[Word]) -> bool {
    const LONG: [&str; 10] = [
        "--quiet",
        "--silent",
        "--regexp-extended",
        "--separate",
        "--unbuffered",
        "--null-data",
        "--posix",
        "--debug",
        "--sandbox",
        "--expression",
    ];
    let mut scripts = Vec::new();
    let mut positional = false;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word.computed {
            return false;
        }
        let text = word.text.as_str();
        if text == "-e" || text == "--expression" {
            if positional {
                return false;
            }
            let Some(script) = words.next() else {
                return false;
            };
            scripts.push(script.text.clone());
        } else if let Some(script) = text.strip_prefix("--expression=") {
            scripts.push(script.to_owned());
        } else if text.starts_with("--") {
            if !LONG.contains(&text) {
                return false;
            }
        } else if text.starts_with('-') && text.len() > 1 {
            if !text[1..].chars().all(|letter| "nErsuz".contains(letter)) {
                return false;
            }
        } else if scripts.is_empty() && !positional {
            scripts.push(text.to_owned());
            positional = true;
        } else {
            positional = true;
        }
    }
    !scripts.is_empty() && scripts.iter().all(|script| sed_script_reads(script))
}

/// Whether an `awk` command line is proven to only read: only `-F` and `-v`
/// options, and a program with no output redirect, pipe, `system` or `@`.
fn awk_reads(arguments: &[Word]) -> bool {
    let mut program = None;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word.computed {
            return false;
        }
        let text = word.text.as_str();
        if program.is_some() {
            continue;
        }
        if text == "-F" || text == "-v" {
            words.next();
        } else if !text.starts_with('-') {
            program = Some(text);
        } else if !text.starts_with("-F") && !text.starts_with("-v") {
            return false;
        }
    }
    program.is_some_and(|program| !program.contains(['>', '|', '@']) && !program.contains("system"))
}

/// Whether one core word's command line only reads.
fn core_reads(command: &str, arguments: &[Word]) -> bool {
    match command {
        "find" => !arguments.iter().any(|word| {
            may_be_option(word)
                || withdraws(
                    &word.text,
                    &[],
                    "",
                    &["-exec", "-ok", "-delete", "-fprint", "-fls"],
                )
        }),
        "fd" => !arguments.iter().any(|word| {
            may_be_option(word) || withdraws(&word.text, &["--exec", "--exec-batch"], "xX", &[])
        }),
        "sort" => !arguments
            .iter()
            .any(|word| may_be_option(word) || withdraws(&word.text, &["--output"], "o", &[])),
        "sed" => sed_reads(arguments),
        "awk" => awk_reads(arguments),
        _ => true,
    }
}

fn git_reads(arguments: &[Word]) -> bool {
    arguments
        .first()
        .is_some_and(|sub| !sub.computed && GIT_READS.contains(&sub.text.as_str()))
        && !arguments
            .iter()
            .any(|word| word.computed || withdraws(&word.text, &GIT_WRITING_OPTIONS, "", &[]))
}

/// The operands of a file operation, or `None` when an option or operand
/// is outside what `edit_auto` proves.
fn file_op_operands<'a>(command: &str, arguments: &'a [Word]) -> Option<Vec<&'a Word>> {
    let (_, letters) = FILE_OPS.iter().find(|(name, _)| *name == command)?;
    let mut operands = Vec::new();
    let mut options_done = false;
    for word in arguments {
        if word.computed || word.glob {
            return None;
        }
        if !options_done && word.text == "--" {
            options_done = true;
        } else if !options_done && word.text.starts_with('-') && word.text.len() > 1 {
            if word.text.starts_with("--") || !word.text[1..].chars().all(|c| letters.contains(c)) {
                return None;
            }
        } else {
            operands.push(word);
        }
    }
    (!operands.is_empty()).then_some(operands)
}

fn is_assignment(word: &Word) -> bool {
    word.text.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Reads a `bash` command run from `cwd`.
pub fn read_command(command: &str, cwd: &Path, home: Option<&Path>) -> ShellReading {
    let mut reading = ShellReading {
        kind: ShellKind::ReadOnly,
        reads: Vec::new(),
        writes: Vec::new(),
        unknown: Vec::new(),
    };
    let other = |mut reading: ShellReading, reason| {
        reading.kind = ShellKind::Other(reason);
        reading
    };
    let Some(segments) = tokenize(command).and_then(segments) else {
        return other(reading, "The command could not be read.");
    };
    let mut cwd = cwd.to_path_buf();
    let mut file_ops = false;
    for segment in &segments {
        let path = |word: &Word, cwd: &Path| resolve(&word.text, cwd, home);
        for word in &segment.inputs {
            reading.reads.extend(path(word, &cwd));
        }
        let outputs: Vec<&Word> = segment
            .outputs
            .iter()
            .filter(|word| word.text != "/dev/null")
            .collect();
        if outputs.iter().any(|word| word.computed || word.glob) {
            return other(reading, "The command writes to a file it names indirectly.");
        }
        for word in &outputs {
            reading.writes.extend(path(word, &cwd));
            file_ops = true;
        }
        let Some((first, arguments)) = segment.words.split_first() else {
            continue;
        };
        if is_assignment(first) {
            return other(reading, "The command sets variables before it runs.");
        }
        if first.computed || first.glob || first.text.contains(['/', '\\']) {
            return other(reading, "The command names its program indirectly.");
        }
        let name = first.text.as_str();
        if WRAPPERS.contains(&name) {
            return other(reading, "The command runs another command inside it.");
        }
        let literal_paths = |reading: &mut Vec<PathBuf>, cwd: &Path| {
            for word in arguments.iter().filter(|word| !word.text.starts_with('-')) {
                reading.extend(path(word, cwd));
            }
        };
        if name == "cd" || name == "pushd" {
            let target = match arguments {
                [] => home.map(Path::to_path_buf),
                [word] if !word.computed && !word.glob => path(word, &cwd),
                _ => None,
            };
            let Some(target) = target else {
                return other(
                    reading,
                    "The command changes to a folder it names indirectly.",
                );
            };
            // Only `&&` runs the next command in the new folder for sure.
            if segment.after != "&&" && !segment.after.is_empty() {
                return other(reading, "The command changes folders between commands.");
            }
            reading.reads.push(target.clone());
            cwd = target;
        } else if READ_ONLY_CORE.contains(&name) {
            if !core_reads(name, arguments) {
                return other(
                    reading,
                    "An option lets this command write or run commands.",
                );
            }
            literal_paths(&mut reading.reads, &cwd);
        } else if name == "git" {
            if !git_reads(arguments) {
                return other(reading, "This git command can change the repository.");
            }
            literal_paths(&mut reading.reads, &cwd);
        } else if let Some(operands) = file_op_operands(name, arguments) {
            file_ops = true;
            let operands: Vec<PathBuf> = operands
                .into_iter()
                .map(|word| path(word, &cwd))
                .collect::<Option<_>>()
                .unwrap_or_default();
            if operands.is_empty() {
                return other(reading, "The command names a file it cannot resolve.");
            }
            if name == "cp" {
                let (target, sources) = operands.split_last().expect("operands are not empty");
                reading.reads.extend(sources.iter().cloned());
                reading.writes.push(target.clone());
            } else {
                reading.writes.extend(operands);
            }
        } else {
            literal_paths(&mut reading.unknown, &cwd);
            return other(reading, "The command can change things.");
        }
    }
    if file_ops {
        reading.kind = ShellKind::FileOps;
    }
    reading
}

/// The words of each part of a command, read loosely: split at separators
/// and whitespace, with quotes dropped. Used only to spot critical commands,
/// so a command the strict reader refuses is still checked.
fn loose_parts(command: &str) -> Vec<Vec<String>> {
    command
        .split([';', '|', '&', '\n', '(', ')', '`', '{', '}'])
        .map(|part| {
            part.split_whitespace()
                .map(|word| word.trim_matches(['\'', '"']).to_owned())
                .filter(|word| !word.is_empty())
                .collect()
        })
        .collect()
}

/// Drops the wrappers in front of a command, with their options and values.
fn unwrap_command(words: &[String]) -> &[String] {
    let mut rest = words;
    loop {
        let Some(first) = rest.first() else {
            return rest;
        };
        let name = first.rsplit('/').next().unwrap_or(first);
        let takes_value = match name {
            "nice" | "timeout" => true,
            "sudo" | "doas" | "env" | "time" | "nohup" | "command" | "exec" | "xargs"
            | "stdbuf" | "setsid" | "bash" | "sh" | "zsh" | "dash" | "ksh" | "eval" => false,
            _ if first.contains('=') && !first.starts_with('-') => false,
            _ => return rest,
        };
        rest = &rest[1..];
        while let Some(word) = rest.first() {
            let option = word.starts_with('-') || (word.contains('=') && !word.starts_with('/'));
            let value = takes_value && word.chars().all(|c| c.is_ascii_digit() || c == '.');
            if !option && !value {
                break;
            }
            rest = &rest[1..];
        }
    }
}

/// Top-level folders whose recursive removal or permission change wrecks a
/// computer.
const SYSTEM_FOLDERS: [&str; 16] = [
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/home",
    "/lib",
    "/opt",
    "/private",
    "/root",
    "/sbin",
    "/System",
    "/Library",
    "/Applications",
    "/Users",
    "/usr",
    "/var",
];

fn critical_target(word: &str, home: Option<&Path>) -> bool {
    let word = word.trim_end_matches(['*']);
    let trimmed = if word.len() > 1 {
        word.trim_end_matches('/')
    } else {
        word
    };
    matches!(
        trimmed,
        "/" | ""
            | "~"
            | "."
            | ".."
            | "$HOME"
            | "${HOME}"
            | "/*"
            | "C:\\"
            | "C:"
            | "$env:USERPROFILE"
    ) || SYSTEM_FOLDERS
        .iter()
        .any(|folder| trimmed.eq_ignore_ascii_case(folder))
        || home.is_some_and(|home| Path::new(trimmed) == home)
}

/// Whether a command is on the critical list: it can wipe a disk, the
/// computer's system folders or the user's home, or stop the computer.
/// Critical commands never run without a person's answer.
pub fn is_critical(command: &str, home: Option<&Path>) -> bool {
    let squeezed: String = command.chars().filter(|c| !c.is_whitespace()).collect();
    if squeezed.contains(":(){") {
        return true;
    }
    loose_parts(command).iter().any(|words| {
        let words = unwrap_command(words);
        let Some((first, arguments)) = words.split_first() else {
            return false;
        };
        let name = first.rsplit(['/', '\\']).next().unwrap_or(first);
        let lower = name.to_ascii_lowercase();
        let recursive = arguments.iter().any(|word| {
            word == "--recursive"
                || word.eq_ignore_ascii_case("-recurse")
                || (word.starts_with('-') && !word.starts_with("--") && word.contains(['r', 'R']))
        });
        let targets_critical = arguments
            .iter()
            .filter(|word| !word.starts_with('-'))
            .any(|word| critical_target(word, home));
        let redirects_to_disk = arguments
            .windows(2)
            .any(|pair| matches!(pair[0].as_str(), ">" | ">>") && disk_device(&pair[1]))
            || arguments
                .iter()
                .any(|word| word.strip_prefix('>').is_some_and(disk_device));
        redirects_to_disk
            || match lower.as_str() {
                "rm" | "remove-item" | "rd" | "rmdir" => {
                    arguments.iter().any(|word| word == "--no-preserve-root")
                        || (recursive && targets_critical)
                }
                "chmod" | "chown" | "chgrp" => recursive && targets_critical,
                "find" => {
                    arguments.iter().any(|word| word == "-delete")
                        && arguments
                            .first()
                            .is_some_and(|word| critical_target(word, home))
                }
                "dd" => arguments
                    .iter()
                    .any(|word| word.strip_prefix("of=").is_some_and(disk_device)),
                "wipefs" | "fdisk" | "sfdisk" | "parted" | "shutdown" | "reboot" | "halt"
                | "poweroff" | "format-volume" | "clear-disk" | "stop-computer"
                | "restart-computer" => true,
                "init" => arguments.iter().any(|word| word == "0" || word == "6"),
                "diskutil" => arguments.first().is_some_and(|sub| {
                    let sub = sub.to_ascii_lowercase();
                    sub.starts_with("erase")
                        || sub.starts_with("partition")
                        || sub.ends_with("disk")
                        || sub == "secureerase"
                }),
                name => name.starts_with("mkfs"),
            }
    })
}

fn disk_device(path: &str) -> bool {
    [
        "/dev/sd",
        "/dev/hd",
        "/dev/nvme",
        "/dev/disk",
        "/dev/rdisk",
        "/dev/mmcblk",
        "/dev/vd",
    ]
    .iter()
    .any(|device| path.starts_with(device))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(command: &str) -> ShellKind {
        read_command(command, Path::new("/work"), Some(Path::new("/home/me"))).kind
    }

    fn reads(command: &str) -> bool {
        kind(command) == ShellKind::ReadOnly
    }

    #[test]
    fn the_read_only_core_reads_and_its_withdrawing_options_do_not() {
        for command in [
            "ls -la",
            "cat README.md | grep -n todo | head -5",
            "rg -l 'fn main' crates && wc -l Cargo.toml",
            "find . -name '*.rs'",
            "find packages/*/docs -type f",
            "fd -e md",
            "sort -u names.txt",
            "sed -n '1,80p' src/lib.rs",
            "sed 's/a/b/g' notes.md",
            "awk '{print $1}' data.csv",
            "git status",
            "git log --oneline -5",
            "git diff HEAD~1 -- src",
            "echo done 2>&1",
            "grep -r token . > /dev/null",
            "cd src && ls",
            "head -n 3 < input.txt",
        ] {
            assert!(reads(command), "{command} reads");
        }
        for command in [
            "find . -name '*.tmp' -delete",
            "find . -exec rm {} \\;",
            "find . -execdir sh -c 'x' \\;",
            "find $dir -name x",
            "find * -name x",
            "fd -x rm",
            "fd --exec-b rm",
            "sort -o out.txt names.txt",
            "sort -uo/tmp/x names.txt",
            "sort --out=/tmp/x names.txt",
            "sed -i 's/a/b/' notes.md",
            "sed 'w out' notes.md",
            "sed 's/a/b/w out' notes.md",
            "sed -f script.sed notes.md",
            "sed \"$range\" notes.md",
            "awk '{print > \"out\"}' data",
            "awk '{system(\"rm x\")}' data",
            "awk -f prog.awk data",
            "git push",
            "git diff --output=patch.diff",
            "git -c core.pager=less log",
            "./grep foo",
            "/usr/bin/grep foo",
        ] {
            assert!(!reads(command), "{command} does not only read");
        }
    }

    #[test]
    fn wrapped_and_unreadable_commands_are_other() {
        for command in [
            "bash -c 'ls'",
            "sh -c \"rm -rf x\"",
            "eval ls",
            "sudo ls",
            "env FOO=1 ls",
            "xargs grep foo",
            "time ls",
            "FOO=1 ls",
            "ls $(pwd)",
            "ls `pwd`",
            "(cd src && rm x)",
            "{ ls; }",
            "cat <<EOF\nhi\nEOF",
            "diff <(ls a) <(ls b)",
            "echo 'unclosed",
            "cat a > $OUT",
            "cd $DIR && ls",
            "cd src; rm x",
        ] {
            assert!(
                matches!(kind(command), ShellKind::Other(_)),
                "{command} is other"
            );
        }
        // A pipeline reads only when every part reads.
        assert!(matches!(kind("ls | xargs rm"), ShellKind::Other(_)));
        assert!(matches!(kind("cat a && npm install"), ShellKind::Other(_)));
    }

    #[test]
    fn file_operations_name_what_they_write_from_the_right_folder() {
        let reading = read_command(
            "mkdir -p build/out && cp notes.md build/out/notes.md && rm -f old.txt",
            Path::new("/work"),
            None,
        );
        assert_eq!(reading.kind, ShellKind::FileOps);
        assert_eq!(
            reading.writes,
            [
                PathBuf::from("/work/build/out"),
                PathBuf::from("/work/build/out/notes.md"),
                PathBuf::from("/work/old.txt"),
            ]
        );
        assert_eq!(reading.reads, [PathBuf::from("/work/notes.md")]);
        let moved = read_command("cd src && mv a.rs ../b.rs", Path::new("/work"), None);
        assert_eq!(moved.kind, ShellKind::FileOps);
        assert!(moved.writes.contains(&PathBuf::from("/work/b.rs")));
        assert!(moved.writes.contains(&PathBuf::from("/work/src/a.rs")));
        let redirect = read_command("echo hi > notes.md", Path::new("/work"), None);
        assert_eq!(redirect.kind, ShellKind::FileOps);
        assert_eq!(redirect.writes, [PathBuf::from("/work/notes.md")]);
        let home = read_command("touch ~/x", Path::new("/work"), Some(Path::new("/home/me")));
        assert_eq!(home.writes, [PathBuf::from("/home/me/x")]);
        for command in [
            "rm *.tmp",
            "rm -t dir a",
            "cp --target-directory=x a",
            "rm $X",
        ] {
            assert!(matches!(kind(command), ShellKind::Other(_)), "{command}");
        }
    }

    #[test]
    fn critical_commands_are_found_behind_wrappers_and_quotes() {
        let home = Some(Path::new("/home/me"));
        for command in [
            "rm -rf /",
            "rm -rf ~",
            "rm -fr /*",
            "sudo rm -rf --no-preserve-root /",
            "cd x && rm -r -f \"$HOME\"",
            "rm -rf /home/me",
            "rm -rf /usr/",
            "chmod -R 777 /",
            "dd if=/dev/zero of=/dev/sda",
            "echo x > /dev/disk2",
            "mkfs.ext4 /dev/sdb1",
            "sudo shutdown -h now",
            ":(){ :|:& };:",
            "diskutil eraseDisk APFS X disk2",
            "Remove-Item -Recurse -Force C:\\",
            "bash -c 'rm -rf /'",
        ] {
            assert!(is_critical(command, home), "{command} is critical");
        }
        for command in [
            "rm -rf build",
            "rm -r /work/tmp",
            "chmod 644 notes.md",
            "dd if=a of=b",
            "git push --force",
        ] {
            assert!(!is_critical(command, home), "{command} is not critical");
        }
    }
}
