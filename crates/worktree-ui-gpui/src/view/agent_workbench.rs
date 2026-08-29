//! The agent workbench: claude code / codex sessions in the embedded
//! terminal, with the session's worktree baseline captured up front so
//! "what did the agent change" is a plain diff against that point.

use super::*;

/// The CLI agents a session can run. Both live in the embedded terminal
/// with the repo's workdir, so per-repo-tab isolation comes for free.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum AgentKind {
    ClaudeCode,
    Codex,
}

impl AgentKind {
    // Only tests enumerate the roster today; the start flows name kinds.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::view) fn all() -> [AgentKind; 2] {
        [AgentKind::ClaudeCode, AgentKind::Codex]
    }

    /// The executable name searched for on PATH.
    pub(in crate::view) fn executable(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude",
            AgentKind::Codex => "codex",
        }
    }

    /// The session's display label, also the seeded terminal tab title.
    pub(in crate::view) fn display_label(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
        }
    }
}

/// Find `name` among `paths` (a PATH split), requiring the executable bit
/// on unix. Pure over its inputs so the search itself is unit-testable.
pub(in crate::view) fn find_executable_in_paths(
    name: &str,
    paths: &[std::path::PathBuf],
) -> Option<std::path::PathBuf> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    paths.iter().find_map(|dir| {
        let candidate = dir.join(name);
        let metadata = std::fs::metadata(&candidate).ok()?;
        if !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        Some(candidate)
    })
}

/// Which agents are launchable right now, in stable order.
// The roster check the tests pin; the start flows look up one name.
#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::view) fn available_agents(paths: &[std::path::PathBuf]) -> Vec<AgentKind> {
    AgentKind::all()
        .into_iter()
        .filter(|kind| find_executable_in_paths(kind.executable(), paths).is_some())
        .collect()
}

/// The real PATH split, for the callers that start sessions.
pub(in crate::view) fn system_search_paths() -> Vec<std::path::PathBuf> {
    std::env::var("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

/// The commit the agent's changes are measured against: the pre-session
/// dirty state when there is one (`git stash create` makes a commit object
/// without touching anything), else HEAD. `None` when neither resolves —
/// the caller should refuse to start rather than measure against nothing.
pub(in crate::view) fn resolve_agent_baseline(
    stash_create_output: &str,
    head_output: &str,
) -> Option<CommitId> {
    let stash = stash_create_output.trim();
    if !stash.is_empty() {
        return Some(CommitId(stash.to_string().into()));
    }
    let head = head_output.trim();
    (!head.is_empty()).then(|| CommitId(head.to_string().into()))
}

/// The sibling folder (and, by git's `worktree add <path>` convention, the
/// branch name) for one agent worktree. Worktrees live beside the main
/// checkout, matching the workspace picker's suggestion, so the main
/// working tree's status never sees the folder.
pub(in crate::view) fn agent_worktree_layout(
    workdir: &std::path::Path,
    unix_seconds: u64,
) -> (std::path::PathBuf, String) {
    let name = format!("agent-{unix_seconds}");
    match workdir.parent() {
        Some(parent) => (parent.join(&name), name),
        None => (std::path::PathBuf::from(&name), name),
    }
}

/// One recorded agent session. The terminal instance itself lives in the
/// main repo tab's terminal session (closing it ends the session); the agent
/// runs in `worktree_path`, whose state is measured against `baseline` (its
/// creation commit). The worktree outlives the session on purpose — it is
/// merged or cleaned up through the regular worktree management UI.
pub(in crate::view) struct AgentSessionState {
    pub kind: AgentKind,
    pub baseline: CommitId,
    pub worktree_path: std::path::PathBuf,
    /// The repo tab of the agent worktree, once opened by "view changes".
    /// The diff view resolves the session through it (the worktree tab's
    /// repo id is not the repo id the session is keyed by).
    pub worktree_repo_id: Option<RepoId>,
}

/// The inputs the diff view needs to offer path-level reject while an agent
/// compare is on screen: a working-tree-tracking range whose base is the
/// session's baseline, with a concrete file selected. `None` for any other
/// diff target — a user's own compare must not grow the button.
pub(in crate::view) fn agent_restore_context(
    session: &AgentSessionState,
    diff_target: Option<&DiffTarget>,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let DiffTarget::CommitRange {
        from_commit_id,
        to_commit_id: None,
        path: Some(path),
        ..
    } = diff_target?
    else {
        return None;
    };
    if *from_commit_id != session.baseline {
        return None;
    }
    Some((session.worktree_path.clone(), path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kinds_map_to_their_executables() {
        assert_eq!(AgentKind::ClaudeCode.executable(), "claude");
        assert_eq!(AgentKind::Codex.executable(), "codex");
        assert_eq!(AgentKind::ClaudeCode.display_label(), "Claude Code");
        assert_eq!(AgentKind::all().len(), 2);
    }

    #[test]
    fn find_executable_requires_a_file_with_the_executable_bit() {
        let dir = std::env::temp_dir().join(format!(
            "worktree_agent_test_{}_{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let runnable = dir.join("claude");
        std::fs::write(&runnable, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runnable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let plain = dir.join("codex");
        std::fs::write(&plain, b"not executable\n").unwrap();
        let nested = dir.join("claude-nested");
        std::fs::create_dir(&nested).unwrap();

        let paths = vec![dir.clone()];
        assert_eq!(
            find_executable_in_paths("claude", &paths),
            Some(runnable.clone()),
            "the executable-bit file is found"
        );
        #[cfg(unix)]
        assert_eq!(
            find_executable_in_paths("codex", &paths),
            None,
            "a plain file is not runnable"
        );
        assert_eq!(
            find_executable_in_paths("claude-nested", &paths),
            None,
            "directories never match"
        );
        assert_eq!(
            find_executable_in_paths("../escape", &paths),
            None,
            "path-shaped names are refused, only bare names are searched"
        );

        let available = available_agents(&paths);
        assert_eq!(available, vec![AgentKind::ClaudeCode]);

        std::fs::remove_dir_all(&dir).ok();
    }

    fn session_with_worktree() -> AgentSessionState {
        AgentSessionState {
            kind: AgentKind::ClaudeCode,
            baseline: CommitId("aaaa111122223333444455556666777788889999".into()),
            worktree_path: std::path::PathBuf::from("/repos/agent-1"),
            worktree_repo_id: Some(RepoId(7)),
        }
    }

    #[test]
    fn agent_restore_context_matches_only_the_sessions_own_compare() {
        let session = session_with_worktree();
        let range = |to: Option<&str>, path: Option<&str>| DiffTarget::CommitRange {
            from_commit_id: session.baseline.clone(),
            to_commit_id: to.map(|sha| CommitId(sha.into())),
            path: path.map(std::path::PathBuf::from),
        };

        let target = range(None, Some("src/lib.rs"));
        assert_eq!(
            agent_restore_context(&session, Some(&target)),
            Some((
                std::path::PathBuf::from("/repos/agent-1"),
                std::path::PathBuf::from("src/lib.rs")
            )),
            "the session's own working-tree compare with a file selected offers restore"
        );

        let whole_range = range(None, None);
        assert_eq!(
            agent_restore_context(&session, Some(&whole_range)),
            None,
            "no single file on screen — nothing to restore"
        );

        let committed = range(
            Some("bbbb111122223333444455556666777788889999"),
            Some("src/lib.rs"),
        );
        assert_eq!(agent_restore_context(&session, Some(&committed)), None);

        let mut other_base = range(None, Some("src/lib.rs"));
        if let DiffTarget::CommitRange { from_commit_id, .. } = &mut other_base {
            *from_commit_id = CommitId("cccc111122223333444455556666777788889999".into());
        }
        assert_eq!(
            agent_restore_context(&session, Some(&other_base)),
            None,
            "a user's own compare against another base keeps the toolbar clean"
        );

        assert_eq!(agent_restore_context(&session, None), None);
    }

    #[test]
    fn agent_worktree_layout_places_the_folder_beside_the_checkout() {
        let (path, branch) =
            agent_worktree_layout(std::path::Path::new("/repos/widgets"), 1770000000);
        assert_eq!(path, std::path::PathBuf::from("/repos/agent-1770000000"));
        assert_eq!(branch, "agent-1770000000");

        // No parent (a root-level workdir): the bare name still works.
        let (path, branch) = agent_worktree_layout(std::path::Path::new("/"), 5);
        assert_eq!(path, std::path::PathBuf::from("agent-5"));
        assert_eq!(branch, "agent-5");
    }

    #[test]
    fn resolve_agent_baseline_prefers_the_dirty_snapshot_over_head() {
        let baseline = resolve_agent_baseline(
            "aaaa111122223333444455556666777788889999\n",
            "bbbb111122223333444455556666777788889999\n",
        );
        assert_eq!(
            baseline,
            Some(CommitId("aaaa111122223333444455556666777788889999".into())),
            "a dirty tree measures against its stash-create snapshot"
        );

        let baseline = resolve_agent_baseline("", "bbbb111122223333444455556666777788889999\n");
        assert_eq!(
            baseline,
            Some(CommitId("bbbb111122223333444455556666777788889999".into())),
            "a clean tree measures against HEAD"
        );

        assert_eq!(
            resolve_agent_baseline("  \n", "\n"),
            None,
            "no resolvable point means no session"
        );
    }
}
