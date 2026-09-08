use super::support::{
    ConflictStageFixture, git_command, require_git_shell_for_status_integration_tests, run_git,
    run_git_expect_failure, run_git_output, set_unmerged_stages, setup_both_added_text_conflict,
    write,
};
use std::fs;
use std::path::{Path, PathBuf};
use worktree_core::conflict_session::{ConflictPayload, ConflictResolverStrategy};
use worktree_core::domain::{FileConflictKind, FileStatusKind};
use worktree_core::services::{ConflictSide, GitBackend};
use worktree_git_gix::GixBackend;
use worktree_test_support::hash_blob;

#[test]
fn status_and_conflict_stages_cover_all_conflict_kinds() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "seed.txt", "seed\n");
    run_git(repo, &["add", "seed.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    let base_blob = hash_blob(repo, b"base\n");
    let ours_blob = hash_blob(repo, b"ours\n");
    let theirs_blob = hash_blob(repo, b"theirs\n");

    let fixtures = [
        ConflictStageFixture {
            path: "dd.txt",
            kind: FileConflictKind::BothDeleted,
            has_base: true,
            has_ours: false,
            has_theirs: false,
        },
        ConflictStageFixture {
            path: "au.txt",
            kind: FileConflictKind::AddedByUs,
            has_base: false,
            has_ours: true,
            has_theirs: false,
        },
        ConflictStageFixture {
            path: "ud.txt",
            kind: FileConflictKind::DeletedByThem,
            has_base: true,
            has_ours: true,
            has_theirs: false,
        },
        ConflictStageFixture {
            path: "ua.txt",
            kind: FileConflictKind::AddedByThem,
            has_base: false,
            has_ours: false,
            has_theirs: true,
        },
        ConflictStageFixture {
            path: "du.txt",
            kind: FileConflictKind::DeletedByUs,
            has_base: true,
            has_ours: false,
            has_theirs: true,
        },
        ConflictStageFixture {
            path: "aa.txt",
            kind: FileConflictKind::BothAdded,
            has_base: false,
            has_ours: true,
            has_theirs: true,
        },
        ConflictStageFixture {
            path: "uu.txt",
            kind: FileConflictKind::BothModified,
            has_base: true,
            has_ours: true,
            has_theirs: true,
        },
    ];

    for fixture in &fixtures {
        set_unmerged_stages(
            repo,
            fixture.path,
            fixture.has_base.then_some(base_blob.as_str()),
            fixture.has_ours.then_some(ours_blob.as_str()),
            fixture.has_theirs.then_some(theirs_blob.as_str()),
        );
    }

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();

    for fixture in &fixtures {
        let path = Path::new(fixture.path);
        let status_entry = status
            .unstaged
            .iter()
            .find(|e| e.path == path)
            .unwrap_or_else(|| panic!("missing status entry for {}", fixture.path));
        assert_eq!(
            status_entry.kind,
            FileStatusKind::Conflicted,
            "expected conflicted kind for {}",
            fixture.path
        );
        assert_eq!(
            status_entry.conflict,
            Some(fixture.kind),
            "wrong conflict kind for {}",
            fixture.path
        );

        assert!(
            !status.staged.iter().any(|e| e.path == path),
            "conflicted path {} should not appear in staged status",
            fixture.path
        );

        let stages = opened
            .conflict_file_stages(path)
            .unwrap()
            .expect("conflict stages");
        assert_eq!(
            stages.base.is_some(),
            fixture.has_base,
            "base stage mismatch for {}",
            fixture.path
        );
        if stages.base.is_some() {
            assert!(
                stages.base_bytes.is_none(),
                "utf-8 base stage should not retain duplicate bytes for {}",
                fixture.path
            );
        }
        assert_eq!(
            stages.ours.is_some(),
            fixture.has_ours,
            "ours stage mismatch for {}",
            fixture.path
        );
        if stages.ours.is_some() {
            assert!(
                stages.ours_bytes.is_none(),
                "utf-8 ours stage should not retain duplicate bytes for {}",
                fixture.path
            );
        }
        assert_eq!(
            stages.theirs.is_some(),
            fixture.has_theirs,
            "theirs stage mismatch for {}",
            fixture.path
        );
        if stages.theirs.is_some() {
            assert!(
                stages.theirs_bytes.is_none(),
                "utf-8 theirs stage should not retain duplicate bytes for {}",
                fixture.path
            );
        }

        let session = opened
            .conflict_session(path)
            .unwrap()
            .expect("conflict session");
        assert_eq!(session.path, PathBuf::from(fixture.path));
        assert_eq!(session.conflict_kind, fixture.kind);
        assert_eq!(
            session.strategy,
            ConflictResolverStrategy::for_conflict(fixture.kind, false)
        );
        assert_eq!(session.base.is_absent(), !fixture.has_base);
        assert_eq!(session.ours.is_absent(), !fixture.has_ours);
        assert_eq!(session.theirs.is_absent(), !fixture.has_theirs);
    }
}

#[test]
fn checkout_conflict_side_resolves_all_conflict_stage_shapes() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    #[derive(Clone, Copy)]
    struct ConflictCheckoutFixture {
        kind: FileConflictKind,
        has_base: bool,
        has_ours: bool,
        has_theirs: bool,
    }

    let fixtures = [
        ConflictCheckoutFixture {
            kind: FileConflictKind::BothDeleted,
            has_base: true,
            has_ours: false,
            has_theirs: false,
        },
        ConflictCheckoutFixture {
            kind: FileConflictKind::AddedByUs,
            has_base: false,
            has_ours: true,
            has_theirs: false,
        },
        ConflictCheckoutFixture {
            kind: FileConflictKind::DeletedByThem,
            has_base: true,
            has_ours: true,
            has_theirs: false,
        },
        ConflictCheckoutFixture {
            kind: FileConflictKind::AddedByThem,
            has_base: false,
            has_ours: false,
            has_theirs: true,
        },
        ConflictCheckoutFixture {
            kind: FileConflictKind::DeletedByUs,
            has_base: true,
            has_ours: false,
            has_theirs: true,
        },
        ConflictCheckoutFixture {
            kind: FileConflictKind::BothAdded,
            has_base: false,
            has_ours: true,
            has_theirs: true,
        },
        ConflictCheckoutFixture {
            kind: FileConflictKind::BothModified,
            has_base: true,
            has_ours: true,
            has_theirs: true,
        },
    ];

    for fixture in fixtures {
        for side in [ConflictSide::Ours, ConflictSide::Theirs] {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path();

            run_git(repo, &["init"]);
            run_git(repo, &["config", "user.email", "you@example.com"]);
            run_git(repo, &["config", "user.name", "You"]);
            run_git(repo, &["config", "commit.gpgsign", "false"]);

            write(repo, "seed.txt", "seed\n");
            run_git(repo, &["add", "seed.txt"]);
            run_git(
                repo,
                &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
            );

            let base_blob = hash_blob(repo, b"base\n");
            let ours_blob = hash_blob(repo, b"ours\n");
            let theirs_blob = hash_blob(repo, b"theirs\n");

            set_unmerged_stages(
                repo,
                "a.txt",
                fixture.has_base.then_some(base_blob.as_str()),
                fixture.has_ours.then_some(ours_blob.as_str()),
                fixture.has_theirs.then_some(theirs_blob.as_str()),
            );

            let backend = GixBackend;
            let opened = backend.open(repo).unwrap();

            let before = opened.status().unwrap();
            let conflict_entry = before
                .unstaged
                .iter()
                .find(|e| e.path == Path::new("a.txt"))
                .expect("expected staged-shape fixture to appear as conflict");
            assert_eq!(conflict_entry.kind, FileStatusKind::Conflicted);
            assert_eq!(conflict_entry.conflict, Some(fixture.kind));

            opened
                .checkout_conflict_side(Path::new("a.txt"), side)
                .unwrap();

            let after = opened.status().unwrap();
            let selected_stage_exists = match side {
                ConflictSide::Ours => fixture.has_ours,
                ConflictSide::Theirs => fixture.has_theirs,
            };

            if selected_stage_exists {
                let expected_bytes: &[u8] = match side {
                    ConflictSide::Ours => b"ours\n",
                    ConflictSide::Theirs => b"theirs\n",
                };
                assert_eq!(fs::read(repo.join("a.txt")).unwrap(), expected_bytes);
                assert!(
                    after
                        .staged
                        .iter()
                        .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Added),
                    "expected selected side to stage added file for {:?} with {:?}; status={after:?}",
                    fixture.kind,
                    side
                );
                assert!(
                    after.unstaged.iter().all(|e| e.path != Path::new("a.txt")),
                    "expected conflict path to disappear from unstaged after resolving {:?} with {:?}; status={after:?}",
                    fixture.kind,
                    side
                );
            } else {
                assert!(
                    !repo.join("a.txt").exists(),
                    "expected path to be removed when chosen stage is missing for {:?} with {:?}",
                    fixture.kind,
                    side
                );
                assert!(
                    after
                        .staged
                        .iter()
                        .chain(after.unstaged.iter())
                        .all(|e| e.path != Path::new("a.txt")),
                    "expected no status entry for removed path after resolving {:?} with {:?}; status={after:?}",
                    fixture.kind,
                    side
                );
            }
        }
    }
}

#[test]
fn accept_conflict_deletion_resolves_delete_outcome_conflicts() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    #[derive(Clone, Copy)]
    struct ConflictDeleteFixture {
        kind: FileConflictKind,
        has_base: bool,
        has_ours: bool,
        has_theirs: bool,
    }

    let fixtures = [
        ConflictDeleteFixture {
            kind: FileConflictKind::BothDeleted,
            has_base: true,
            has_ours: false,
            has_theirs: false,
        },
        ConflictDeleteFixture {
            kind: FileConflictKind::AddedByUs,
            has_base: false,
            has_ours: true,
            has_theirs: false,
        },
        ConflictDeleteFixture {
            kind: FileConflictKind::AddedByThem,
            has_base: false,
            has_ours: false,
            has_theirs: true,
        },
        ConflictDeleteFixture {
            kind: FileConflictKind::DeletedByUs,
            has_base: true,
            has_ours: false,
            has_theirs: true,
        },
        ConflictDeleteFixture {
            kind: FileConflictKind::DeletedByThem,
            has_base: true,
            has_ours: true,
            has_theirs: false,
        },
    ];

    for fixture in fixtures {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();

        run_git(repo, &["init"]);
        run_git(repo, &["config", "user.email", "you@example.com"]);
        run_git(repo, &["config", "user.name", "You"]);
        run_git(repo, &["config", "commit.gpgsign", "false"]);

        write(repo, "seed.txt", "seed\n");
        run_git(repo, &["add", "seed.txt"]);
        run_git(
            repo,
            &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
        );

        let base_blob = hash_blob(repo, b"base\n");
        let ours_blob = hash_blob(repo, b"ours\n");
        let theirs_blob = hash_blob(repo, b"theirs\n");

        set_unmerged_stages(
            repo,
            "a.txt",
            fixture.has_base.then_some(base_blob.as_str()),
            fixture.has_ours.then_some(ours_blob.as_str()),
            fixture.has_theirs.then_some(theirs_blob.as_str()),
        );

        let backend = GixBackend;
        let opened = backend.open(repo).unwrap();

        let before = opened.status().unwrap();
        let conflict_entry = before
            .unstaged
            .iter()
            .find(|e| e.path == Path::new("a.txt"))
            .expect("expected fixture path to appear as conflict");
        assert_eq!(conflict_entry.kind, FileStatusKind::Conflicted);
        assert_eq!(conflict_entry.conflict, Some(fixture.kind));

        opened.accept_conflict_deletion(Path::new("a.txt")).unwrap();

        let after = opened.status().unwrap();
        assert!(
            !repo.join("a.txt").exists(),
            "expected path to be removed after accepting deletion for {:?}",
            fixture.kind
        );
        assert!(
            after
                .staged
                .iter()
                .chain(after.unstaged.iter())
                .all(|e| e.path != Path::new("a.txt")),
            "expected no status entry for deleted path after resolving {:?}; status={after:?}",
            fixture.kind
        );
    }
}

#[test]
fn status_reports_single_conflict_for_modify_delete() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    run_git(repo, &["rm", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours_delete"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();

    let entries = status
        .unstaged
        .iter()
        .filter(|e| e.path == Path::new("a.txt"))
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one status entry for a.txt, got {:#?}",
        status.unstaged
    );
    assert_eq!(entries[0].kind, FileStatusKind::Conflicted);
    assert_eq!(entries[0].conflict, Some(FileConflictKind::DeletedByUs));
}

#[test]
fn status_reports_conflict_kind_for_add_add() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "base.txt", "base\n");
    run_git(repo, &["add", "base.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs_add"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "a.txt", "ours\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours_add"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();
    assert_eq!(status.unstaged.len(), 1);
    assert_eq!(status.unstaged[0].path, PathBuf::from("a.txt"));
    assert_eq!(status.unstaged[0].kind, FileStatusKind::Conflicted);
    assert_eq!(
        status.unstaged[0].conflict,
        Some(FileConflictKind::BothAdded)
    );
}

#[test]
fn conflict_file_stages_preserve_non_utf8_bytes() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let base_bytes = b"\x00base\xff\n".to_vec();
    let ours_bytes = b"\x00ours\xff\n".to_vec();
    let theirs_bytes = b"\x00theirs\xff\n".to_vec();

    write(repo, "bin.dat", &base_bytes);
    run_git(repo, &["add", "bin.dat"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "bin.dat", &theirs_bytes);
    run_git(repo, &["add", "bin.dat"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "bin.dat", &ours_bytes);
    run_git(repo, &["add", "bin.dat"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let stages = opened
        .conflict_file_stages(Path::new("bin.dat"))
        .unwrap()
        .expect("conflict stage data");

    assert_eq!(stages.path, PathBuf::from("bin.dat"));
    assert_eq!(stages.base_bytes.as_deref(), Some(base_bytes.as_slice()));
    assert_eq!(stages.ours_bytes.as_deref(), Some(ours_bytes.as_slice()));
    assert_eq!(
        stages.theirs_bytes.as_deref(),
        Some(theirs_bytes.as_slice())
    );
    assert_eq!(stages.base, None);
    assert_eq!(stages.ours, None);
    assert_eq!(stages.theirs, None);

    let session = opened
        .conflict_session(Path::new("bin.dat"))
        .unwrap()
        .expect("conflict session");
    assert_eq!(session.path, PathBuf::from("bin.dat"));
    assert_eq!(session.strategy, ConflictResolverStrategy::BinarySidePick);
    assert_eq!(session.total_regions(), 1);
    assert_eq!(session.unsolved_count(), 1);
    assert!(!session.is_fully_resolved());
    assert!(matches!(session.base, ConflictPayload::Binary(_)));
    assert!(matches!(session.ours, ConflictPayload::Binary(_)));
    assert!(matches!(session.theirs, ConflictPayload::Binary(_)));
}

#[test]
fn checkout_conflict_side_resolves_non_utf8_binary_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let base_bytes = b"\x00base\xff\n".to_vec();
    let ours_bytes = b"\x00ours\xff\n".to_vec();
    let theirs_bytes = b"\x00theirs\xff\n".to_vec();

    write(repo, "bin.dat", &base_bytes);
    run_git(repo, &["add", "bin.dat"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "bin.dat", &theirs_bytes);
    run_git(repo, &["add", "bin.dat"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "bin.dat", &ours_bytes);
    run_git(repo, &["add", "bin.dat"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let session = opened
        .conflict_session(Path::new("bin.dat"))
        .unwrap()
        .expect("binary conflict session");
    assert_eq!(session.strategy, ConflictResolverStrategy::BinarySidePick);

    opened
        .checkout_conflict_side(Path::new("bin.dat"), ConflictSide::Theirs)
        .unwrap();

    assert_eq!(fs::read(repo.join("bin.dat")).unwrap(), theirs_bytes);

    let status_after = opened.status().unwrap();
    assert!(
        !status_after
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("bin.dat") && e.kind == FileStatusKind::Conflicted),
        "binary conflict should be cleared after choosing theirs"
    );
    assert!(
        status_after
            .staged
            .iter()
            .any(|e| e.path == Path::new("bin.dat")),
        "chosen binary side should be staged"
    );
}

#[test]
fn conflict_session_both_deleted_binary_prefers_decision_strategy() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "seed.txt", "seed\n");
    run_git(repo, &["add", "seed.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    let base_blob = hash_blob(repo, b"\x00base\xff\n");
    set_unmerged_stages(repo, "gone.bin", Some(base_blob.as_str()), None, None);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|e| e.path == Path::new("gone.bin"))
        .expect("expected conflict status entry");
    assert_eq!(entry.kind, FileStatusKind::Conflicted);
    assert_eq!(entry.conflict, Some(FileConflictKind::BothDeleted));

    let session = opened
        .conflict_session(Path::new("gone.bin"))
        .unwrap()
        .expect("conflict session");
    assert_eq!(session.conflict_kind, FileConflictKind::BothDeleted);
    assert_eq!(session.strategy, ConflictResolverStrategy::DecisionOnly);
    assert!(matches!(session.base, ConflictPayload::Binary(_)));
    assert!(session.ours.is_absent());
    assert!(session.theirs.is_absent());
}

#[test]
fn checkout_conflict_side_resolves_modify_delete_using_ours() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    run_git(repo, &["rm", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours_delete"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened
        .checkout_conflict_side(Path::new("a.txt"), ConflictSide::Ours)
        .unwrap();

    assert!(
        !repo.join("a.txt").exists(),
        "expected ours resolution to remove file from worktree"
    );
    let status = opened.status().unwrap();
    assert!(
        !status
            .staged
            .iter()
            .chain(status.unstaged.iter())
            .any(|e| e.path == Path::new("a.txt")),
        "expected ours resolution to clear status entries for a.txt, got {status:?}"
    );
}

#[test]
fn checkout_conflict_side_resolves_modify_delete_using_theirs() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    run_git(repo, &["rm", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours_delete"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened
        .checkout_conflict_side(Path::new("a.txt"), ConflictSide::Theirs)
        .unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "theirs\n",
        "expected theirs resolution to restore file contents"
    );
    let status = opened.status().unwrap();
    assert_eq!(
        status.unstaged,
        Vec::new(),
        "expected theirs resolution to clear unstaged entries"
    );
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Added),
        "expected theirs resolution to stage file as added, got {status:?}"
    );
}

#[test]
fn checkout_conflict_side_stages_resolution() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "a.txt", "ours\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened
        .checkout_conflict_side(Path::new("a.txt"), ConflictSide::Theirs)
        .unwrap();

    let status = opened.status().unwrap();
    assert!(status.unstaged.iter().all(|s| s.path != Path::new("a.txt")));
    assert!(
        status
            .staged
            .iter()
            .any(|s| s.path == Path::new("a.txt") && s.kind == FileStatusKind::Modified)
    );

    let on_disk = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert_eq!(on_disk, "theirs\n");
}

#[test]
fn merge_commit_message_is_available_during_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "feature\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "a.txt", "main\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "main"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    assert!(opened.merge_ref_with_output("feature").is_err());

    let msg = opened
        .merge_commit_message()
        .unwrap()
        .expect("merge commit message");
    assert_eq!(
        msg.lines().next().unwrap_or_default(),
        "Merge branch 'feature'"
    );
    assert!(
        !msg.contains('#'),
        "expected message to be cleaned, got: {msg}"
    );

    run_git(repo, &["merge", "--abort"]);
    assert!(opened.merge_commit_message().unwrap().is_none());
}

#[test]
fn commit_finishes_merge_when_resolved_tree_matches_head() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "feature\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "a.txt", "main\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "main"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    assert!(opened.merge_ref_with_output("feature").is_err());
    run_git(repo, &["checkout", "--ours", "a.txt"]);
    run_git(repo, &["add", "a.txt"]);

    let status = opened.status().unwrap();
    assert!(status.staged.is_empty(), "expected no staged changes");
    assert!(status.unstaged.is_empty(), "expected no unstaged changes");

    opened
        .commit("Merge branch 'feature'")
        .expect("merge commit should succeed even without tree changes");

    assert!(opened.merge_commit_message().unwrap().is_none());

    let parents = git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-list", "--parents", "-n", "1", "HEAD"])
        .output()
        .expect("rev-list --parents");
    assert!(parents.status.success());
    let parent_count = String::from_utf8(parents.stdout)
        .unwrap()
        .split_whitespace()
        .count()
        .saturating_sub(1);
    assert_eq!(parent_count, 2, "expected merge commit");
}

/// Unstaging must not disturb a merge in progress: a bare `git reset` collapses
/// unmerged index entries and clears MERGE_HEAD, which turns conflicted files
/// into ordinary modifications still full of conflict markers.
#[test]
fn unstage_all_leaves_conflicted_paths_and_the_merge_alone() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "c.txt", "base\n");
    write(repo, "other.txt", "other\n");
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );
    let base_branch = run_git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let base_branch = base_branch.trim().to_string();

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "c.txt", "theirs\n");
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-am", "theirs"],
    );
    run_git(repo, &["checkout", &base_branch]);
    write(repo, "c.txt", "ours\n");
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-am", "ours"],
    );

    // Conflict on c.txt, plus an unrelated staged change.
    let _ = std::process::Command::new("git")
        .current_dir(repo)
        .args(["merge", "feature"])
        .output();
    write(repo, "other.txt", "other\nstaged\n");
    run_git(repo, &["add", "other.txt"]);

    let conflicted_before = run_git_output(repo, &["ls-files", "-u"]);
    assert!(
        conflicted_before.contains("c.txt"),
        "expected c.txt to be unmerged before unstaging:\n{conflicted_before}"
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened.unstage(&[]).unwrap();

    let conflicted_after = run_git_output(repo, &["ls-files", "-u"]);
    assert!(
        conflicted_after.contains("c.txt"),
        "unstage-all must leave the conflict in the index:\n{conflicted_after}"
    );
    assert!(
        repo.join(".git").join("MERGE_HEAD").exists(),
        "unstage-all must not abort the merge"
    );

    let status = opened.status().unwrap();
    assert!(
        status
            .unstaged
            .iter()
            .any(|entry| entry.path == PathBuf::from("c.txt") && entry.conflict.is_some()),
        "c.txt must still be reported as conflicted: {:?}",
        status.unstaged
    );
    assert!(
        status.staged.is_empty(),
        "the unrelated staged change must still have been unstaged: {:?}",
        status.staged
    );
}

/// The conflict-safe unstage-all resets named paths rather than everything, so
/// it has to name *both* sides of a staged rename. The status list reports only
/// the destination, and resetting that alone leaves the source path staged as
/// deleted — half a rename in the index.
#[test]
fn unstage_all_during_a_merge_resets_both_sides_of_a_staged_rename() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "c.txt", "base\n");
    // Long enough that rename detection scores the move as a rename.
    write(
        repo,
        "old.txt",
        "alpha\nbravo\ncharlie\ndelta\necho\nfoxtrot\n",
    );
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );
    let base_branch = run_git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let base_branch = base_branch.trim().to_string();

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "c.txt", "theirs\n");
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-am", "theirs"],
    );
    run_git(repo, &["checkout", &base_branch]);
    write(repo, "c.txt", "ours\n");
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-am", "ours"],
    );

    // Conflict on c.txt, plus a staged rename that has nothing to do with it.
    let _ = std::process::Command::new("git")
        .current_dir(repo)
        .args(["merge", "feature"])
        .output();
    run_git(repo, &["mv", "old.txt", "new.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened.unstage(&[]).unwrap();

    let staged = run_git_output(repo, &["diff", "--cached", "--name-only"]);
    assert!(
        !staged.lines().any(|line| line == "old.txt"),
        "unstage-all must not leave old.txt staged as deleted:\n{staged}"
    );
    assert!(
        !staged.lines().any(|line| line == "new.txt"),
        "unstage-all must unstage the rename destination too:\n{staged}"
    );

    // The rename itself stays on disk: unstaging only rewrites the index.
    assert!(
        repo.join("new.txt").exists() && !repo.join("old.txt").exists(),
        "unstage-all must not touch the worktree"
    );

    let conflicted_after = run_git_output(repo, &["ls-files", "-u"]);
    assert!(
        conflicted_after.contains("c.txt"),
        "unstage-all must leave the conflict in the index:\n{conflicted_after}"
    );
    assert!(
        repo.join(".git").join("MERGE_HEAD").exists(),
        "unstage-all must not abort the merge"
    );
}

// ---------------------------------------------------------------------------
// End-to-end conflict resolution workflow tests
// ---------------------------------------------------------------------------

/// End-to-end test: create a merge conflict, load the conflict session,
/// resolve all regions manually, generate resolved text, write it to disk,
/// stage the file, and verify the conflict is fully resolved.
#[test]
fn resolve_conflict_write_and_stage_clears_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    // Create a BothModified conflict: both sides change the same lines.
    let base_content = "header\nconflict-line\nfooter\n";
    let ours_content = "header\nours-version\nfooter\n";
    let theirs_content = "header\ntheirs-version\nfooter\n";

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "doc.txt", base_content);
    run_git(repo, &["add", "doc.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "doc.txt", theirs_content);
    run_git(repo, &["add", "doc.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "doc.txt", ours_content);
    run_git(repo, &["add", "doc.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // 1. Verify file is in conflict status
    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|e| e.path == Path::new("doc.txt"))
        .expect("expected conflict entry");
    assert_eq!(entry.kind, FileStatusKind::Conflicted);
    assert_eq!(entry.conflict, Some(FileConflictKind::BothModified));

    // 2. Load conflict session via backend API
    let session = opened
        .conflict_session(Path::new("doc.txt"))
        .unwrap()
        .expect("conflict session");
    assert_eq!(session.strategy, ConflictResolverStrategy::FullTextResolver);
    assert_eq!(session.conflict_kind, FileConflictKind::BothModified);
    let plan = session
        .merge_plan
        .as_ref()
        .expect("full-text Gix session should retain its stage merge plan");
    assert_eq!(session.region_plan_blocks.len(), session.regions.len());
    assert!(
        session
            .region_plan_blocks
            .iter()
            .all(|block_index| plan.blocks.get(*block_index).is_some()),
        "every displayed region should map to a valid plan block",
    );
    let marker_projection = worktree_core::merge::render_merge_plan(
        plan,
        &worktree_core::merge::MergeOptions {
            style: worktree_core::merge::ConflictStyle::Diff3,
            ..Default::default()
        },
    )
    .output;
    let worktree_content = fs::read_to_string(repo.join("doc.txt")).unwrap();
    assert_eq!(session.current_text(), Some(worktree_content.as_str()));
    assert_eq!(
        session.marker_projection_text(),
        Some(marker_projection.as_str())
    );
    assert!(
        marker_projection.contains("|||||||"),
        "stage-backed three-way geometry should include the ancestor section",
    );

    // 3. Verify worktree file contains conflict markers
    let validation = worktree_core::services::validate_conflict_resolution_text(&worktree_content);
    assert!(
        validation.has_conflict_markers,
        "worktree file should contain conflict markers"
    );

    // 4. Write manually resolved content (pick ours version)
    let resolved_content = "header\nours-version\nfooter\n";
    let resolved_validation =
        worktree_core::services::validate_conflict_resolution_text(resolved_content);
    assert!(
        !resolved_validation.has_conflict_markers,
        "resolved content should have no conflict markers"
    );

    // 5. Write resolved text to worktree and stage
    fs::write(repo.join("doc.txt"), resolved_content).unwrap();
    opened.stage(&[Path::new("doc.txt")]).unwrap();

    // 6. Verify conflict is resolved — no more conflict status
    let status_after = opened.status().unwrap();
    assert!(
        !status_after
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("doc.txt") && e.kind == FileStatusKind::Conflicted),
        "doc.txt should no longer be conflicted after staging resolved content"
    );
}

#[test]
fn resolve_both_added_conflict_write_and_stage_clears_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_added_text_conflict(repo, "new.txt", "ours added\n", "theirs added\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let before = opened.status().unwrap();
    let conflict_entry = before
        .unstaged
        .iter()
        .find(|e| e.path == Path::new("new.txt"))
        .expect("expected both-added conflict path in unstaged status");
    assert_eq!(conflict_entry.kind, FileStatusKind::Conflicted);
    assert_eq!(conflict_entry.conflict, Some(FileConflictKind::BothAdded));

    let merged_before = fs::read_to_string(repo.join("new.txt")).unwrap();
    assert!(
        merged_before.contains("<<<<<<<"),
        "expected merge markers before resolution"
    );

    let session = opened
        .conflict_session(Path::new("new.txt"))
        .unwrap()
        .expect("conflict session for both-added path");
    assert_eq!(session.strategy, ConflictResolverStrategy::FullTextResolver);
    assert_eq!(session.conflict_kind, FileConflictKind::BothAdded);
    assert_eq!(session.total_regions(), 1);
    assert_eq!(session.unsolved_count(), 1);

    let resolved = "resolved both-added\n";
    write(repo, "new.txt", resolved);
    opened.stage(&[Path::new("new.txt")]).unwrap();

    let validation = worktree_core::services::validate_conflict_resolution_text(resolved);
    assert!(!validation.has_conflict_markers);
    assert_eq!(validation.marker_lines, 0);

    let after = opened.status().unwrap();
    assert!(
        after
            .unstaged
            .iter()
            .all(|e| e.path != Path::new("new.txt")),
        "expected conflict path to be removed from unstaged after save+stage; status={after:?}"
    );
    assert!(
        after.staged.iter().any(|e| {
            e.path == Path::new("new.txt")
                && matches!(e.kind, FileStatusKind::Modified | FileStatusKind::Added)
        }),
        "expected resolved both-added file to be staged as modified/added; status={after:?}"
    );
    assert_eq!(fs::read_to_string(repo.join("new.txt")).unwrap(), resolved);
}

/// End-to-end test: the stage-backed merge plan materializes trivial changes
/// as automatic context and exposes only genuine conflicts as regions.
#[test]
fn autosolve_safe_resolves_trivial_conflict_regions_end_to_end() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "seed.txt", "seed\n");
    run_git(repo, &["add", "seed.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    // Create a BothModified conflict using synthetic stages.
    // Write a worktree file with conflict markers containing three regions:
    //   Region 0: only ours changed (trivial → OnlyOursChanged)
    //   Region 1: both changed differently (genuine conflict)
    //   Region 2: both sides identical (trivial → IdenticalSides)
    let base_blob = hash_blob(repo, b"base-r0\nbase-r1\nbase-r2\n");
    let ours_blob = hash_blob(repo, b"ours-r0\nours-r1\nsame-r2\n");
    let theirs_blob = hash_blob(repo, b"base-r0\ntheirs-r1\nsame-r2\n");
    set_unmerged_stages(
        repo,
        "multi.txt",
        Some(&base_blob),
        Some(&ours_blob),
        Some(&theirs_blob),
    );

    // Write worktree file with three conflict marker blocks
    let merged_markers = concat!(
        "<<<<<<< HEAD\n",
        "ours-r0\n",
        "||||||| base\n",
        "base-r0\n",
        "=======\n",
        "base-r0\n",
        ">>>>>>> feature\n",
        "<<<<<<< HEAD\n",
        "ours-r1\n",
        "||||||| base\n",
        "base-r1\n",
        "=======\n",
        "theirs-r1\n",
        ">>>>>>> feature\n",
        "<<<<<<< HEAD\n",
        "same-r2\n",
        "||||||| base\n",
        "base-r2\n",
        "=======\n",
        "same-r2\n",
        ">>>>>>> feature\n",
    );
    write(repo, "multi.txt", merged_markers);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let mut session = opened
        .conflict_session(Path::new("multi.txt"))
        .unwrap()
        .expect("stage-backed conflict session");

    assert_eq!(session.strategy, ConflictResolverStrategy::FullTextResolver);
    assert!(session.merge_plan.is_some());
    assert_eq!(session.total_regions(), 1);
    assert_eq!(
        session.unsolved_count(),
        1,
        "only the genuine conflict should be exposed as a region",
    );
    assert_eq!(session.current_text(), Some(merged_markers));
    let projected = session.marker_projection_text().expect("marker projection");
    assert_eq!(projected.matches("<<<<<<<").count(), 1);
    assert!(projected.contains("ours-r0\n"));
    assert!(projected.contains("same-r2\n"));

    // The plan already resolved the trivial stage changes, so the legacy safe
    // pass has no additional marker region to process.
    let auto_resolved = session.auto_resolve_safe();
    assert_eq!(auto_resolved, 0);
    assert_eq!(session.unsolved_count(), 1);
    assert_eq!(session.next_unresolved_after(0), Some(0));
    assert_eq!(session.prev_unresolved_before(0), Some(0));
}

/// End-to-end test: conflict session for a modify/delete conflict
/// produces correct strategy and payloads, and the "keep" side can be
/// staged to resolve the conflict.
#[test]
fn conflict_session_modify_delete_keep_resolves_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base content\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    // Feature branch modifies the file
    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "modified by feature\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "modify"],
    );

    // Main branch deletes the file
    run_git(repo, &["checkout", "-"]);
    run_git(repo, &["rm", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "delete"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // Verify conflict session for modify/delete
    let session = opened
        .conflict_session(Path::new("a.txt"))
        .unwrap()
        .expect("conflict session for modify/delete");
    assert_eq!(
        session.strategy,
        ConflictResolverStrategy::TwoWayKeepDelete,
        "modify/delete conflicts should use TwoWayKeepDelete strategy"
    );
    assert_eq!(session.conflict_kind, FileConflictKind::DeletedByUs);

    // Ours deleted (absent), theirs has content
    assert!(
        session.ours.is_absent(),
        "ours (delete side) should be absent"
    );
    assert!(
        session.theirs.as_text().is_some(),
        "theirs (modify side) should have text"
    );
    assert_eq!(
        session.unsolved_count(),
        1,
        "two-way non-marker conflict sessions should expose one unresolved decision region"
    );
    assert_eq!(session.regions[0].ours, "");
    assert_eq!(session.regions[0].theirs, "modified by feature\n");

    // Resolve by keeping theirs (the modified version)
    opened
        .checkout_conflict_side(Path::new("a.txt"), ConflictSide::Theirs)
        .unwrap();

    // Verify file is restored and no longer conflicted
    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "modified by feature\n"
    );
    let status = opened.status().unwrap();
    assert!(
        !status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Conflicted),
        "a.txt should no longer be conflicted after keeping theirs"
    );
}

/// Validates the safety gate: `validate_conflict_resolution_text` correctly
/// detects remaining markers in partially-resolved text.
#[test]
fn validate_conflict_resolution_detects_partial_resolution() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    use worktree_core::services::validate_conflict_resolution_text;

    // Fully resolved text — no markers
    let clean = "line1\nline2\nline3\n";
    assert!(!validate_conflict_resolution_text(clean).has_conflict_markers);

    // Partially resolved — one conflict block remains
    let partial = concat!(
        "resolved section\n",
        "<<<<<<< HEAD\n",
        "ours\n",
        "=======\n",
        "theirs\n",
        ">>>>>>> feature\n",
        "another resolved section\n",
    );
    let v = validate_conflict_resolution_text(partial);
    assert!(v.has_conflict_markers);
    assert_eq!(v.marker_lines, 3); // <<<<<<<, =======, >>>>>>>

    // diff3-style markers
    let diff3 = concat!(
        "<<<<<<< HEAD\n",
        "ours\n",
        "||||||| base\n",
        "base\n",
        "=======\n",
        "theirs\n",
        ">>>>>>> feature\n",
    );
    let v3 = validate_conflict_resolution_text(diff3);
    assert!(v3.has_conflict_markers);
    assert_eq!(v3.marker_lines, 4); // <<<<<<<, |||||||, =======, >>>>>>>
}

/// End-to-end test: BothDeleted text conflict session uses DecisionOnly
/// strategy, and restoring from base via `checkout_conflict_side(Base)`
/// resolves the conflict.
#[test]
fn conflict_session_both_deleted_restore_from_base_resolves_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "seed.txt", "seed\n");
    run_git(repo, &["add", "seed.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    // BothDeleted: only base stage present, no ours or theirs
    let base_blob = hash_blob(repo, b"original content\n");
    set_unmerged_stages(repo, "removed.txt", Some(base_blob.as_str()), None, None);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // Verify conflict session
    let session = opened
        .conflict_session(Path::new("removed.txt"))
        .unwrap()
        .expect("conflict session for BothDeleted");
    assert_eq!(session.conflict_kind, FileConflictKind::BothDeleted);
    assert_eq!(session.strategy, ConflictResolverStrategy::DecisionOnly);
    assert!(
        matches!(session.base, ConflictPayload::Text(ref t) if t.as_ref() == "original content\n")
    );
    assert!(session.ours.is_absent());
    assert!(session.theirs.is_absent());
    assert!(matches!(
        session.current.as_ref(),
        Some(ConflictPayload::Absent)
    ));
    assert_eq!(session.unsolved_count(), 1);

    // Resolve by accepting deletion
    opened
        .accept_conflict_deletion(Path::new("removed.txt"))
        .unwrap();

    // Verify conflict is resolved
    let status = opened.status().unwrap();
    assert!(
        !status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("removed.txt") && e.kind == FileStatusKind::Conflicted),
        "removed.txt should no longer be conflicted after accepting deletion"
    );
    assert!(
        !repo.join("removed.txt").exists(),
        "file should be deleted after accepting deletion"
    );
}

/// End-to-end test: AddedByUs conflict session uses TwoWayKeepDelete
/// strategy, and keeping the file via `checkout_conflict_side(Ours)`
/// resolves the conflict.
#[test]
fn conflict_session_added_by_us_keep_resolves_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "seed.txt", "seed\n");
    run_git(repo, &["add", "seed.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    // AddedByUs: only ours stage present (no base, no theirs)
    let ours_blob = hash_blob(repo, b"added by us\n");
    set_unmerged_stages(repo, "new.txt", None, Some(ours_blob.as_str()), None);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // Verify status
    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|e| e.path == Path::new("new.txt"))
        .expect("expected AddedByUs conflict entry");
    assert_eq!(entry.kind, FileStatusKind::Conflicted);
    assert_eq!(entry.conflict, Some(FileConflictKind::AddedByUs));

    // Verify conflict session
    let session = opened
        .conflict_session(Path::new("new.txt"))
        .unwrap()
        .expect("conflict session for AddedByUs");
    assert_eq!(session.conflict_kind, FileConflictKind::AddedByUs);
    assert_eq!(session.strategy, ConflictResolverStrategy::TwoWayKeepDelete);
    assert!(session.base.is_absent());
    assert!(matches!(session.ours, ConflictPayload::Text(ref t) if t.as_ref() == "added by us\n"));
    assert!(session.theirs.is_absent());
    assert!(matches!(
        session.current.as_ref(),
        Some(ConflictPayload::Absent)
    ));
    assert_eq!(session.unsolved_count(), 1);

    // Resolve by keeping ours (the added file)
    opened
        .checkout_conflict_side(Path::new("new.txt"), ConflictSide::Ours)
        .unwrap();

    // Verify file exists and conflict is resolved
    assert_eq!(
        fs::read_to_string(repo.join("new.txt")).unwrap(),
        "added by us\n"
    );
    let status_after = opened.status().unwrap();
    assert!(
        !status_after
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("new.txt") && e.kind == FileStatusKind::Conflicted),
        "new.txt should no longer be conflicted after keeping ours"
    );
    assert!(
        status_after
            .staged
            .iter()
            .any(|e| e.path == Path::new("new.txt")),
        "new.txt should be staged after resolution"
    );
}

/// End-to-end test: AddedByThem conflict session uses TwoWayKeepDelete
/// strategy, and keeping the file via `checkout_conflict_side(Theirs)`
/// resolves the conflict.
#[test]
fn conflict_session_added_by_them_keep_resolves_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "seed.txt", "seed\n");
    run_git(repo, &["add", "seed.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    // AddedByThem: only theirs stage present (no base, no ours)
    let theirs_blob = hash_blob(repo, b"added by them\n");
    set_unmerged_stages(
        repo,
        "their_new.txt",
        None,
        None,
        Some(theirs_blob.as_str()),
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // Verify status
    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|e| e.path == Path::new("their_new.txt"))
        .expect("expected AddedByThem conflict entry");
    assert_eq!(entry.kind, FileStatusKind::Conflicted);
    assert_eq!(entry.conflict, Some(FileConflictKind::AddedByThem));

    // Verify conflict session
    let session = opened
        .conflict_session(Path::new("their_new.txt"))
        .unwrap()
        .expect("conflict session for AddedByThem");
    assert_eq!(session.conflict_kind, FileConflictKind::AddedByThem);
    assert_eq!(session.strategy, ConflictResolverStrategy::TwoWayKeepDelete);
    assert!(session.base.is_absent());
    assert!(session.ours.is_absent());
    assert!(
        matches!(session.theirs, ConflictPayload::Text(ref t) if t.as_ref() == "added by them\n")
    );
    assert!(matches!(
        session.current.as_ref(),
        Some(ConflictPayload::Absent)
    ));
    assert_eq!(session.unsolved_count(), 1);

    // Resolve by keeping theirs (the added file)
    opened
        .checkout_conflict_side(Path::new("their_new.txt"), ConflictSide::Theirs)
        .unwrap();

    // Verify file exists and conflict is resolved
    assert_eq!(
        fs::read_to_string(repo.join("their_new.txt")).unwrap(),
        "added by them\n"
    );
    let status_after = opened.status().unwrap();
    assert!(
        !status_after
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("their_new.txt") && e.kind == FileStatusKind::Conflicted),
        "their_new.txt should no longer be conflicted after keeping theirs"
    );
    assert!(
        status_after
            .staged
            .iter()
            .any(|e| e.path == Path::new("their_new.txt")),
        "their_new.txt should be staged after resolution"
    );
}

/// End-to-end test: DeletedByThem conflict session uses TwoWayKeepDelete
/// strategy (base+ours present, theirs absent), and keeping ours
/// via `checkout_conflict_side(Ours)` resolves the conflict.
#[test]
fn conflict_session_deleted_by_them_keep_ours_resolves_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base content\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    // Feature branch deletes the file
    run_git(repo, &["checkout", "-b", "feature"]);
    run_git(repo, &["rm", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "delete"],
    );

    // Main branch modifies the file
    run_git(repo, &["checkout", "-"]);
    write(repo, "a.txt", "modified by us\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "modify"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // Verify status shows DeletedByThem
    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Conflicted)
        .expect("expected DeletedByThem conflict entry");
    assert_eq!(entry.conflict, Some(FileConflictKind::DeletedByThem));

    // Verify conflict session
    let session = opened
        .conflict_session(Path::new("a.txt"))
        .unwrap()
        .expect("conflict session for DeletedByThem");
    assert_eq!(session.conflict_kind, FileConflictKind::DeletedByThem);
    assert_eq!(session.strategy, ConflictResolverStrategy::TwoWayKeepDelete);
    assert!(session.base.as_text().is_some());
    assert!(
        matches!(session.ours, ConflictPayload::Text(ref t) if t.as_ref() == "modified by us\n"),
        "ours (modified side) should have text"
    );
    assert!(
        session.theirs.is_absent(),
        "theirs (delete side) should be absent"
    );
    assert_eq!(session.unsolved_count(), 1);
    assert_eq!(session.regions[0].ours, "modified by us\n");
    assert_eq!(session.regions[0].theirs, "");

    // Resolve by keeping ours (the modified version)
    opened
        .checkout_conflict_side(Path::new("a.txt"), ConflictSide::Ours)
        .unwrap();

    // Verify file is kept and conflict is resolved
    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "modified by us\n"
    );
    let status_after = opened.status().unwrap();
    assert!(
        !status_after
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Conflicted),
        "a.txt should no longer be conflicted after keeping ours"
    );
}
