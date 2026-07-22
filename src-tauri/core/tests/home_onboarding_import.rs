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
    let mut failed = false;
    let mut hook = |point, index| {
        if point == OnboardingPersistHook::Publish && index == 1 && !failed {
            failed = true;
            return Err(std::io::Error::other("injected mid-batch failure"));
        }
        Ok(())
    };
    assert!(
        persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook).is_err()
    );
    assert!(failed);
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

#[test]
fn persistent_operation_failures_are_recovered_by_the_next_locked_import() {
    for operation in [
        OnboardingPersistHook::RemoveTemporary,
        OnboardingPersistHook::RollbackFile,
        OnboardingPersistHook::RollbackDirectory,
        OnboardingPersistHook::RollbackSync,
    ] {
        let home = temp_home("persistent-recovery");
        let mut hook = |point, index| {
            if point == OnboardingPersistHook::AfterPublish && index == 0 {
                return Err(std::io::Error::other("force rollback"));
            }
            if point == operation {
                return Err(std::io::Error::other("persistent operation failure"));
            }
            Ok(())
        };
        assert!(
            persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook)
                .is_err()
        );

        let adjacent = OnboardingHomeWritePlan {
            writes: vec![HomeWrite {
                relative_path: "agents/recovered.md".into(),
                contents: "recovered".into(),
            }],
        };
        persist_onboarding_home_write_plan(&home, &adjacent).unwrap();
        assert_eq!(
            fs::read(home.join("agents/recovered.md")).unwrap(),
            b"recovered"
        );
        assert!(!home.join("memory").exists());
        assert!(!walk(&home).iter().any(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.ends_with(".tmp") || name.ends_with(".txn")
        }));
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn publication_and_directory_sync_operation_failures_roll_back() {
    for operation in [
        OnboardingPersistHook::Publish,
        OnboardingPersistHook::SyncDirectory,
    ] {
        let home = temp_home("operation-failure");
        let mut failed = false;
        let mut hook = |point, _| {
            if point == operation && !failed {
                failed = true;
                return Err(std::io::Error::other("injected operation failure"));
            }
            Ok(())
        };
        assert!(
            persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook)
                .is_err()
        );
        assert!(failed);
        assert!(!home.join("memory").exists());
        assert!(!walk(&home).iter().any(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.ends_with(".tmp") || name.ends_with(".txn")
        }));
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn persistent_staging_and_retirement_failures_are_recovered() {
    for operation in [
        OnboardingPersistHook::WriteJournal,
        OnboardingPersistHook::SyncJournal,
        OnboardingPersistHook::ReplaceJournal,
        OnboardingPersistHook::WritePayload,
        OnboardingPersistHook::SyncPayload,
        OnboardingPersistHook::RetireAnchor,
        OnboardingPersistHook::RetireJournal,
        OnboardingPersistHook::RetireTransaction,
    ] {
        let home = temp_home("journal-recovery");
        let mut hook = |point, _| {
            if point == operation {
                Err(std::io::Error::other("persistent journal failure"))
            } else {
                Ok(())
            }
        };
        assert!(
            persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook)
                .is_err()
        );
        let adjacent = OnboardingHomeWritePlan {
            writes: vec![HomeWrite {
                relative_path: "agents/after-recovery.md".into(),
                contents: "after".into(),
            }],
        };
        persist_onboarding_home_write_plan(&home, &adjacent).unwrap();
        assert_eq!(
            fs::read(home.join("agents/after-recovery.md")).unwrap(),
            b"after"
        );
        assert!(!walk(&home).iter().any(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.ends_with(".tmp") || name.ends_with(".txn")
        }));
        fs::remove_dir_all(home).unwrap();
    }
}

#[test]
fn failure_before_the_commit_decision_is_rolled_back_on_recovery() {
    let home = temp_home("pre-commit-recovery");
    let mut hook = |point, _| {
        if point == OnboardingPersistHook::WriteCommitDecision {
            Err(std::io::Error::other("commit decision unavailable"))
        } else {
            Ok(())
        }
    };
    assert!(
        persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook).is_err()
    );
    assert!(!home.join("memory/nested/one.md").exists());
    assert!(!home.join("memory/nested/two.md").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".txn")));

    let different = OnboardingHomeWritePlan {
        writes: vec![HomeWrite {
            relative_path: "agents/different.md".into(),
            contents: "different".into(),
        }],
    };
    persist_onboarding_home_write_plan(&home, &different).unwrap();
    assert_eq!(
        fs::read(home.join("agents/different.md")).unwrap(),
        b"different"
    );
    assert!(!home.join("memory/nested/one.md").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".txn")));
    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn recovery_rejects_a_foreign_regular_transaction_without_touching_hard_links() {
    let home = temp_home("foreign-regular-transaction");
    let outside = temp_home("foreign-regular-transaction-outside");
    fs::create_dir_all(home.join("memory/nested")).unwrap();
    fs::write(home.join("memory/existing.md"), b"existing home").unwrap();
    fs::write(outside.join("outside.md"), b"existing outside").unwrap();

    let transaction = home.join(".onboarding-import-foreign.txn");
    fs::create_dir(&transaction).unwrap();
    fs::hard_link(
        home.join("memory/existing.md"),
        transaction.join("payload-0"),
    )
    .unwrap();
    fs::write(
        transaction.join("manifest.json"),
        br#"{"committed":false,"entries":[{"parent":"memory","destination":"existing.md","temporary":"payload-0","anchor":"payload-0"}],"created_directories":[]}"#,
    )
    .unwrap();

    assert!(persist_onboarding_home_write_plan(&home, &two_file_plan()).is_err());
    assert_eq!(
        fs::read(home.join("memory/existing.md")).unwrap(),
        b"existing home"
    );
    assert_eq!(
        fs::read(outside.join("outside.md")).unwrap(),
        b"existing outside"
    );
    assert!(transaction.join("manifest.json").exists());
    assert!(transaction.join("payload-0").exists());

    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn commit_decision_and_cleanup_failures_never_undo_a_committed_batch() {
    for operation in [
        OnboardingPersistHook::SyncCommitDecision,
        OnboardingPersistHook::SyncCommitDirectory,
        OnboardingPersistHook::RetireAnchor,
        OnboardingPersistHook::RetireJournal,
        OnboardingPersistHook::RetireTransaction,
    ] {
        let home = temp_home("commit-recovery");
        let mut hook = |point, _| {
            if point == operation {
                Err(std::io::Error::other("persistent finalization failure"))
            } else {
                Ok(())
            }
        };
        assert!(
            persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook)
                .is_err()
        );
        assert_eq!(fs::read(home.join("memory/nested/one.md")).unwrap(), b"one");
        assert_eq!(fs::read(home.join("memory/nested/two.md")).unwrap(), b"two");
        let adjacent = OnboardingHomeWritePlan {
            writes: vec![HomeWrite {
                relative_path: "agents/after-commit.md".into(),
                contents: "kept".into(),
            }],
        };
        persist_onboarding_home_write_plan(&home, &adjacent).unwrap();
        assert_eq!(fs::read(home.join("memory/nested/one.md")).unwrap(), b"one");
        assert!(!walk(&home)
            .iter()
            .any(|path| path.to_string_lossy().ends_with(".txn")));
        fs::remove_dir_all(home).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn recovery_rejects_transaction_symlinks_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let home = temp_home("transaction-symlink");
    let outside = temp_home("transaction-symlink-outside");
    fs::write(outside.join("manifest.json"), b"outside journal").unwrap();
    fs::write(outside.join("payload-0"), b"outside payload").unwrap();
    symlink(&outside, home.join(".onboarding-import-foreign.txn")).unwrap();

    assert!(persist_onboarding_home_write_plan(&home, &two_file_plan()).is_err());
    assert_eq!(
        fs::read(outside.join("manifest.json")).unwrap(),
        b"outside journal"
    );
    assert_eq!(
        fs::read(outside.join("payload-0")).unwrap(),
        b"outside payload"
    );

    fs::remove_file(home.join(".onboarding-import-foreign.txn")).unwrap();
    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(outside).unwrap();
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
    fs::create_dir_all(home.join("memory/nested")).unwrap();
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
    assert!(!home.join("memory/nested/moved-by-racer.md").exists());
    assert!(!walk(&home)
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".tmp")));
    fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
#[test]
fn replaced_whole_home_prevents_success_and_cleans_the_original_home() {
    let home = temp_home("whole-home-race");
    let moved = home.with_extension("moved-by-racer");
    let mut replaced = false;
    let mut hook = |point, index| {
        if point == OnboardingPersistHook::AfterPublish && index == 0 && !replaced {
            fs::rename(&home, &moved)?;
            fs::create_dir(&home)?;
            fs::write(home.join("racer.md"), b"racer")?;
            replaced = true;
        }
        Ok(())
    };
    assert!(
        persist_onboarding_home_write_plan_with_hook(&home, &two_file_plan(), &mut hook).is_err()
    );
    assert_eq!(fs::read(home.join("racer.md")).unwrap(), b"racer");
    assert!(!moved.join("memory").exists());
    assert!(!walk(&moved).iter().any(|path| {
        let name = path.file_name().unwrap().to_string_lossy();
        name.ends_with(".tmp") || name.ends_with(".txn")
    }));
    fs::remove_dir_all(home).unwrap();
    fs::remove_dir_all(moved).unwrap();
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
