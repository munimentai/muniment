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
    styled_line(
        output,
        &format!("--- {}", escape_controls(old_path)),
        CYAN,
        color,
    );
    styled_line(
        output,
        &format!("+++ {}", escape_controls(new_path)),
        CYAN,
        color,
    );

    if file.binary {
        writeln!(output, "Binary file changed").unwrap();
        return;
    }

    for hunk in &file.hunks {
        styled_line(output, &escape_controls(&hunk.header), CYAN, color);
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

fn escape_controls(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn render_line(output: &mut String, line: &DiffLine, color: bool) {
    let (marker, line_color) = match line.kind {
        DiffLineKind::Context => (' ', None),
        DiffLineKind::Addition => ('+', Some(GREEN)),
        DiffLineKind::Deletion => ('-', Some(RED)),
    };

    if !color || line_color.is_none() {
        writeln!(output, "{marker}{}", escape_controls(&line.text)).unwrap();
        return;
    }

    let line_color = line_color.unwrap();
    write!(output, "{line_color}{marker}").unwrap();
    let segments_match = !line.segments.is_empty()
        && line
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<String>()
            == line.text;
    if !segments_match {
        write!(output, "{}", escape_controls(&line.text)).unwrap();
    } else {
        for segment in &line.segments {
            let emphasized = matches!(
                segment.kind,
                DiffLineSegmentKind::Addition | DiffLineSegmentKind::Deletion
            );
            if emphasized {
                write!(
                    output,
                    "{BOLD}{}{RESET}{line_color}",
                    escape_controls(&segment.text)
                )
                .unwrap();
            } else {
                write!(output, "{}", escape_controls(&segment.text)).unwrap();
            }
        }
    }
    writeln!(output, "{RESET}").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use muniment_code_diff::{decode, DiffHunk, DiffLineSegment, DiffStatus};
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

    fn line(kind: DiffLineKind, text: &str, segments: Vec<DiffLineSegment>) -> DiffLine {
        DiffLine {
            kind,
            old_line_number: None,
            new_line_number: None,
            text: text.into(),
            segments,
        }
    }

    fn diff_with(lines: Vec<DiffLine>) -> CodeDiff {
        CodeDiff {
            schema_version: 1,
            id: "test".into(),
            files: vec![DiffFile {
                old_path: Some("old.txt".into()),
                new_path: Some("new.txt".into()),
                status: DiffStatus::Modified,
                old_mode: None,
                new_mode: None,
                binary: false,
                hunks: vec![DiffHunk {
                    old_start: 1,
                    old_count: 0,
                    new_start: 1,
                    new_count: 0,
                    header: "@@ -1,0 +1,0 @@".into(),
                    lines,
                }],
            }],
            truncated: false,
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

        assert_eq!(
            rendered,
            concat!(
                "\u{1b}[36m--- src/message.txt\u{1b}[0m\n",
                "\u{1b}[36m+++ src/message.txt\u{1b}[0m\n",
                "\u{1b}[36m@@ -1 +1 @@\u{1b}[0m\n",
                "\u{1b}[31m-Hello \u{1b}[1mworld\u{1b}[0m\u{1b}[31m\u{1b}[0m\n",
                "\u{1b}[32m+Hello \u{1b}[1mMuniment\u{1b}[0m\u{1b}[32m\u{1b}[0m\n",
            )
        );
    }

    #[test]
    fn no_color_disables_ansi_for_a_terminal() {
        let path = fixture_directory().join("modified.json");
        let diff = decode(&fs::read(path).unwrap()).unwrap();
        let rendered = render_code_diff(&diff, true, true);

        assert_eq!(rendered, expected("modified.json"));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn keeps_context_marker_and_disables_color_for_a_nonterminal() {
        let diff = diff_with(vec![line(DiffLineKind::Context, "same", vec![])]);
        let rendered = render_code_diff(&diff, false, false);

        assert_eq!(
            rendered,
            "--- old.txt\n+++ new.txt\n@@ -1,0 +1,0 @@\n same\n"
        );
        assert!(!rendered.contains('\u{1b}'));

        let color = render_code_diff(&diff, true, false);
        assert!(color.ends_with(" same\n"));
    }

    #[test]
    fn falls_back_to_line_text_for_mismatched_segments() {
        let cases = [
            vec![DiffLineSegment {
                kind: DiffLineSegmentKind::Addition,
                text: String::new(),
            }],
            vec![DiffLineSegment {
                kind: DiffLineSegmentKind::Addition,
                text: "whole".into(),
            }],
            vec![DiffLineSegment {
                kind: DiffLineSegmentKind::Addition,
                text: "different text".into(),
            }],
        ];

        for segments in cases {
            let diff = diff_with(vec![line(DiffLineKind::Addition, "whole line", segments)]);
            let rendered = render_code_diff(&diff, true, false);
            assert!(rendered.contains("\u{1b}[32m+whole line\u{1b}[0m\n"));
            assert!(!rendered.contains(BOLD));
        }
    }

    #[test]
    fn escapes_controls_from_every_printed_model_field() {
        let mut diff = diff_with(vec![
            line(DiffLineKind::Context, "line\ttext", vec![]),
            line(
                DiffLineKind::Addition,
                "segment\u{1b}",
                vec![DiffLineSegment {
                    kind: DiffLineSegmentKind::Addition,
                    text: "segment\u{1b}".into(),
                }],
            ),
        ]);
        diff.files[0].old_path = Some("old\u{1b}.txt".into());
        diff.files[0].new_path = Some("new\n.txt".into());
        diff.files[0].hunks[0].header = "header\rtext".into();

        let plain = render_code_diff(&diff, false, false);
        assert_eq!(
            plain,
            "--- old\\u{1b}.txt\n+++ new\\n.txt\nheader\\rtext\n line\\ttext\n+segment\\u{1b}\n"
        );
        assert!(!plain.contains('\u{1b}'));

        let color = render_code_diff(&diff, true, false);
        assert!(color.contains("old\\u{1b}.txt"));
        assert!(color.contains("new\\n.txt"));
        assert!(color.contains("header\\rtext"));
        assert!(color.contains(" line\\ttext\n"));
        assert!(color.contains("\u{1b}[1msegment\\u{1b}\u{1b}[0m"));
        let model_text = color
            .replace(CYAN, "")
            .replace(GREEN, "")
            .replace(BOLD, "")
            .replace(RESET, "");
        assert!(!model_text.contains('\u{1b}'));
    }

    #[test]
    fn renders_truncated_empty_diff() {
        let diff = CodeDiff {
            schema_version: 1,
            id: "empty".into(),
            files: vec![],
            truncated: true,
        };

        for color in [false, true] {
            assert_eq!(
                render_code_diff(&diff, color, false),
                "Warning: This diff is truncated.\nNo changes.\n"
            );
        }
    }

    #[test]
    fn renders_binary_files_in_both_modes() {
        let diff = decode(&fs::read(fixture_directory().join("binary.json")).unwrap()).unwrap();

        assert_eq!(
            render_code_diff(&diff, false, false),
            expected("binary.json")
        );
        assert_eq!(
            render_code_diff(&diff, true, false),
            concat!(
                "\u{1b}[36m--- assets/icon.png\u{1b}[0m\n",
                "\u{1b}[36m+++ assets/icon.png\u{1b}[0m\n",
                "Binary file changed\n",
            )
        );
    }
}
