//! AI review of one file — the diff view's "Review this file" answer.
//!
//! Shaped like `hunk_explanation`, but two things differ. The snapshot is a
//! *whole file's* patch rather than one hunk's, because a review is only
//! useful with the surrounding hunks in view. And the reply is parsed into
//! [`ReviewFinding`]s before it lands, so the panel renders a list — severity,
//! line, one sentence, one suggestion — instead of a wall of prose.

use super::*;

use crate::ai_commit::{ReviewFinding, ReviewSeverity};

/// One review request's lifecycle. Held on the host — not in the kind — because
/// the reply lands after the popover has already rendered, and a later visit to
/// the same file starts a fresh request rather than replaying an old answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FileReviewPhase {
    Generating,
    Ready(Vec<ReviewFinding>),
    Error(String),
}

/// The snapshot one review runs against.
pub(super) struct FileReview {
    /// Which file the review covers, for the identity line above the body.
    pub(super) path: std::path::PathBuf,
    /// The unified patch captured when the request was made. The diff can
    /// reload under the popover (a stage toggle moves every line), but the
    /// answer reviews this snapshot, and Retry resends it — re-deriving from
    /// the live diff would quietly review a different file state than the one
    /// shown above the body.
    pub(super) patch: String,
    pub(super) phase: FileReviewPhase,
}

/// The colour one severity renders in. Three levels map onto the theme's three
/// status colours, so a finding reads the same way a status elsewhere in the
/// app does.
fn severity_color(theme: AppTheme, severity: ReviewSeverity) -> gpui::Rgba {
    match severity {
        ReviewSeverity::Error => theme.colors.status.danger.foreground,
        ReviewSeverity::Warning => theme.colors.status.warning.foreground,
        ReviewSeverity::Info => theme.colors.status.info.foreground,
    }
}

impl PopoverHost {
    /// The diff menu's "Review this file": snapshot the file's patch, open the
    /// review popover anchored where the menu was, and start the request.
    /// Returns whether the popover opened, so the caller can leave its own
    /// close-path alone when it did.
    pub(super) fn start_file_review(
        &mut self,
        repo_id: RepoId,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(patch) = self.build_unified_patch_for_path(repo_id, path) else {
            self.push_toast(
                components::ToastKind::Error,
                crate::i18n::t!("toast.context_menu.patch_build_failed").into_owned(),
                cx,
            );
            return false;
        };
        // Open first: opening resets any earlier review, and the fresh snapshot
        // below is then the one the request runs against.
        let anchor = self.popover_anchor_point();
        self.open_popover_at(
            PopoverKind::FileReview {
                repo_id,
                path: path.to_path_buf(),
            },
            anchor,
            window,
            cx,
        );
        self.file_review = Some(FileReview {
            path: path.to_path_buf(),
            patch,
            phase: FileReviewPhase::Generating,
        });
        self.drive_file_review(cx);
        true
    }

    /// Send (or resend) the request for the stored snapshot. Shared by the menu
    /// entry and the popover's Retry button.
    fn drive_file_review(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(patch) = self.file_review.as_ref().map(|it| it.patch.clone()) else {
            return;
        };
        if let Some(review) = self.file_review.as_mut() {
            review.phase = FileReviewPhase::Generating;
        }

        // The request needs the network or a local CLI; test builds exercise
        // the popover's states directly and record what would have been sent.
        #[cfg(not(test))]
        {
            let settings = crate::ai_commit::current();
            let locale = rust_i18n::locale();
            cx.spawn(async move |host, cx| {
                let result = crate::ai_commit::generate_review(&settings, &patch, &locale).await;
                let _ = host.update(cx, |host, cx| {
                    host.finish_file_review(result, cx);
                });
            })
            .detach();
        }
        #[cfg(test)]
        {
            self.file_review_test_requests += 1;
            self.file_review_test_last_patch = Some(patch);
        }
        cx.notify();
    }

    /// Retry from the popover's error state: same snapshot, fresh request.
    pub(super) fn retry_file_review(&mut self, cx: &mut gpui::Context<Self>) {
        self.drive_file_review(cx);
    }

    /// Land the reply. The popover may have been closed while the request ran —
    /// the state still updates (a later open resets it first), it just has
    /// nothing on screen to repaint. A cancelled request is dropped: its answer
    /// would land over whatever the user moved on to.
    pub(super) fn finish_file_review(
        &mut self,
        result: std::result::Result<Vec<ReviewFinding>, String>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(review) = self.file_review.as_mut() else {
            return;
        };
        if review.phase != FileReviewPhase::Generating {
            return;
        }
        review.phase = match result {
            Ok(findings) => FileReviewPhase::Ready(findings),
            Err(message) => FileReviewPhase::Error(message),
        };
        cx.notify();
    }

    /// Cancel the in-flight review: the popover closes and the state drops, so
    /// the reply (when it lands) has nothing to write into. The underlying
    /// request itself is a single-shot CLI/HTTP call — dropping its landing is
    /// the cancellation this app can honestly offer.
    pub(super) fn cancel_file_review(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.file_review = None;
        self.close_popover(cx);
        let _ = window;
        cx.notify();
    }
}

/// One finding: severity and line on a header row, the sentence under it, and
/// the suggestion indented beneath that when the model offered one.
fn finding_card(
    theme: AppTheme,
    finding: &ReviewFinding,
    scaled_px: impl Fn(f32) -> Pixels + Copy,
) -> gpui::Div {
    let mut header = div().flex().items_center().gap(scaled_px(6.0)).child(
        div()
            .text_xs()
            .text_color(severity_color(theme, finding.severity))
            .child(finding.severity.label()),
    );
    if let Some(line) = finding.line {
        header = header.child(
            div()
                .text_xs()
                .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                .text_color(theme.colors.foreground.secondary)
                .child(crate::i18n::t!("panels.file_review.line", line = line).into_owned()),
        );
    }

    let mut card = div()
        .flex()
        .flex_col()
        .gap(scaled_px(2.0))
        .p_2()
        .rounded(px(theme.radii.control))
        .border_1()
        .border_color(theme.colors.stroke.subtle)
        .child(header)
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground.primary)
                .child(finding.title.clone()),
        );
    if !finding.suggestion.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(finding.suggestion.clone()),
        );
    }
    card
}

/// The answer body: one card per finding, capped and scrollable so a long list
/// cannot push the popover off screen.
fn findings_body(
    theme: AppTheme,
    findings: &[ReviewFinding],
    scaled_px: impl Fn(f32) -> Pixels + Copy,
) -> impl IntoElement {
    div()
        .id("file_review_findings")
        .debug_selector(|| "file_review_findings".to_string())
        .max_h(scaled_px(280.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(scaled_px(6.0))
        .px_2()
        .py_1()
        .children(
            findings
                .iter()
                .map(|finding| finding_card(theme, finding, scaled_px)),
        )
}

/// The body shown when the model found nothing to say: a review that came back
/// clean is a result, not an empty panel.
fn clean_body(theme: AppTheme) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .text_sm()
        .text_color(theme.colors.foreground.secondary)
        .debug_selector(|| "file_review_clean".to_string())
        .child(crate::i18n::tr("panels.file_review.clean"))
}

pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    _path: &std::path::Path,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);
    let close = components::Button::new(
        "file_review_close",
        crate::i18n::tr("panels.file_review.close"),
    )
    .style(components::ButtonStyle::Outlined)
    .on_click(theme, cx, |this, _e, _w, cx| {
        this.close_popover(cx);
    })
    .debug_selector(|| "file_review_close".to_string());

    let mut dialog = ConfirmDialog::new(
        crate::i18n::tr("panels.file_review.title"),
        DIALOG_540_WIDTH,
    );
    let actions: gpui::Div;

    match this.file_review.as_ref() {
        // Only start_file_review opens this kind, and it always stores a
        // snapshot first; the arm keeps the panel total without a panic.
        None => {
            actions = div();
        }
        Some(review) => {
            dialog = dialog.mono_value(theme, review.path.to_string_lossy().replace('\\', "/"));
            match &review.phase {
                FileReviewPhase::Generating => {
                    // Same body styling as `ConfirmDialog::text`, built by hand
                    // so the placeholder carries a debug selector for tests.
                    dialog = dialog.section(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .debug_selector(|| "file_review_generating".to_string())
                            .child(crate::i18n::tr("panels.file_review.generating")),
                    );
                    let stop = components::Button::new(
                        "file_review_stop",
                        crate::i18n::tr("panels.file_review.stop"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.cancel_file_review(window, cx);
                    })
                    .debug_selector(|| "file_review_stop".to_string());
                    actions = div().child(stop);
                }
                FileReviewPhase::Ready(findings) if findings.is_empty() => {
                    dialog = dialog.section(clean_body(theme));
                    actions = div();
                }
                FileReviewPhase::Ready(findings) => {
                    dialog = dialog.section(findings_body(theme, findings, scaled_px));
                    actions = div();
                }
                FileReviewPhase::Error(message) => {
                    dialog = dialog.section(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(theme.colors.status.danger.foreground)
                            .debug_selector(|| "file_review_error".to_string())
                            .child(message.clone()),
                    );
                    let retry = components::Button::new(
                        "file_review_retry",
                        crate::i18n::tr("panels.file_review.retry"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.retry_file_review(cx);
                    })
                    .debug_selector(|| "file_review_retry".to_string());
                    actions = div().child(retry);
                }
            }
        }
    }

    dialog.render(theme, close, actions, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_colors_come_from_the_theme_status_palette() {
        let theme = AppTheme::worktree_dark();
        assert_eq!(
            severity_color(theme, ReviewSeverity::Error),
            theme.colors.status.danger.foreground
        );
        assert_eq!(
            severity_color(theme, ReviewSeverity::Warning),
            theme.colors.status.warning.foreground
        );
        assert_eq!(
            severity_color(theme, ReviewSeverity::Info),
            theme.colors.status.info.foreground
        );
    }
}
