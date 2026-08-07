use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// Each entry must name the test and constant, for example
// ("deadline_uses_the_production_contract", "REQUEST_TIMEOUT").
const DELIBERATE_EXCEPTIONS: &[(&str, &str)] = &[("allowed_reference", "DEFAULT_TIMEOUT")];

#[test]
fn test_modules_do_not_inherit_production_timeout_constants() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = scan_tree(&source_root);
    assert!(
        violations.is_empty(),
        "test modules reference production timeout constants:\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_fixture_detects_a_production_timeout_reference() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/timeout_guard/references_constant.rs");
    let violations = scan_file(&fixture);
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert!(violations[0].contains("DEFAULT_TIMEOUT"));
    assert!(violations[0].contains("inherits_the_timeout"));
}

#[test]
fn guard_checks_references_after_an_allowed_reference() {
    let fixture = fixture("allowed_then_forbidden.rs");
    let violations = scan_file(&fixture);
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert!(violations[0].contains("forbidden_reference"));
}

#[test]
fn guard_finds_constants_declared_after_a_test_module() {
    let fixture = fixture("constant_after_test_module.rs");
    let violations = scan_file(&fixture);
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert!(violations[0].contains("LATE_TIMEOUT"));
}

#[test]
fn guard_checks_each_test_module() {
    let fixture = fixture("multiple_test_modules.rs");
    let violations = scan_file(&fixture);
    assert_eq!(violations.len(), 2, "{violations:#?}");
    assert!(violations[0].contains("first_reference"));
    assert!(violations[1].contains("second_reference"));
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/timeout_guard")
        .join(name)
}

fn scan_tree(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files.sort();
    let constants: BTreeSet<String> = files
        .iter()
        .flat_map(|path| production_timeout_constants(&fs::read_to_string(path).unwrap()))
        .collect();
    files
        .into_iter()
        .flat_map(|path| scan_file_for_constants(&path, &constants))
        .collect()
}

fn collect_rust_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn scan_file(path: &Path) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap();
    let constants = production_timeout_constants(&source);
    scan_source(path, &source, &constants)
}

fn scan_file_for_constants(path: &Path, constants: &BTreeSet<String>) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap();
    scan_source(path, &source, constants)
}

fn scan_source(path: &Path, source: &str, constants: &BTreeSet<String>) -> Vec<String> {
    if constants.is_empty() {
        return Vec::new();
    }

    let lines: Vec<_> = source.lines().collect();
    let mut violations = Vec::new();
    for (start, end) in test_modules(&lines) {
        for constant in constants {
            for (offset, line) in lines[start..end].iter().enumerate() {
                let reference_line = start + offset;
                let test =
                    enclosing_test_name(&lines, start, reference_line).unwrap_or("unknown_test");
                for _ in identifier_occurrences(line, constant) {
                    if !DELIBERATE_EXCEPTIONS.contains(&(test, constant.as_str())) {
                        violations.push(format!(
                            "{}:{}: {test} references {constant}",
                            path.display(),
                            reference_line + 1
                        ));
                    }
                }
            }
        }
    }
    violations
}

fn production_timeout_constants(source: &str) -> BTreeSet<String> {
    let lines: Vec<_> = source.lines().collect();
    let test_modules = test_modules(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(line, _)| {
            !test_modules
                .iter()
                .any(|(start, end)| start <= line && line < end)
        })
        .map(|(_, line)| *line)
        .filter_map(|line| {
            let line = line.trim_start();
            let declaration = line
                .strip_prefix("const ")
                .or_else(|| line.strip_prefix("pub const "))
                .or_else(|| line.strip_prefix("pub(crate) const "))?;
            let name = declaration.split([':', ' ']).next()?;
            (name.contains("_TIMEOUT") || name.contains("_DEADLINE")).then(|| name.to_owned())
        })
        .collect()
}

fn test_modules(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut modules = Vec::new();
    let mut line = 0;
    while line < lines.len() {
        if lines[line].trim() != "#[cfg(test)]" {
            line += 1;
            continue;
        }
        let Some(module_line) =
            (line + 1..lines.len()).find(|index| !lines[*index].trim().is_empty())
        else {
            return modules;
        };
        if !lines[module_line].trim_start().starts_with("mod ") {
            line = module_line + 1;
            continue;
        }
        let mut depth = 0_i32;
        let mut saw_open = false;
        let mut end = module_line;
        for (index, source_line) in lines.iter().enumerate().skip(module_line) {
            let change = brace_depth_change(source_line);
            saw_open |= source_line.contains('{');
            depth += change;
            end = index + 1;
            if saw_open && depth == 0 {
                break;
            }
        }
        modules.push((module_line, end));
        line = end;
    }
    modules
}

fn brace_depth_change(line: &str) -> i32 {
    line.chars().fold(0, |depth, character| match character {
        '{' => depth + 1,
        '}' => depth - 1,
        _ => depth,
    })
}

fn identifier_occurrences<'a>(
    line: &'a str,
    identifier: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    line.match_indices(identifier).filter_map(|(start, _)| {
        let before = line[..start].chars().next_back();
        let after = line[start + identifier.len()..].chars().next();
        (!before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char))
            .then_some(start)
    })
}

fn is_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn enclosing_test_name<'a>(
    lines: &'a [&str],
    start: usize,
    reference_line: usize,
) -> Option<&'a str> {
    lines[start..=reference_line].iter().rev().find_map(|line| {
        let rest = line.trim_start().strip_prefix("fn ")?;
        rest.split('(').next()
    })
}
