use chrono::NaiveDate;
use muniment_core::{
    home::{
        compile_onboarding_home_write_plan, persist_onboarding_home_write_plan,
        persist_onboarding_home_write_plan_with_hook, HomeWrite, OnboardingHomeWritePlan,
        OnboardingHomeWritePlanError, OnboardingPersistHook, ONBOARDING_IMPORT_MAX_ENTRIES,
        ONBOARDING_IMPORT_MAX_TOTAL_BYTES,
    },
    import_preview::{EntryKind, ExtractedEntry},
    llama::OnboardingTriageReport,
};
use std::{
    fs,
    path::Component,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_home(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "muniment-onboarding-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn report(agents: &[&str]) -> OnboardingTriageReport {
    OnboardingTriageReport::parse(&format!(
        "## User type\nBuilder\n## Proposed Home layout\nKeep projects organized.\n## Starter agents\n{}",
        agents
            .iter()
            .map(|agent| format!("- {agent}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
    .unwrap()
}

fn entry(name: &str, provenance: &str, text: &str) -> ExtractedEntry {
    ExtractedEntry {
        source_name: name.into(),
        kind: EntryKind::Markdown,
        text: text.into(),
        source_provenance: provenance.into(),
    }
}

#[test]
fn compiles_multiple_sources_deterministically_and_preserves_verbatim_bodies() {
    let report = report(&["Research / reviewer", "../../Shell runner"]);
    let entries = vec![
        entry(
            "../../notes.md",
            "claude-export:one\nquoted",
            "first\r\n---\n\0last",
        ),
        entry("/absolute.json", "codex-export:two", "second body\n"),
    ];
    let date = NaiveDate::from_ymd_opt(2026, 7, 21).unwrap();

    let first = compile_onboarding_home_write_plan(&report, &entries, date).unwrap();
    let second = compile_onboarding_home_write_plan(&report, &entries, date).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.writes().len(), 5);

    for write in first.writes() {
        let path = write.relative_path();
        assert!(!path.is_absolute());
        assert!(path.starts_with("memory") || path.starts_with("agents"));
        assert!(!path
            .components()
            .any(|part| matches!(part, Component::ParentDir)));
    }
    assert!(first.writes()[1]
        .relative_path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("agent-research-reviewer-"));
    assert!(first.writes()[2]
        .relative_path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("agent-shell-runner-"));

    for (write, original) in first.writes()[3..].iter().zip(&entries) {
        let frontmatter = format!(
            "---\nsource: {}\nimport_date: 2026-07-21\n---\n",
            serde_json::to_string(&original.source_provenance).unwrap()
        );
        assert!(write.bytes().starts_with(frontmatter.as_bytes()));
        assert_eq!(
            &write.bytes()[frontmatter.len()..],
            original.text.as_bytes()
        );
    }
}

#[test]
fn rejects_empty_inputs_and_disambiguates_duplicate_destinations() {
    let valid_report = report(&["One", "Two"]);
    let date = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    assert_eq!(
        compile_onboarding_home_write_plan(&valid_report, &[], date),
        Err(OnboardingHomeWritePlanError::EmptyInput)
    );
    assert_eq!(
        compile_onboarding_home_write_plan(&valid_report, &[entry("x", "p", "")], date),
        Err(OnboardingHomeWritePlanError::EmptyInput)
    );

    let duplicate = entry("same.md", "same-source", "body");
    let duplicate_plan =
        compile_onboarding_home_write_plan(&valid_report, &[duplicate.clone(), duplicate], date)
            .unwrap();
    assert_ne!(
        duplicate_plan.writes()[3].relative_path(),
        duplicate_plan.writes()[4].relative_path()
    );
    let disambiguated = compile_onboarding_home_write_plan(
        &report(&["../", "\\"]),
        &[entry("x", "p", "body")],
        date,
    )
    .unwrap();
    assert_ne!(
        disambiguated.writes()[1].relative_path(),
        disambiguated.writes()[2].relative_path()
    );
}

#[test]
fn revalidates_publicly_constructed_reports() {
    let date = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let entries = [entry("x", "p", "body")];
    let mut invalid = report(&["One", "Two"]);
    invalid.user_type.clear();
    assert_eq!(
        compile_onboarding_home_write_plan(&invalid, &entries, date),
        Err(OnboardingHomeWritePlanError::EmptyInput)
    );

    for (agents, expected) in [
        (vec![], OnboardingHomeWritePlanError::EmptyInput),
        (
            vec!["One".into()],
            OnboardingHomeWritePlanError::InvalidReport,
        ),
        (vec!["".into()], OnboardingHomeWritePlanError::EmptyInput),
        (
            vec!["One".into(); 4],
            OnboardingHomeWritePlanError::InvalidReport,
        ),
    ] {
        let invalid = OnboardingTriageReport {
            user_type: "Builder".into(),
            proposed_home_layout: "Layout".into(),
            starter_agents: agents,
        };
        assert_eq!(
            compile_onboarding_home_write_plan(&invalid, &entries, date),
            Err(expected)
        );
    }

    let mut invalid = report(&["One", "Two"]);
    invalid.proposed_home_layout = "x".repeat(32 * 1024);
    assert_eq!(
        compile_onboarding_home_write_plan(&invalid, &entries, date),
        Err(OnboardingHomeWritePlanError::InvalidReport)
    );
}

#[test]
fn enforces_entry_count_and_total_byte_bounds() {
    let report = report(&["One", "Two"]);
    let date = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
    let too_many = (0..=ONBOARDING_IMPORT_MAX_ENTRIES)
        .map(|index| entry(&format!("{index}.md"), "source", "x"))
        .collect::<Vec<_>>();
    assert_eq!(
        compile_onboarding_home_write_plan(&report, &too_many, date),
        Err(OnboardingHomeWritePlanError::TooManyEntries)
    );

    let too_large = entry(
        "large.md",
        "source",
        &"x".repeat(ONBOARDING_IMPORT_MAX_TOTAL_BYTES),
    );
    assert_eq!(
        compile_onboarding_home_write_plan(&report, &[too_large], date),
        Err(OnboardingHomeWritePlanError::DocumentBytesExceeded)
    );

    let oversized_aggregate = [
        entry("one.md", "source-one", &"x".repeat(200 * 1024)),
        entry("two.md", "source-two", &"y".repeat(200 * 1024)),
    ];
    assert_eq!(
        compile_onboarding_home_write_plan(&report, &oversized_aggregate, date),
        Err(OnboardingHomeWritePlanError::DocumentBytesExceeded)
    );
}

#[test]
fn persists_a_multi_file_plan_byte_for_byte() {
    let home = temp_home("success");
    let plan = OnboardingHomeWritePlan {
        writes: vec![
            HomeWrite {
                relative_path: "memory/imports/one.md".into(),
                contents: "one\r\n\0".into(),
            },
            HomeWrite {
                relative_path: "agents/two.md".into(),
                contents: "café 🦀\n".into(),
            },
        ],
    };

    persist_onboarding_home_write_plan(&home, &plan).unwrap();
    assert_eq!(
        fs::read(home.join("memory/imports/one.md")).unwrap(),
        b"one\r\n\0"
    );
    assert_eq!(
        fs::read(home.join("agents/two.md")).unwrap(),
        "café 🦀\n".as_bytes()
    );
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn existing_destination_refuses_the_whole_plan_and_rolls_back_created_directories() {
    let home = temp_home("conflict");
    fs::create_dir(home.join("agents")).unwrap();
    fs::write(home.join("agents/existing.md"), b"authoritative").unwrap();
    let plan = OnboardingHomeWritePlan {
        writes: vec![
            HomeWrite {
                relative_path: "memory/new/note.md".into(),
                contents: "new".into(),
            },
            HomeWrite {
                relative_path: "agents/existing.md".into(),
                contents: "replacement".into(),
            },
        ],
    };

    assert!(persist_onboarding_home_write_plan(&home, &plan).is_err());
    assert_eq!(
        fs::read(home.join("agents/existing.md")).unwrap(),
        b"authoritative"
    );
    assert!(!home.join("memory").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_unsafe_destinations_and_symlink_ancestors() {
    use std::os::unix::fs::symlink;
    let home = temp_home("unsafe");
    let outside = temp_home("outside");
    symlink(&outside, home.join("memory")).unwrap();
    let unsafe_plans = [
        HomeWrite {
            relative_path: "../escape.md".into(),
            contents: "bad".into(),
        },
        HomeWrite {
            relative_path: "memory/followed.md".into(),
            contents: "bad".into(),
        },
    ];
    for write in unsafe_plans {
        assert!(persist_onboarding_home_write_plan(
            &home,
            &OnboardingHomeWritePlan {
                writes: vec![write]
            }
        )
        .is_err());
    }
    assert!(walk(&outside).is_empty());
    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn rejects_non_normalized_destination_aliases() {
    let home = temp_home("aliases");
    for relative_path in ["memory//x.md", "memory/./x.md"] {
        let plan = OnboardingHomeWritePlan {
            writes: vec![HomeWrite {
                relative_path: relative_path.into(),
                contents: "bad".into(),
            }],
        };
        assert!(persist_onboarding_home_write_plan(&home, &plan).is_err());
    }
    let plan = OnboardingHomeWritePlan {
        writes: vec![
            HomeWrite {
                relative_path: "memory/x.md".into(),
                contents: "one".into(),
            },
            HomeWrite {
                relative_path: "memory/./x.md".into(),
                contents: "two".into(),
            },
        ],
    };
    assert!(persist_onboarding_home_write_plan(&home, &plan).is_err());
    assert!(!home.join("memory").exists());
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn mid_batch_publication_failure_rolls_back_files_and_temporaries() {
    let home = temp_home("mid-batch");
    let mut writes = (0..=ONBOARDING_IMPORT_MAX_ENTRIES)
        .map(|index| HomeWrite {
            relative_path: format!("memory/batch/{index:03}.md"),
            contents: "payload".repeat(128),
        })
        .collect::<Vec<_>>();
    writes.push(HomeWrite {
        relative_path: "agents/final.md".into(),
        contents: "final".into(),
    });
    let watched_home = home.clone();
    let interferer = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if walk(&watched_home)
                .iter()
                .any(|path| path.to_string_lossy().ends_with(".tmp"))
                && fs::create_dir(watched_home.join("agents/final.md")).is_ok()
            {
                return true;
            }
            std::thread::yield_now();
        }
        false
    });

    assert!(
        persist_onboarding_home_write_plan(&home, &OnboardingHomeWritePlan { writes }).is_err()
    );
    assert!(
        interferer.join().unwrap(),
        "did not reach staged mid-batch state"
    );
    assert!(home.join("agents/final.md").is_dir());
    assert!(!home.join("memory").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

fn two_file_plan() -> OnboardingHomeWritePlan {
    OnboardingHomeWritePlan {
        writes: vec![
            HomeWrite {
                relative_path: "memory/nested/one.md".into(),
                contents: "one".into(),
            },
            HomeWrite {
                relative_path: "memory/nested/two.md".into(),
                contents: "two".into(),
            },
        ],
    }
}

#[test]
fn transient_failures_at_each_transaction_phase_are_fully_rolled_back() {
    for phase in [
        OnboardingPersistHook::AfterPublish,
        OnboardingPersistHook::BeforeTemporaryRemoval,
        OnboardingPersistHook::BeforeDirectorySync,
    ] {
        let home = temp_home("phase-failure");
        let mut failed = false;
        let mut hook = |point, index| {
            if !failed
                && point == phase
                && (phase != OnboardingPersistHook::AfterPublish || index == 0)
            {
                failed = true;
                return Err(std::io::Error::other("injected phase failure"));
            }
            Ok(())
        };
        assert!(
            persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook)
                .is_err()
        );
        assert!(failed);
        assert!(!home.join("memory").exists());
        assert!(!walk(&home)
            .iter()
            .any(|path| path.to_string_lossy().ends_with(".tmp")));
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn transient_rollback_failure_is_retried_before_returning() {
    let home = temp_home("rollback-retry");
    let mut publication_failed = false;
    let mut rollback_failed = false;
    let mut hook = |point, index| match point {
        OnboardingPersistHook::AfterPublish if index == 0 && !publication_failed => {
            publication_failed = true;
            Err(std::io::Error::other("stop publication"))
        }
        OnboardingPersistHook::BeforeRollback if !rollback_failed => {
            rollback_failed = true;
            Err(std::io::Error::other("transient rollback failure"))
        }
        _ => Ok(()),
    };
    assert!(
        persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook).is_err()
    );
    assert!(publication_failed && rollback_failed);
    assert!(!home.join("memory").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn replaced_visible_ancestor_prevents_success_and_is_not_removed() {
    let home = temp_home("ancestor-race");
    let moved = home.join("moved-by-racer");
    let mut replaced = false;
    let mut hook = |point, _| {
        if point == OnboardingPersistHook::AfterStaging && !replaced {
            fs::rename(home.join("memory"), &moved)?;
            fs::create_dir(home.join("memory"))?;
            fs::write(home.join("memory/racer.md"), b"racer")?;
            replaced = true;
        }
        Ok(())
    };
    assert!(
        persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook).is_err()
    );
    assert_eq!(fs::read(home.join("memory/racer.md")).unwrap(), b"racer");
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn replaced_published_destination_prevents_success_and_is_not_removed() {
    let home = temp_home("destination-race");
    let mut replaced = false;
    let mut hook = |point, index| {
        if point == OnboardingPersistHook::AfterPublish && index == 0 && !replaced {
            let destination = home.join("memory/nested/one.md");
            fs::rename(&destination, home.join("memory/nested/moved-by-racer.md"))?;
            fs::write(&destination, b"racer")?;
            replaced = true;
        }
        Ok(())
    };
    assert!(
        persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook).is_err()
    );
    assert_eq!(
        fs::read(home.join("memory/nested/one.md")).unwrap(),
        b"racer"
    );
    assert!(!home.join("memory/nested/two.md").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if fs::symlink_metadata(&path)
                .map(|metadata| metadata.is_dir())
                .unwrap_or(false)
            {
                pending.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths
}
