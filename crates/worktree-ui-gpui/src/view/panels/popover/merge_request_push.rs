use super::*;

use super::stash_prompt::checkable_option_row;

/// How many `target..HEAD` commits feed the description prompt. The list is
/// context, not an exhaustive changelog; huge branches summarize poorly.
const MR_DESCRIPTION_COMMIT_CAP: usize = 50;

/// Parse `git log --pretty=%h%x1f%s%x1e` output into (short sha, subject)
/// pairs, skipping empty records. Pure so the caps and separators are
/// unit-testable without a repository.
pub(super) fn parse_mr_description_commits(log_output: &str) -> Vec<(String, String)> {
    log_output
        .split('\x1e')
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| record.split_once('\x1f'))
        .map(|(sha, subject)| (sha.trim().to_string(), subject.trim().to_string()))
        .take(MR_DESCRIPTION_COMMIT_CAP)
        .collect()
}

/// The branch the description should be written against: the target input
/// when filled, else the remote HEAD symbolic ref (`refs/remotes/origin/
/// main` → `main`) — GitLab's own default target. `None` when neither
/// resolves, which is a "fill the target first" error, not a guess.
pub(super) fn resolve_mr_description_target(
    target_input: &str,
    origin_head_ref: Option<&str>,
) -> Option<String> {
    let input = target_input.trim();
    if !input.is_empty() {
        return Some(input.to_string());
    }
    let reference = origin_head_ref?.trim();
    let rest = reference.strip_prefix("refs/remotes/")?;
    let branch = rest.split_once('/')?.1;
    (!branch.is_empty()).then(|| branch.to_string())
}

/// Push options and revision ranges arrive from a text field, so refuse
/// anything a `git log <target>..HEAD` could read as an option or a range
/// trick. Mirrors the gix layer's `validate_ref_like_arg` intent, inline
/// because that helper lives in the backend crate.
pub(super) fn target_branch_is_safe(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with('-')
        && !target.contains("..")
        && !target.contains(':')
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Run one git invocation in `workdir` and collect stdout, mapping a
/// non-zero exit to the trimmed stderr. Blocking — callers wrap it in
/// `smol::unblock`.
pub(in crate::view) fn git_output(workdir: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let mut command = worktree_core::process::git_command();
    command.current_dir(workdir).args(args);
    let output = command
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return if detail.is_empty() {
            Err(format!(
                "git {} failed with exit code {}",
                args.first().copied().unwrap_or_default(),
                output.status.code().unwrap_or(-1)
            ))
        } else {
            Err(detail.to_string())
        };
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Collect the description context for one repo: the commits between
/// `target` and HEAD plus the diffstat against the merge base. Blocking —
/// callers wrap it in `smol::unblock`.
#[cfg(not(test))]
pub(super) fn collect_mr_description_context(
    workdir: &std::path::Path,
    target: &str,
) -> Result<(Vec<(String, String)>, String), String> {
    if !target_branch_is_safe(target) {
        return Err(crate::i18n::tr_str("input.mr_push.target_unsafe").to_string());
    }
    let range = format!("{target}..HEAD");
    let log = git_output(
        workdir,
        &["log", "--pretty=%h%x1f%s%x1e", range.as_str()],
    )?;
    let commits = parse_mr_description_commits(&log);
    if commits.is_empty() {
        return Err(crate::i18n::tr_str("input.mr_push.no_commits").to_string());
    }
    let stat_range = format!("{target}...HEAD");
    let stat = git_output(
        workdir,
        &["diff", "--stat", stat_range.as_str()],
    )?;
    Ok((commits, stat))
}

/// Push HEAD carrying `git push -o merge_request.*` options so GitLab opens
/// the merge request from the push itself. `merge_request.create` is
/// implicit; the four rows below map one-to-one onto the C# client's push
/// dialog options.
pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_push = this.can_submit_mr_push();
    let scaled_px = super::popover_scaled_px_fn(cx);

    let mut body = div()
        .flex()
        .flex_col()
        .child(
            div()
                .id("mr_push_hint")
                .debug_selector(|| "mr_push_hint".to_string())
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(crate::i18n::tr("input.mr_push.hint")),
        )
        .child(
            div()
                .id("mr_push_target_row")
                .debug_selector(|| "mr_push_target_row".to_string())
                .px_2()
                .py_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.mr_push_target_input.clone()),
        )
        .child(
            checkable_option_row(
                "mr_push_pipeline_toggle",
                crate::i18n::tr("input.mr_push.pipeline"),
                theme,
                this.mr_push_merge_when_pipeline_succeeds,
                &this.mr_push_pipeline_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.mr_push_merge_when_pipeline_succeeds =
                    !this.mr_push_merge_when_pipeline_succeeds;
                cx.notify();
            })),
        )
        .child(
            checkable_option_row(
                "mr_push_remove_source_toggle",
                crate::i18n::tr("input.mr_push.remove_source"),
                theme,
                this.mr_push_remove_source_branch,
                &this.mr_push_remove_source_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.mr_push_remove_source_branch = !this.mr_push_remove_source_branch;
                cx.notify();
            })),
        )
        .child(
            checkable_option_row(
                "mr_push_mr_branch_toggle",
                crate::i18n::tr("input.mr_push.mr_branch"),
                theme,
                this.mr_push_push_to_mr_branch,
                &this.mr_push_mr_branch_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.mr_push_push_to_mr_branch = !this.mr_push_push_to_mr_branch;
                cx.notify();
            })),
        )
        .child(super::merge_request_push_description::section(this, theme, cx));

    body = body.child(
        div()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .child(
                cancel_button("mr_push_cancel", "mr_push_cancel_hint", theme)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.dismiss_prompt_popover(window, cx);
                    }),
            )
            .child(
                components::Button::new(
                    "mr_push_go",
                    crate::i18n::tr("input.mr_push.push"),
                )
                .separated_end_slot(super::hotkey_hint(theme, "mr_push_go_hint", "Enter"))
                .style(components::ButtonStyle::Filled)
                .disabled(!can_push)
                .on_click(theme, cx, |this, _e, window, cx| {
                    this.submit_mr_push(window, cx);
                }),
            ),
    );

    components::context_menu(
        theme,
        div()
            .id("mr_push_popover")
            .debug_selector(|| "mr_push_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(440.0))
            .child(popover_title(crate::i18n::tr("input.mr_push.title")))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mr_description_commits_reads_records_and_caps_them() {
        let mut log = String::new();
        for ix in 0..MR_DESCRIPTION_COMMIT_CAP + 10 {
            log.push_str(&format!("\nabc{ix:04} \x1f Subject {ix}\x1e"));
        }
        let commits = parse_mr_description_commits(&log);
        assert_eq!(commits.len(), MR_DESCRIPTION_COMMIT_CAP, "the cap applies");
        assert_eq!(commits[0], ("abc0000".to_string(), "Subject 0".to_string()));

        // Malformed records (no separator) are skipped, not fatal.
        let commits = parse_mr_description_commits("junk\x1ea1\x1fFix one\x1e\n");
        assert_eq!(commits, vec![("a1".to_string(), "Fix one".to_string())]);
    }

    #[test]
    fn resolve_mr_description_target_prefers_the_input_then_origin_head() {
        assert_eq!(
            resolve_mr_description_target(" feat/x ", None),
            Some("feat/x".to_string()),
            "a filled target wins, trimmed"
        );
        assert_eq!(resolve_mr_description_target("", None), None);
        assert_eq!(
            resolve_mr_description_target(
                "",
                Some("refs/remotes/origin/main\n")
            ),
            Some("main".to_string()),
            "the remote default branch stands in for an empty target"
        );
        assert_eq!(
            resolve_mr_description_target("", Some("refs/heads/main")),
            None,
            "a non-remote symbolic ref is not a default target"
        );
    }

    #[test]
    fn target_branch_is_safe_rejects_option_and_range_tricks() {
        for bad in ["-x", "a..b", "a:b", "a b", ""] {
            assert!(!target_branch_is_safe(bad), "{bad:?} must be refused");
        }
        assert!(target_branch_is_safe("feat/widget-2.0_x"));
        assert!(target_branch_is_safe("main"));
    }
}
