//! `PopoverHost` runtime state: active repo, context menu invoker, text inputs and
//! the toast hook.

use super::super::*;
use super::host::PopoverHost;
use super::kinds::PopoverKind;

// @split-module: impl_state
impl PopoverHost {
    pub(in crate::view::panels::popover) fn clear_active_context_menu_invoker(
        &self,
        cx: &mut gpui::Context<Self>,
    ) {
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.set_active_context_menu_invoker(None, cx);
            });
        });
    }

    pub(in crate::view::panels::popover) fn history_refs_menu_active(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.root_view
            .update(cx, |root, _cx| {
                root.active_context_menu_invoker
                    .as_ref()
                    .is_some_and(|invoker| {
                        invoker.as_ref().starts_with(
                            crate::view::history_refs_hover::HISTORY_REFS_HOVER_MENU_INVOKER_PREFIX,
                        )
                    })
            })
            .unwrap_or(false)
    }

    /// Subscription that submits a prompt when Enter is pressed in one of its
    /// inputs. Escape is consumed here; prompt dismissal is handled by the
    /// PopoverPrompt key context.
    pub(in crate::view::panels::popover) fn prompt_enter_subscription(
        input: &Entity<components::TextInput>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        is_active: fn(&Self) -> bool,
        submit: fn(&mut Self, &mut Window, &mut gpui::Context<Self>),
    ) -> gpui::Subscription {
        cx.observe_in(input, window, move |this, input, window, cx| {
            let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
            let _ = input.update(cx, |input, _| input.take_escape_pressed());

            if !is_active(this) {
                return;
            }

            if enter_pressed {
                submit(this, window, cx);
                return;
            }

            cx.notify();
        })
    }

    /// Every text input owned by the host, including the lazily created
    /// picker search inputs that currently exist.
    pub(in crate::view::panels::popover) fn all_text_inputs(
        &self,
    ) -> impl Iterator<Item = &Entity<components::TextInput>> {
        [
            &self.clone_repo.clone_repo_url_input,
            &self.clone_repo.clone_repo_parent_dir_input,
            &self.rebase_onto.rebase_onto_input,
            &self.create_tag.create_tag_input,
            &self.create_tag.create_tag_message_input,
            &self.gitignore.gitignore_patterns_input,
            &self.squash.squash_message_input,
            &self.squash.squash_description_input,
            &self.remote_prompts.remote_name_input,
            &self.remote_prompts.remote_url_input,
            &self.remote_prompts.remote_url_edit_input,
            &self.remote_prompts.remote_ssh_key_input,
            &self.create_branch.create_branch_input,
            &self.stash.stash_message_input,
            &self.commit_prompt.commit_prompt_message_input,
            &self.push_upstream.push_upstream_branch_input,
            &self.worktree_add.worktree_path_input,
            &self.worktree_add.worktree_ref_input,
            &self.submodule_add.submodule_url_input,
            &self.submodule_add.submodule_path_input,
            &self.submodule_add.submodule_ref_input,
            &self.submodule_add.submodule_branch_input,
            &self.submodule_add.submodule_name_input,
            &self.rebase_reword.rebase_reword_input,
            &self.rebase_reword.rebase_reword_description_input,
        ]
        .into_iter()
        .chain(
            [
                &self.repo_picker.repo_picker_search_input,
                &self.branch_picker.branch_picker_search_input,
                &self.remote_picker.remote_picker_search_input,
                &self.file_history.file_history_search_input,
                &self
                    .history_author_filter
                    .history_author_filter_search_input,
                &self.history_ref_filter.history_ref_filter_search_input,
                &self.worktree_picker.worktree_picker_search_input,
                &self.workspace_picker.workspace_picker_search_input,
                &self.upstream_picker.upstream_picker_search_input,
                &self.submodule_picker.submodule_picker_search_input,
                &self.tag_picker.tag_picker_search_input,
                &self.commit_search_picker.commit_search_picker_search_input,
                &self.stash_picker.stash_picker_search_input,
            ]
            .into_iter()
            .flatten(),
        )
    }

    pub(in crate::view) fn is_kind_open(&self, kind: &PopoverKind) -> bool {
        self.popover.as_ref() == Some(kind)
    }

    /// Whether the unsaved-edits confirmation is the popover on screen.
    ///
    /// Asked by the close/quit path instead of a mirrored bool: that dialog
    /// blocks every further close while it is up, and a mirror that missed a
    /// dismissal wedged the window shut for the rest of the session.
    pub(in crate::view) fn showing_unsaved_file_edits_prompt(&self) -> bool {
        matches!(self.popover, Some(PopoverKind::UnsavedFileEditsConfirm(_)))
    }

    pub(in crate::view::panels::popover) fn active_repo_id(&self) -> Option<RepoId> {
        self.state.active_repo
    }

    pub(in crate::view::panels::popover) fn active_repo(&self) -> Option<&RepoState> {
        let repo_id = self.active_repo_id()?;
        self.state.repos.iter().find(|r| r.id == repo_id)
    }

    /// The active branch filter, or `None` when it matches everything.
    ///
    /// Mirrors `matches_branch_filter`, which treats a blank query as "no
    /// filter" — so a lone space must not read as a filter that hides
    /// everything.
    pub(in crate::view) fn active_branch_filter(&self) -> Option<&str> {
        let query = self.branch_filter_query.trim();
        (!query.is_empty()).then_some(query)
    }

    /// Whether a sidebar collapse key is currently collapsed, going through
    /// `branch_sidebar::is_collapsed` so default-collapsed sections and the
    /// inverted `expanded:` storage are read the same way the tree reads them.
    pub(in crate::view) fn sidebar_collapse_key_is_collapsed(
        &self,
        repo_id: RepoId,
        collapse_key: &str,
    ) -> bool {
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return false;
        };
        // A repo with nothing stored reads the same as one with an empty set:
        // `is_collapsed` answers from the key's own default in both cases.
        static EMPTY: std::sync::LazyLock<std::collections::BTreeSet<String>> =
            std::sync::LazyLock::new(std::collections::BTreeSet::new);
        let items = self
            .collapsed_items_by_repo
            .get(&repo.spec.workdir)
            .unwrap_or(&EMPTY);
        crate::view::branch_sidebar::is_collapsed(items, collapse_key)
    }

    /// How many pinned branches the section is actually showing, for the pinned
    /// header's "Unpin all (N)".
    ///
    /// Counting raw pin keys would overcount: the row builder skips a pin whose
    /// branch no longer exists, and skips one filtered out by the branch
    /// filter, so "Unpin all (3)" could sit above a single row.
    pub(in crate::view) fn pinned_branch_count(
        &self,
        repo_id: RepoId,
        section: BranchSection,
    ) -> usize {
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return 0;
        };
        let filter = self.active_branch_filter().unwrap_or_default();
        self.pinned_branches_by_repo
            .get(&repo.spec.workdir)
            .map_or(0, |items| {
                items
                    .iter()
                    .filter(|key| {
                        crate::view::branch_sidebar::pinned_branch_renders(
                            repo, key, section, filter,
                        )
                    })
                    .count()
            })
    }

    pub(in crate::view) fn is_branch_pinned(
        &self,
        repo_id: RepoId,
        section: BranchSection,
        name: &str,
    ) -> bool {
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return false;
        };
        let key = crate::view::branch_sidebar::branch_pin_storage_key(section, name);
        self.pinned_branches_by_repo
            .get(&repo.spec.workdir)
            .is_some_and(|items| items.contains(&key))
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub(in crate::view::panels::popover) fn install_linux_desktop_integration(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.install_linux_desktop_integration(cx);
        });
    }

    pub(in crate::view::panels::popover) fn push_toast(
        &mut self,
        kind: components::ToastKind,
        message: String,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.push_toast(kind, message, cx);
        });
    }
}
