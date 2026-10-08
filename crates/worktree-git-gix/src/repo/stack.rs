use super::GixRepo;
use crate::util::{
    bytes_to_text_preserving_utf8, run_git_capture, run_git_raw_output, run_git_with_output,
    validate_ref_like_arg,
};
use std::collections::HashMap;
use worktree_core::domain::{StackRestackOutcome, StackRestackPlan};
use worktree_core::error::{Error, ErrorKind};
use worktree_core::services::{CommandOutput, Result};

impl GixRepo {
    /// Replays every branch in `plan` on top of its (possibly already-rebased)
    /// parent, base-first, using `git rebase --onto`.
    ///
    /// For each step, the commits unique to `branch` (those reachable from
    /// `branch` but not from its `parent`) are replayed onto the parent's
    /// *current* tip, while anchored against the parent's *original* tip
    /// captured before this restack began. Because the plan is ordered
    /// base-first, a parent has already been moved to its new position by the
    /// time its children are processed, so re-anchoring each child on the
    /// original parent tip produces a correct, fully-rewritten stack.
    ///
    /// Roots carry `parent == None`; the backend anchors them on the plan's
    /// `base_branch` instead. With neither a parent nor a base there is nothing
    /// to rebase onto, so the step is skipped.
    ///
    /// On a conflict (or any rebase failure) the in-progress rebase is aborted
    /// and every already-rebased branch is reset back to its captured original
    /// tip, returning the repository to its pre-restack state; the conflict
    /// branch is reported through the returned `Error`.
    pub(super) fn restack_stack(&self, plan: &StackRestackPlan) -> Result<StackRestackOutcome> {
        // Preserve the user's checked-out branch across the rebases below so a
        // restack does not silently leave them on the top branch.
        let head_target = self.capture_head_target();

        // Capture the original tip of every ref we might move or rebase onto,
        // so a conflict can roll everything back to the pre-restack state.
        let mut refs: Vec<String> = Vec::new();
        if let Some(base) = &plan.base_branch {
            refs.push(base.clone());
        }
        for step in &plan.steps {
            if !refs.contains(&step.branch) {
                refs.push(step.branch.clone());
            }
            if let Some(parent) = &step.parent
                && !refs.contains(parent)
            {
                refs.push(parent.clone());
            }
        }

        let mut original_tips: HashMap<String, String> = HashMap::new();
        for r in &refs {
            validate_ref_like_arg(r, "stack ref")?;
            let tip = self.ref_tip(r)?;
            original_tips.insert(r.clone(), tip);
        }

        let mut rebased: Vec<String> = Vec::new();
        for step in &plan.steps {
            validate_ref_like_arg(&step.branch, "stack branch")?;

            // Roots carry `parent == None`; anchor them on the plan's base
            // branch. With no anchor at all there is nothing to rebase onto.
            let parent_ref = step.parent.clone().or_else(|| plan.base_branch.clone());
            let Some(parent_ref) = parent_ref else {
                continue;
            };

            let new_parent_tip = self.ref_tip(&parent_ref)?;
            let old_parent_tip = original_tips.get(&parent_ref).cloned().ok_or_else(|| {
                Error::new(ErrorKind::Backend(format!(
                    "restack: parent ref '{parent_ref}' has no recorded tip"
                )))
            })?;

            let mut cmd = self.git_workdir_cmd();
            cmd.args([
                "rebase",
                "--onto",
                &new_parent_tip,
                &old_parent_tip,
                &step.branch,
            ]);
            let label = format!(
                "git rebase --onto {} {} {}",
                new_parent_tip, old_parent_tip, step.branch
            );
            let output = run_git_raw_output(cmd, &label)?;
            if !output.status.success() {
                // Conflict (or other failure): abort the in-progress rebase and
                // roll the already-rebased branches back to their original tips.
                let _ = self.rebase_abort_with_output();
                for b in &rebased {
                    if let Some(tip) = original_tips.get(b) {
                        let _ = self.reset_branch_ref(b, tip);
                    }
                }
                let stderr = bytes_to_text_preserving_utf8(&output.stderr)
                    .trim()
                    .to_string();
                self.restore_head(&head_target);
                return Err(Error::new(ErrorKind::Backend(format!(
                    "restack conflict while rebasing '{}': {}",
                    step.branch, stderr
                ))));
            }

            rebased.push(step.branch.clone());
        }

        self.restore_head(&head_target);
        Ok(StackRestackOutcome { rebased })
    }

    /// Resolves `r` to its current commit id via `git rev-parse --verify`.
    fn ref_tip(&self, r: &str) -> Result<String> {
        let mut cmd = self.git_workdir_cmd();
        cmd.args(["rev-parse", "--verify", r]);
        let out = run_git_capture(cmd, &format!("git rev-parse {r}"))?;
        Ok(out.trim().to_string())
    }

    /// Force-updates `branch` to point at `tip` without touching the working
    /// tree or HEAD. Used to roll an already-rebased branch back to its
    /// pre-restack tip when a later branch hits a conflict (HEAD is then on the
    /// conflicting branch, so a plain `git reset` would target the wrong ref).
    fn reset_branch_ref(&self, branch: &str, tip: &str) -> Result<CommandOutput> {
        validate_ref_like_arg(branch, "stack branch")?;
        validate_ref_like_arg(tip, "commit tip")?;
        let mut cmd = self.git_workdir_cmd();
        cmd.args(["update-ref", &format!("refs/heads/{branch}"), tip]);
        run_git_with_output(cmd, &format!("git update-ref refs/heads/{branch} {tip}"))
    }

    /// Returns the symbolic branch name when HEAD is on a branch, otherwise the
    /// current commit id (detached HEAD), so [`GixRepo::restack_stack`] can
    /// return the user to where they started after moving HEAD across branches.
    fn capture_head_target(&self) -> Option<String> {
        let mut sym = self.git_workdir_cmd();
        sym.args(["symbolic-ref", "--short", "-q", "HEAD"]);
        if let Ok(name) = run_git_capture(sym, "git symbolic-ref HEAD") {
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
        let mut head = self.git_workdir_cmd();
        head.args(["rev-parse", "HEAD"]);
        run_git_capture(head, "git rev-parse HEAD")
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Best-effort: returns HEAD to `target` (a branch name or commit id) after
    /// a restack. Failures are intentionally ignored — the branch refs are
    /// already correct; only the user's checkout position is affected.
    fn restore_head(&self, target: &Option<String>) {
        if let Some(t) = target {
            let mut cmd = self.git_workdir_cmd();
            cmd.args(["checkout", "-q", t]);
            let _ = run_git_with_output(cmd, &format!("git checkout {t}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open::open_worktree_repo;
    use std::path::Path;
    use std::process::Command as ProcCommand;
    use worktree_core::domain::StackRestackStep;

    fn init_repo() -> (tempfile::TempDir, GixRepo) {
        let dir = tempfile::tempdir().unwrap();
        let ok = ProcCommand::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git init must succeed");
        // A throwaway identity so commits in the temp repo succeed regardless of
        // the machine's global git config.
        for (k, v) in [
            ("user.email", "test@worktree.local"),
            ("user.name", "WorkTree Test"),
        ] {
            let ok = ProcCommand::new("git")
                .args(["config", k, v])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success();
            assert!(ok, "git config {k} must succeed");
        }
        let repo = open_worktree_repo(dir.path()).unwrap().into_sync();
        let gix_repo = GixRepo::new(dir.path().to_path_buf(), repo);
        (dir, gix_repo)
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = ProcCommand::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?} must succeed");
    }

    fn commit_file(dir: &Path, name: &str, content: &str, msg: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "-m", msg]);
    }

    fn tip(dir: &Path, r: &str) -> String {
        let out = ProcCommand::new("git")
            .args(["rev-parse", r])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn head_branch(dir: &Path) -> Option<String> {
        let out = ProcCommand::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        let s = String::from_utf8(out.stdout).unwrap();
        let s = s.trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    #[test]
    fn restack_rebases_children_onto_new_parent() {
        let (dir, repo) = init_repo();
        commit_file(dir.path(), "base.txt", "base\n", "base");

        git(dir.path(), &["checkout", "-b", "b1"]);
        commit_file(dir.path(), "b1.txt", "b1\n", "b1");

        git(dir.path(), &["checkout", "-b", "b2"]);
        commit_file(dir.path(), "b2.txt", "b2\n", "b2");

        // Move the base so the children need to be replayed onto a new main.
        git(dir.path(), &["checkout", "main"]);
        commit_file(dir.path(), "main2.txt", "main2\n", "main2");

        let plan = StackRestackPlan {
            base_branch: Some("main".to_string()),
            steps: vec![
                StackRestackStep {
                    branch: "b1".to_string(),
                    parent: None,
                },
                StackRestackStep {
                    branch: "b2".to_string(),
                    parent: Some("b1".to_string()),
                },
            ],
        };

        let outcome = repo.restack_stack(&plan).expect("restack succeeds");
        assert_eq!(
            outcome.rebased,
            vec!["b1".to_string(), "b2".to_string()],
            "both branches should be reported as rebased"
        );

        let main_tip = tip(dir.path(), "main");
        assert_eq!(tip(dir.path(), "b1^"), main_tip, "b1 must sit on new main");
        assert_eq!(
            tip(dir.path(), "b2^"),
            tip(dir.path(), "b1"),
            "b2 must sit on new b1"
        );

        // HEAD returns to where it started (inspect before moving HEAD away).
        assert_eq!(head_branch(dir.path()), Some("main".to_string()));

        // Content is preserved across the replay.
        git(dir.path(), &["checkout", "b2"]);
        assert!(dir.path().join("base.txt").exists());
        assert!(dir.path().join("b1.txt").exists());
        assert!(dir.path().join("b2.txt").exists());
        assert!(dir.path().join("main2.txt").exists());
    }

    #[test]
    fn restack_aborts_and_rolls_back_on_conflict() {
        let (dir, repo) = init_repo();
        commit_file(dir.path(), "a.txt", "a\nx\n", "base");

        git(dir.path(), &["checkout", "-b", "b1"]);
        // b1 only touches b.txt, so it rebases cleanly onto a moved main.
        commit_file(dir.path(), "b.txt", "b1\n", "b1");

        git(dir.path(), &["checkout", "-b", "b2"]);
        // b2 changes a.txt — this is what will conflict after main moves.
        std::fs::write(dir.path().join("a.txt"), "a\nx\nb2\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        git(dir.path(), &["commit", "-m", "b2"]);

        // Move main with a conflicting change to a.txt.
        git(dir.path(), &["checkout", "main"]);
        std::fs::write(dir.path().join("a.txt"), "a\nx CHANGED\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        git(dir.path(), &["commit", "-m", "main move"]);

        let b1_orig = tip(dir.path(), "b1");
        let b2_orig = tip(dir.path(), "b2");

        let plan = StackRestackPlan {
            base_branch: Some("main".to_string()),
            steps: vec![
                StackRestackStep {
                    branch: "b1".to_string(),
                    parent: None,
                },
                StackRestackStep {
                    branch: "b2".to_string(),
                    parent: Some("b1".to_string()),
                },
            ],
        };

        let result = repo.restack_stack(&plan);
        assert!(result.is_err(), "a conflict must surface as an error");

        // b1 rebased successfully before the conflict, so it must be rolled back
        // to its pre-restack tip; b2 is restored by the rebase --abort.
        assert_eq!(tip(dir.path(), "b1"), b1_orig, "b1 must roll back");
        assert_eq!(tip(dir.path(), "b2"), b2_orig, "b2 must roll back");
        assert!(
            !repo.rebase_in_progress().unwrap(),
            "no rebase should remain in progress"
        );
    }
}
