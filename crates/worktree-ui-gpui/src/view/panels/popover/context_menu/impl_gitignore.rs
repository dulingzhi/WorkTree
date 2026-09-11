//! `PopoverHost` add-to-gitignore: target, scope, patterns and submission.

use super::super::*;
use super::helpers::gitignore_pattern_line;

// @split-module: impl_gitignore
impl PopoverHost {
    /// What an "Add to .gitignore" action on `path` would cover, or `None` when
    /// the action does not apply.
    ///
    /// The single source of truth for eligibility: the menu uses it to decide
    /// whether to show the entry and the dialog uses it to seed itself. Two
    /// copies of this rule would let the menu offer an action the dialog then
    /// refuses (or the reverse), and nothing would catch the drift.
    ///
    /// The selection is read but *not* consumed — the dialog is cancellable, and
    /// losing a selection to a dialog the user backed out of is exactly the
    /// failure the read/take split documented above exists to prevent.
    pub(in crate::view::panels::popover) fn add_to_gitignore_target(
        &self,
        repo_id: RepoId,
        area: DiffArea,
        path: &std::path::PathBuf,
        cx: &gpui::App,
    ) -> Option<(
        Vec<std::path::PathBuf>,
        worktree_core::gitignore::GitignoreSuggestions,
    )> {
        use worktree_core::domain::FileStatusKind;

        // `.gitignore` has no effect on anything already in the index, so a
        // pattern for a tracked path is a line that changes nothing and leaves
        // the row exactly where it was.
        if area != DiffArea::Unstaged {
            return None;
        }
        let repo = self.state.repos.iter().find(|r| r.id == repo_id)?;
        let (paths, used_selection) = self.status_paths_for_action(repo_id, area, path, cx);

        // Every targeted path must be untracked, not just the clicked one: the
        // untracked and unstaged buckets are separate, and a tracked path that
        // snuck into the selection is a silent no-op the user would debug.
        // Indexed when there is a selection to check, because one
        // `status_entry_for_path` per path is a linear scan of the whole status
        // list each time and this runs on every right-click.
        let all_untracked = if used_selection {
            let untracked: FxHashSet<&std::path::Path> = repo
                .status_entries_for_area(area)
                .unwrap_or(&[])
                .iter()
                .filter(|entry| entry.kind == FileStatusKind::Untracked)
                .map(|entry| entry.path.as_path())
                .collect();
            paths.iter().all(|p| untracked.contains(p.as_path()))
        } else {
            paths.first().is_some_and(|p| {
                matches!(
                    repo.status_entry_for_path(area, p).map(|s| s.kind),
                    Some(FileStatusKind::Untracked)
                )
            })
        };
        if !all_untracked {
            return None;
        }

        // Rules out anything with no expressible pattern at all: a non-UTF-8
        // path, or a name containing a line break.
        let suggestions = worktree_core::gitignore::suggestions_for_paths(&paths)?;
        Some((paths, suggestions))
    }

    /// Seed the "Add to .gitignore" dialog when it opens.
    pub(in crate::view::panels::popover) fn prepare_add_to_gitignore(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        path: &std::path::PathBuf,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let target = self.add_to_gitignore_target(repo_id, area, path, cx);
        let scope = worktree_core::gitignore::GitignoreScope::File;
        let text = target
            .as_ref()
            .map(|(_, suggestions)| suggestions.lines_for(scope).join("\n"))
            .unwrap_or_default();
        let (paths, suggestions) = match target {
            Some((paths, suggestions)) => (paths, Some(suggestions)),
            None => (vec![path.clone()], None),
        };

        self.gitignore.gitignore_paths = paths;
        self.gitignore.gitignore_suggestions = suggestions;
        self.gitignore.gitignore_scope = scope;

        let theme = self.theme;
        self.gitignore
            .gitignore_patterns_input
            .update(cx, |input, cx| {
                input.clear_transient_key_presses();
                input.set_theme(theme, cx);
                input.set_text(&text, cx);
                cx.notify();
            });
        // `set_text` resets only the horizontal offset, so a dialog reopened
        // after scrolling a long selection would show blank space where the
        // patterns are.
        self.gitignore
            .gitignore_patterns_scroll
            .set_offset(gpui::point(px(0.0), px(0.0)));
        let focus = self
            .gitignore
            .gitignore_patterns_input
            .read_with(cx, |i, _| i.focus_handle());
        window.focus(&focus, cx);
    }

    /// Re-seed the pattern field after the user picks a different scope.
    ///
    /// This overwrites whatever is in the field. That is the point: the user
    /// just asked for a different pattern, and merging the old text into the
    /// new scope would leave a field matching neither.
    pub(in crate::view::panels::popover) fn set_add_to_gitignore_scope(
        &mut self,
        scope: worktree_core::gitignore::GitignoreScope,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(text) = self
            .gitignore
            .gitignore_suggestions
            .as_ref()
            .map(|s| s.lines_for(scope).join("\n"))
        else {
            return;
        };
        self.gitignore.gitignore_scope = scope;
        self.gitignore
            .gitignore_patterns_input
            .update(cx, |input, cx| {
                input.set_text(&text, cx);
                cx.notify();
            });
        // As in `prepare_add_to_gitignore`: the new text is usually shorter than
        // what it replaced, so a stale vertical offset would scroll it off.
        self.gitignore
            .gitignore_patterns_scroll
            .set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }

    /// The non-blank lines currently in the pattern field.
    pub(in crate::view::panels::popover) fn add_to_gitignore_patterns(
        &self,
        cx: &gpui::App,
    ) -> Vec<String> {
        self.gitignore
            .gitignore_patterns_input
            .read_with(cx, |input, _| {
                input
                    .text()
                    .lines()
                    .filter_map(gitignore_pattern_line)
                    .map(ToOwned::to_owned)
                    .collect()
            })
    }

    /// Whether the pattern field holds anything submittable.
    ///
    /// Separate from [`Self::add_to_gitignore_patterns`] because this runs every
    /// frame the dialog is on screen, and building the whole `Vec<String>` just
    /// to ask whether it is empty allocates one `String` per selected file per
    /// frame.
    pub(in crate::view::panels::popover) fn can_submit_add_to_gitignore(
        &self,
        cx: &gpui::App,
    ) -> bool {
        self.gitignore
            .gitignore_patterns_input
            .read_with(cx, |input, _| {
                input
                    .text()
                    .lines()
                    .any(|line| gitignore_pattern_line(line).is_some())
            })
    }

    pub(in crate::view::panels::popover) fn submit_add_to_gitignore(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        let patterns = self.add_to_gitignore_patterns(cx);
        if patterns.is_empty() {
            return;
        }
        // Now that the action is going ahead, the row selection has served its
        // purpose and is cleared. The returned paths are unused — the patterns
        // come from the field, which the user may have edited.
        let _ = self.take_status_paths_for_action(repo_id, area, &path, cx);
        self.store
            .dispatch(Msg::AppendGitignorePatterns { repo_id, patterns });
        self.close_popover(cx);
    }
}
