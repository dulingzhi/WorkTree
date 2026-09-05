use super::*;

pub(super) fn clone_repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last).trim();
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.to_string()
    }
}

impl PopoverHost {
    /// Validates the repo's current multi-selection against its loaded log and
    /// HEAD, returning a squash plan when the selection is eligible. Shared by
    /// the squash prompt's render, prefill, and submit paths so they always
    /// agree on the range.
    pub(in crate::view) fn squash_plan_for_repo_id(
        &self,
        repo_id: RepoId,
    ) -> Option<worktree_core::squash::SquashPlan> {
        let repo = self.state.repos.iter().find(|r| r.id == repo_id)?;
        let Loadable::Ready(page) = &repo.log else {
            return None;
        };
        let head = repo.head_commit_id()?;
        worktree_core::squash::squash_eligibility(
            &page.commits,
            &repo.history_state.multi_selection.commits,
            &head,
        )
    }

    /// Populates the squash prompt's inputs from the loaded message preview.
    /// Only fires when the preview matches the live plan's range (never a stale
    /// preview from an earlier selection) and only while both inputs are still
    /// empty for a range not yet prefilled (never over the user's own text).
    pub(super) fn sync_squash_prompt_prefill(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::SquashPrompt { repo_id }) = self.popover else {
            return;
        };
        let Some(plan) = self.squash_plan_for_repo_id(repo_id) else {
            return;
        };
        let repo = self.state.repos.iter().find(|r| r.id == repo_id);
        let Some(Loadable::Ready(preview)) = repo.map(|repo| &repo.history_state.squash_preview)
        else {
            return;
        };
        // The preview must belong to the range currently planned, not a leftover
        // from a previous prompt whose PrepareSquash dispatch has not landed yet.
        if preview.oldest != plan.oldest || preview.head != plan.head {
            return;
        }
        let range = (plan.oldest.clone(), plan.head.clone());
        if self.squash_prompt_prefilled_range.as_ref() == Some(&range) {
            return;
        }
        // Empty inputs mean the user has not typed anything for this range yet;
        // if they had, we must not overwrite it.
        let inputs_empty = self
            .squash_message_input
            .read_with(cx, |input, _| input.text().is_empty())
            && self
                .squash_description_input
                .read_with(cx, |input, _| input.text().is_empty());
        if !inputs_empty {
            return;
        }

        let subject = preview.subject.clone();
        let body = preview.body.clone();
        self.squash_prompt_prefilled_range = Some(range);
        self.squash_message_input.update(cx, |input, cx| {
            input.set_text(subject, cx);
            cx.notify();
        });
        self.squash_description_input.update(cx, |input, cx| {
            input.set_text(body, cx);
            cx.notify();
        });
    }

    /// Reads the squash prompt inputs, builds the final message, and dispatches
    /// the squash against the live plan. No-ops if the selection is no longer
    /// eligible or the subject is empty.
    pub(super) fn submit_squash(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::SquashPrompt { repo_id }) = self.popover else {
            return;
        };
        let Some(plan) = self.squash_plan_for_repo_id(repo_id) else {
            return;
        };
        let subject = self
            .squash_message_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if subject.is_empty() {
            return;
        }
        let body = self
            .squash_description_input
            .read_with(cx, |input, _| input.text().to_string());
        let message = if body.trim().is_empty() {
            subject
        } else {
            format!("{subject}\n\n{}", body.trim_end())
        };
        self.store.dispatch(Msg::SquashCommits {
            repo_id,
            oldest: plan.oldest,
            expected_head: plan.head,
            message,
            count: plan.commit_count,
        });
        self.close_popover(cx);
    }

    pub(super) fn can_submit_create_tag(&self, cx: &mut gpui::Context<Self>) -> bool {
        matches!(self.popover, Some(PopoverKind::CreateTagPrompt { .. }))
            && self
                .create_tag_input
                .read_with(cx, |input, _| is_submittable_branch_name(input.text()))
    }

    pub(super) fn can_submit_clone_repo(&self, cx: &mut gpui::Context<Self>) -> bool {
        matches!(self.popover, Some(PopoverKind::CloneRepo))
            && self
                .clone_repo_url_input
                .read_with(cx, |input, _| !input.text().trim().is_empty())
            && self
                .clone_repo_parent_dir_input
                .read_with(cx, |input, _| !input.text().trim().is_empty())
    }

    pub(super) fn can_submit_submodule_change_pointer(&self, cx: &mut gpui::Context<Self>) -> bool {
        matches!(
            self.popover,
            Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::ChangePointerPrompt { .. }),
                ..
            })
        ) && self
            .submodule_ref_input
            .read_with(cx, |input, _| !input.text().trim().is_empty())
    }

    pub(super) fn submit_create_tag(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::CreateTagPrompt { repo_id, target }) = self.popover.clone() else {
            return;
        };

        let name = self
            .create_tag_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if !is_submittable_branch_name(&name) {
            return;
        }

        let annotated = self.create_tag_annotated;
        let message = if annotated {
            let msg = self
                .create_tag_message_input
                .read_with(cx, |input, _| input.text().trim().to_string());
            Some(msg)
        } else {
            None
        };

        self.store.dispatch(Msg::CreateTag {
            repo_id,
            name,
            target,
            message,
            annotated,
        });
        self.close_popover(cx);
    }

    pub(super) fn submit_clone_repo(&mut self, cx: &mut gpui::Context<Self>) {
        if !matches!(self.popover, Some(PopoverKind::CloneRepo)) {
            return;
        }

        let url = self
            .clone_repo_url_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        let parent = self
            .clone_repo_parent_dir_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if url.is_empty() || parent.is_empty() {
            return;
        }

        let repo_name = clone_repo_name_from_url(&url);
        let dest = std::path::PathBuf::from(parent).join(repo_name);
        let ssh_key = self
            .clone_ssh_key_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        let ssh_key = (!ssh_key.is_empty()).then_some(ssh_key);
        self.store.dispatch(Msg::CloneRepo { url, dest, ssh_key });
        self.close_popover(cx);
    }

    pub(super) fn submit_submodule_change_pointer(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::ChangePointerPrompt { path }),
        }) = self.popover.clone()
        else {
            return;
        };

        let reference = self
            .submodule_ref_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if reference.is_empty() {
            return;
        }

        self.store.dispatch(Msg::ChangeSubmodulePointer {
            repo_id,
            path,
            reference,
        });
        self.dismiss_inline_popover(window, cx);
    }

    pub(super) fn can_submit_create_branch(&self, cx: &mut gpui::Context<Self>) -> bool {
        self.create_branch_prompt_repo_and_target().is_some()
            && self
                .create_branch_input
                .read_with(cx, |input, _| is_submittable_branch_name(input.text()))
    }

    pub(super) fn create_branch_prompt_repo_and_target(&self) -> Option<(RepoId, String)> {
        match &self.popover {
            Some(PopoverKind::CreateBranchFromRefPrompt {
                repo_id,
                source_selectable: true,
                ..
            }) => {
                let target = self.create_branch_source_target.clone();
                if target.is_empty() {
                    None
                } else {
                    Some((*repo_id, target))
                }
            }
            Some(PopoverKind::CreateBranchFromRefPrompt {
                repo_id, target, ..
            }) => Some((*repo_id, target.clone())),
            _ => None,
        }
    }

    pub(super) fn submit_create_branch(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some((repo_id, target)) = self.create_branch_prompt_repo_and_target() else {
            return;
        };
        let name = self
            .create_branch_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if !is_submittable_branch_name(&name) {
            return;
        }

        let checkout = match self.popover {
            Some(PopoverKind::CreateBranchFromRefPrompt { .. }) => {
                self.create_branch_checkout_enabled
            }
            _ => return,
        };

        if checkout {
            self.store.dispatch(Msg::CreateBranchAndCheckout {
                repo_id,
                name,
                target,
            });
        } else {
            self.store.dispatch(Msg::CreateBranch {
                repo_id,
                name,
                target,
            });
        }
        self.dismiss_inline_popover(window, cx);
    }

    pub(super) fn can_submit_rename_branch(&self, cx: &mut gpui::Context<Self>) -> bool {
        let Some(PopoverKind::RenameBranchPrompt { name, .. }) = &self.popover else {
            return false;
        };
        self.create_branch_input.read_with(cx, |input, _| {
            let new_name = input.text().trim();
            !new_name.is_empty() && new_name != name
        })
    }

    pub(super) fn submit_rename_branch(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(PopoverKind::RenameBranchPrompt { repo_id, name, .. }) = self.popover.clone()
        else {
            return;
        };
        let new_name = self
            .create_branch_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if new_name.is_empty() || new_name == name {
            return;
        }
        self.store.dispatch(Msg::RenameBranch {
            repo_id,
            old_name: name,
            new_name,
        });
        self.dismiss_inline_popover(window, cx);
    }

    pub(super) fn can_submit_stash(&self, cx: &mut gpui::Context<Self>) -> bool {
        self.active_repo_id().is_some()
            && self
                .stash_message_input
                .read_with(cx, |input, _| !input.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_commit_prompt(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.can_submit_commit_prompt(cx) {
            return;
        }
        let Some(PopoverKind::CommitPrompt { repo_id }) = self.popover.clone() else {
            return;
        };
        let message = self
            .commit_prompt_message_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if message.is_empty() {
            return;
        }
        self.store.dispatch(Msg::Commit {
            repo_id,
            message,
            push_after_commit: false,
        });
        self.commit_prompt_message_drafts.remove(&repo_id);
        self.commit_prompt_message_input
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.commit_prompt_message_scroll
            .set_offset(point(px(0.0), px(0.0)));
        self.dismiss_inline_popover(window, cx);
    }

    pub(super) fn save_commit_prompt_draft(&mut self, cx: &gpui::Context<Self>) {
        let Some(PopoverKind::CommitPrompt { repo_id }) = self.popover else {
            return;
        };
        let draft: SharedString = self
            .commit_prompt_message_input
            .read(cx)
            .text()
            .to_string()
            .into();
        if draft.is_empty() {
            self.commit_prompt_message_drafts.remove(&repo_id);
        } else {
            self.commit_prompt_message_drafts.insert(repo_id, draft);
        }
    }

    pub(in crate::view::panels) fn can_submit_commit_prompt(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.active_repo().is_some_and(|repo| {
            repo.staged_status_entries()
                .is_some_and(|entries| !entries.is_empty())
                || matches!(repo.merge_commit_message, Loadable::Ready(Some(_)))
        }) && self
            .commit_prompt_message_input
            .read_with(cx, |input, _| !input.text().trim().is_empty())
    }

    pub(super) fn submit_stash(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::StashPrompt { paths }) = self.popover.clone() else {
            return;
        };
        let Some(repo_id) = self.active_repo_id() else {
            return;
        };
        let message = self
            .stash_message_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if message.is_empty() {
            return;
        }

        let include_untracked = self.stash_include_untracked;
        let keep_index = self.stash_keep_index;
        let stashing_selection = !paths.is_empty();
        self.store.dispatch(Msg::Stash {
            repo_id,
            message,
            include_untracked,
            keep_index,
            paths: paths.into(),
        });
        if stashing_selection {
            // A selection-seeded stash has gone ahead; the rows it described
            // are leaving the status list, so the selection has served its
            // purpose.
            self.clear_status_multi_selection(repo_id, cx);
        }
        self.dismiss_inline_popover(window, cx);
    }

    pub(super) fn can_submit_stash_branch(&self, cx: &mut gpui::Context<Self>) -> bool {
        matches!(self.popover, Some(PopoverKind::StashBranchPrompt { .. }))
            && self
                .create_branch_input
                .read_with(cx, |input, _| !input.text().trim().is_empty())
    }

    /// The merge-request push is always submittable while its prompt is up:
    /// every option is optional and the target branch defaults server-side.
    pub(super) fn can_submit_mr_push(&self) -> bool {
        matches!(
            self.popover,
            Some(PopoverKind::MergeRequestPushPrompt { .. })
        )
    }

    pub(super) fn submit_mr_push(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::MergeRequestPushPrompt { repo_id }) = self.popover.clone() else {
            return;
        };
        let target_branch = self
            .mr_push_target_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        let options = worktree_core::services::MergeRequestPushOptions {
            create: true,
            target_branch: (!target_branch.is_empty()).then_some(target_branch),
            merge_when_pipeline_succeeds: self.mr_push_merge_when_pipeline_succeeds,
            remove_source_branch: self.mr_push_remove_source_branch,
            push_to_mr_branch: self.mr_push_push_to_mr_branch,
        };
        self.store
            .dispatch(Msg::PushMergeRequest { repo_id, options });
        self.dismiss_inline_popover(window, cx);
    }

    /// Generate the MR description with the configured AI source. Guarded
    /// feedback — provider not configured — surfaces as a warning toast, the
    /// same contract as the commit ✨.
    pub(in crate::view::panels) fn start_mr_description_generation(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(PopoverKind::MergeRequestPushPrompt { repo_id }) = self.popover.clone() else {
            return;
        };
        if self.mr_push_description_generating {
            return;
        }
        if !crate::ai_commit::current().is_configured() {
            self.push_toast(
                components::ToastKind::Warning,
                crate::i18n::tr_str("misc.ai_commit.not_configured").to_string(),
                cx,
            );
            return;
        }
        let Some(repo) = self.state.repos.iter().find(|repo| repo.id == repo_id) else {
            return;
        };
        let workdir = repo.spec.workdir.clone();
        let target_input = self
            .mr_push_target_input
            .read_with(cx, |input, _| input.text().to_string());
        self.mr_push_description_generating = true;
        self.mr_push_description_error = None;
        cx.notify();

        // The request itself needs git and the network; test builds exercise
        // the state machine through `finish_mr_description_generation`.
        #[cfg(test)]
        {
            let _ = (&workdir, &target_input);
        }
        #[cfg(not(test))]
        {
            let settings = crate::ai_commit::current();
            let locale = rust_i18n::locale();
            cx.spawn(async move |this, cx| {
                // Collecting context runs git — keep it off the executor.
                let context = smol::unblock(move || {
                    let origin_head = merge_request_push::git_output(
                        &workdir,
                        &["symbolic-ref", "refs/remotes/origin/HEAD"],
                    )
                    .ok();
                    let Some(target) = merge_request_push::resolve_mr_description_target(
                        &target_input,
                        origin_head.as_deref(),
                    ) else {
                        return Err(
                            crate::i18n::tr_str("input.mr_push.target_required").to_string()
                        );
                    };
                    merge_request_push::collect_mr_description_context(&workdir, &target)
                        .map(|context| (target, context))
                })
                .await;
                let result = match context {
                    Err(message) => Err(message),
                    Ok((target, (commits, diff_stat))) => {
                        crate::ai_commit::generate_mr_description(
                            &settings, &target, &commits, &diff_stat, &locale,
                        )
                        .await
                    }
                };
                let _ = this.update(cx, |host, cx| {
                    host.finish_mr_description_generation(result, cx)
                });
            })
            .detach();
        }
    }

    /// Landing seam for the generated description: success fills the
    /// editable field, failure shows inline next to it. Test builds call
    /// this directly.
    pub(in crate::view::panels) fn finish_mr_description_generation(
        &mut self,
        result: Result<String, String>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.mr_push_description_generating = false;
        match result {
            Ok(description) => {
                self.mr_push_description_error = None;
                self.mr_push_description_input.update(cx, |input, cx| {
                    input.set_text(description, cx);
                    cx.notify();
                });
            }
            Err(message) => self.mr_push_description_error = Some(message.into()),
        }
        cx.notify();
    }

    pub(super) fn submit_stash_branch(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(PopoverKind::StashBranchPrompt { repo_id, index }) = self.popover.clone() else {
            return;
        };
        let branch = self
            .create_branch_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if branch.is_empty() {
            return;
        }

        self.store.dispatch(Msg::StashBranch {
            repo_id,
            index,
            branch,
        });
        self.dismiss_inline_popover(window, cx);
    }

    pub(in crate::view::panels) fn can_submit_remote_add(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.remote_name_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
            && self
                .remote_url_input
                .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_remote_add(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Remote(RemotePopoverKind::AddPrompt),
        }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_remote_add(cx) {
            return;
        }
        let name = self
            .remote_name_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        let url = self
            .remote_url_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        self.store.dispatch(Msg::AddRemote { repo_id, name, url });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn can_submit_remote_edit_url(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.remote_url_edit_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_remote_edit_url(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Remote(RemotePopoverKind::EditUrlPrompt { name, kind }),
        }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_remote_edit_url(cx) {
            return;
        }
        let url = self
            .remote_url_edit_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        self.store.dispatch(Msg::SetRemoteUrl {
            repo_id,
            name,
            url,
            kind,
        });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn can_submit_remote_ssh_key(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.remote_ssh_key_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_remote_ssh_key(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Remote(RemotePopoverKind::SshKeyPrompt { name }),
        }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_remote_ssh_key(cx) {
            return;
        }
        let key = self
            .remote_ssh_key_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        self.store.dispatch(Msg::SetRemoteSshKey {
            repo_id,
            remote: name,
            key: Some(key),
        });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn clear_remote_ssh_key(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Remote(RemotePopoverKind::SshKeyPrompt { name }),
        }) = self.popover.clone()
        else {
            return;
        };
        self.store.dispatch(Msg::SetRemoteSshKey {
            repo_id,
            remote: name,
            key: None,
        });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn can_submit_push_set_upstream(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.push_upstream_branch_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_push_set_upstream(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(PopoverKind::PushSetUpstreamPrompt { repo_id, remote }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_push_set_upstream(cx) {
            return;
        }
        let branch = self
            .push_upstream_branch_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        self.store.dispatch(Msg::PushSetUpstream {
            repo_id,
            remote,
            branch,
        });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn can_submit_checkout_remote_branch(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.create_branch_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_checkout_remote_branch(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(PopoverKind::CheckoutRemoteBranchPrompt {
            repo_id,
            remote,
            branch,
        }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_checkout_remote_branch(cx) {
            return;
        }
        let local_branch = self
            .create_branch_input
            .read_with(cx, |i, _| i.text().trim().to_string());

        let local_branch_exists = self
            .state
            .repos
            .iter()
            .find(|r| r.id == repo_id)
            .and_then(|repo| match &repo.branches {
                Loadable::Ready(branches) => {
                    Some(branches.iter().any(|b| b.name == local_branch.as_str()))
                }
                _ => None,
            })
            .unwrap_or(false);
        if local_branch_exists {
            self.push_toast(
                components::ToastKind::Error,
                format!("Branch already exists: {local_branch}"),
                cx,
            );
            return;
        }

        self.store.dispatch(Msg::CheckoutRemoteBranch {
            repo_id,
            remote,
            branch,
            local_branch,
        });
        self.main_pane.update(cx, |pane, cx| {
            pane.rebuild_diff_cache(cx);
            cx.notify();
        });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn can_submit_worktree_add(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.worktree_path_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_worktree_add(&mut self, cx: &mut gpui::Context<Self>) {
        if self.suppress_worktree_submit_after_ref_enter {
            return;
        }
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
        }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_worktree_add(cx) {
            return;
        }
        let folder = self
            .worktree_path_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        let reference = self.worktree_ref_source_target.trim().to_string();
        let reference = (!reference.is_empty()).then_some(reference);
        self.store.dispatch(Msg::AddWorktree {
            repo_id,
            path: std::path::PathBuf::from(folder),
            reference,
        });
        self.close_popover(cx);
    }

    pub(in crate::view::panels) fn can_submit_submodule_add(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        self.submodule_url_input
            .read_with(cx, |i, _| !i.text().trim().is_empty())
            && self
                .submodule_path_input
                .read_with(cx, |i, _| !i.text().trim().is_empty())
    }

    pub(in crate::view::panels) fn submit_submodule_add(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::Repo {
            repo_id,
            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::AddPrompt),
        }) = self.popover.clone()
        else {
            return;
        };
        if !self.can_submit_submodule_add(cx) {
            return;
        }
        let url = self
            .submodule_url_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        let path_text = self
            .submodule_path_input
            .read_with(cx, |i, _| i.text().trim().to_string());
        let branch = self.submodule_branch_input.read_with(cx, |i, _| {
            let text = i.text().trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        });
        let name = self.submodule_name_input.read_with(cx, |i, _| {
            let text = i.text().trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        });
        let force = self.submodule_force_enabled;
        self.store.dispatch(Msg::AddSubmodule {
            repo_id,
            url,
            path: std::path::PathBuf::from(path_text),
            branch,
            name,
            force,
        });
        self.close_popover(cx);
    }
}
