use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// Each entry must name the test and constant, for example
// ("deadline_uses_the_production_contract", "REQUEST_TIMEOUT").
const DELIBERATE_EXCEPTIONS: &[(&str, &str)] = &[];

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

    test_modules(source)
        .into_iter()
        .flat_map(|(module_line, module)| {
            constants.iter().filter_map(move |constant| {
                let reference_line = module
                    .lines()
                    .position(|line| contains_identifier(line, constant))?;
                let test = enclosing_test_name(module, reference_line).unwrap_or("unknown_test");
                if DELIBERATE_EXCEPTIONS.contains(&(test, constant.as_str())) {
                    return None;
                }
                Some(format!(
                    "{}:{}: {test} references {constant}",
                    path.display(),
                    module_line + reference_line
                ))
            })
        })
        .collect()
}

fn production_timeout_constants(source: &str) -> BTreeSet<String> {
    let mut test_cfg = false;
    source
        .lines()
        .take_while(|line| {
            let trimmed = line.trim();
            if trimmed == "#[cfg(test)]" {
                test_cfg = true;
                return true;
            }
            if test_cfg && trimmed.starts_with("mod ") {
                return false;
            }
            if !trimmed.is_empty() && test_cfg {
                test_cfg = false;
            }
            true
        })
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

fn test_modules(source: &str) -> Vec<(usize, &str)> {
    let lines: Vec<&str> = source.lines().collect();
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
            break;
        };
        if !lines[module_line].trim_start().starts_with("mod ") {
            line = module_line + 1;
            continue;
        }
        let start = lines[..=module_line]
            .iter()
            .map(|item| item.len() + 1)
            .sum();
        modules.push((module_line + 2, &source[start..]));
        break;
    }
    modules
}

fn contains_identifier(line: &str, identifier: &str) -> bool {
    line.match_indices(identifier).any(|(start, _)| {
        let before = line[..start].chars().next_back();
        let after = line[start + identifier.len()..].chars().next();
        !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
    })
}

fn is_identifier_char(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

fn enclosing_test_name(module: &str, reference_line: usize) -> Option<&str> {
    let lines: Vec<_> = module.lines().take(reference_line + 1).collect();
    lines.into_iter().rev().find_map(|line| {
        let rest = line.trim_start().strip_prefix("fn ")?;
        rest.split('(').next()
    })
}
