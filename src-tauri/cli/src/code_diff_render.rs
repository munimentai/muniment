use muniment_code_diff::{CodeDiff, DiffFile, DiffLine, DiffLineKind, DiffLineSegmentKind};
use std::fmt::Write;

const RESET: &str = "\u{1b}[0m";
const BOLD: &str = "\u{1b}[1m";
const RED: &str = "\u{1b}[31m";
const GREEN: &str = "\u{1b}[32m";
const CYAN: &str = "\u{1b}[36m";

pub fn render_code_diff(diff: &CodeDiff, stdout_is_terminal: bool, no_color: bool) -> String {
    let color = stdout_is_terminal && !no_color;
    let mut output = String::new();

    if diff.truncated {
        writeln!(output, "Warning: This diff is truncated.").unwrap();
    }
    if diff.files.is_empty() {
        writeln!(output, "No changes.").unwrap();
        return output;
    }

    for file in &diff.files {
        render_file(&mut output, file, color);
    }
    output
}

fn render_file(output: &mut String, file: &DiffFile, color: bool) {
    let old_path = file.old_path.as_deref().unwrap_or("/dev/null");
    let new_path = file.new_path.as_deref().unwrap_or("/dev/null");
    styled_line(output, &format!("--- {old_path}"), CYAN, color);
    styled_line(output, &format!("+++ {new_path}"), CYAN, color);

    if file.binary {
        writeln!(output, "Binary file changed").unwrap();
        return;
    }

    for hunk in &file.hunks {
        styled_line(output, &hunk.header, CYAN, color);
        for line in &hunk.lines {
            render_line(output, line, color);
        }
    }
}

fn styled_line(output: &mut String, text: &str, style: &str, color: bool) {
    if color {
        writeln!(output, "{style}{text}{RESET}").unwrap();
    } else {
        writeln!(output, "{text}").unwrap();
    }
}

fn render_line(output: &mut String, line: &DiffLine, color: bool) {
    let (marker, line_color) = match line.kind {
        DiffLineKind::Context => (' ', None),
        DiffLineKind::Addition => ('+', Some(GREEN)),
        DiffLineKind::Deletion => ('-', Some(RED)),
    };

    if !color || line_color.is_none() {
        writeln!(output, "{marker}{}", line.text).unwrap();
        return;
    }

    let line_color = line_color.unwrap();
    write!(output, "{line_color}{marker}").unwrap();
    if line.segments.is_empty() {
        write!(output, "{}", line.text).unwrap();
    } else {
        for segment in &line.segments {
            let emphasized = matches!(
                segment.kind,
                DiffLineSegmentKind::Addition | DiffLineSegmentKind::Deletion
            );
            if emphasized {
                write!(output, "{BOLD}{}{RESET}{line_color}", segment.text).unwrap();
            } else {
                write!(output, "{}", segment.text).unwrap();
            }
        }
    }
    writeln!(output, "{RESET}").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_code_diff::decode;
    use std::fs;
    use std::path::PathBuf;

    fn fixture_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol-fixtures/code-diff/1")
    }

    fn expected(name: &str) -> &'static str {
        match name {
            "binary.json" => "--- assets/icon.png\n+++ assets/icon.png\nBinary file changed\n",
            "empty.json" => "No changes.\n",
            "modified.json" => concat!(
                "--- src/message.txt\n",
                "+++ src/message.txt\n",
                "@@ -1 +1 @@\n",
                "-Hello world\n",
                "+Hello Muniment\n",
            ),
            "renamed.json" => concat!(
                "--- old-name.txt\n",
                "+++ new-name.txt\n",
                "@@ -1 +1 @@\n",
                "-Hello world\n",
                "+Hello Muniment\n",
            ),
            "truncated.json" => concat!(
                "Warning: This diff is truncated.\n",
                "--- src/large.txt\n",
                "+++ src/large.txt\n",
                "@@ -1 +1 @@\n",
                "-Hello world\n",
                "+Hello Muniment\n",
            ),
            _ => panic!("add an expected rendering for {name}"),
        }
    }

    #[test]
    fn renders_every_fixture_as_plain_unified_text() {
        let mut paths: Vec<_> = fs::read_dir(fixture_directory())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();

        assert!(!paths.is_empty());
        for path in paths {
            let diff = decode(&fs::read(&path).unwrap()).unwrap();
            let rendered = render_code_diff(&diff, false, false);
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(rendered, expected(name), "fixture {name}");
            assert!(!rendered.contains('\u{1b}'), "fixture {name}");
        }
    }

    #[test]
    fn colors_headers_lines_and_changed_segments_for_a_terminal() {
        let path = fixture_directory().join("modified.json");
        let diff = decode(&fs::read(path).unwrap()).unwrap();
        let rendered = render_code_diff(&diff, true, false);

        assert!(rendered.contains("\u{1b}[36m--- src/message.txt\u{1b}[0m"));
        assert!(rendered.contains("\u{1b}[31m-Hello \u{1b}[1mworld"));
        assert!(rendered.contains("\u{1b}[32m+Hello \u{1b}[1mMuniment"));
    }

    #[test]
    fn no_color_disables_ansi_for_a_terminal() {
        let path = fixture_directory().join("modified.json");
        let diff = decode(&fs::read(path).unwrap()).unwrap();
        let rendered = render_code_diff(&diff, true, true);

        assert_eq!(rendered, expected("modified.json"));
        assert!(!rendered.contains('\u{1b}'));
    }
}
