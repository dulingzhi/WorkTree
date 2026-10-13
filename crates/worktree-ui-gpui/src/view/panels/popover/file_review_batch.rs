//! AI review of every changed file, collected into one Markdown summary.
//!
//! Shaped for a merge request: each file becomes its own section, so the
//! exported Markdown pastes as one review comment covering the whole change
//! instead of N separate ones.
//!
//! Requests run **one at a time**. A burst of concurrent model calls is slower
//! to a first answer, harder to cancel, and — on a large change — a good way
//! to find the provider's rate limit. One file failing does not stop the run:
//! its section records the failure and the rest continue.

use super::*;

use crate::ai_commit::ReviewFinding;

/// What one file came back with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FileReviewOutcome {
    Ready(Vec<ReviewFinding>),
    Error(String),
}

/// A whole-change review in progress. Held on the host — not in the kind —
/// because replies land after the popover has already rendered, and a second
/// run replaces the first rather than appending to it.
pub(super) struct FileReviewBatch {
    pub(super) repo_id: RepoId,
    /// Every file the run covers, in order.
    pub(super) paths: Vec<std::path::PathBuf>,
    /// One entry per finished file, kept in `paths` order.
    pub(super) results: Vec<(std::path::PathBuf, FileReviewOutcome)>,
    /// Index of the next file to request. `results.len()` files have landed, so
    /// `next > results.len()` means exactly one is in flight.
    pub(super) next: usize,
}

impl FileReviewBatch {
    fn total(&self) -> usize {
        self.paths.len()
    }

    fn is_done(&self) -> bool {
        self.results.len() == self.paths.len()
    }
}

impl PopoverHost {
    /// Review every changed file, one after another, into one summary.
    /// Returns whether the popover opened, so the caller can leave its own
    /// close-path alone when it did.
    pub(in crate::view) fn start_file_review_batch(
        &mut self,
        repo_id: RepoId,
        paths: Vec<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if paths.is_empty() {
            return false;
        }
        let anchor = self.popover_anchor_point();
        self.open_popover_at(PopoverKind::FileReviewBatch { repo_id }, anchor, window, cx);
        self.file_review_batch = Some(FileReviewBatch {
            repo_id,
            paths,
            results: Vec::new(),
            next: 0,
        });
        self.drive_next_batch_file(cx);
        true
    }

    /// Land one file's reply and start the next.
    pub(super) fn finish_file_review_batch(
        &mut self,
        result: std::result::Result<Vec<ReviewFinding>, String>,
        cx: &mut gpui::Context<Self>,
    ) {
        // Results land in request order, so the file at `results.len()` is the
        // one the finished request was for.
        let index = self.file_review_batch.as_ref().map(|b| b.results.len());
        let path = index.and_then(|i| {
            self.file_review_batch
                .as_ref()
                .and_then(|b| b.paths.get(i).cloned())
        });
        let Some(path) = path else {
            return;
        };
        if let Some(batch) = self.file_review_batch.as_mut() {
            batch.results.push((
                path,
                match result {
                    Ok(findings) => FileReviewOutcome::Ready(findings),
                    Err(message) => FileReviewOutcome::Error(message),
                },
            ));
        }
        cx.notify();
        self.drive_next_batch_file(cx);
    }

    /// Request the next file that has not been asked for yet.
    ///
    /// Loops rather than recursing: a file with no patch is skipped, and a
    /// large change with many such files must not grow the stack per file.
    fn drive_next_batch_file(&mut self, cx: &mut gpui::Context<Self>) {
        #[cfg(not(test))]
        let settings = crate::ai_commit::current();
        #[cfg(not(test))]
        let locale = rust_i18n::locale().to_string();

        loop {
            let Some((repo_id, path)) = self
                .file_review_batch
                .as_ref()
                .filter(|batch| batch.next < batch.paths.len())
                .map(|batch| (batch.repo_id, batch.paths[batch.next].clone()))
            else {
                return;
            };
            let Some(patch) = self.build_unified_patch_for_path(repo_id, &path) else {
                // No patch to review: record it and move on. Skipping silently
                // would understate how much of the change was actually looked
                // at.
                if let Some(batch) = self.file_review_batch.as_mut() {
                    batch.results.push((
                        path,
                        FileReviewOutcome::Error(
                            crate::i18n::t!("toast.context_menu.patch_build_failed").into_owned(),
                        ),
                    ));
                    batch.next += 1;
                }
                continue;
            };
            if let Some(batch) = self.file_review_batch.as_mut() {
                batch.next += 1;
            }

            #[cfg(not(test))]
            {
                let settings = settings.clone();
                let locale = locale.clone();
                cx.spawn(async move |host, cx| {
                    let result =
                        crate::ai_commit::generate_review(&settings, &patch, &locale).await;
                    let _ = host.update(cx, |host, cx| {
                        host.finish_file_review_batch(result, cx);
                    });
                })
                .detach();
            }
            #[cfg(test)]
            {
                self.file_review_batch_test_requests += 1;
                self.file_review_batch_test_last_patch = Some(patch);
            }
            cx.notify();
            return;
        }
    }

    /// Cancel the run: the popover closes and the state drops, so any reply
    /// still in flight lands on nothing.
    pub(super) fn cancel_file_review_batch(&mut self, cx: &mut gpui::Context<Self>) {
        self.file_review_batch = None;
        self.close_popover(cx);
        cx.notify();
    }
}

/// The Markdown one click puts on the clipboard: one section per file, in the
/// order they were reviewed. A file the model had nothing to say about is
/// omitted; a file that failed keeps its section so the export cannot claim to
/// have covered more than it did.
fn batch_to_markdown(results: &[(std::path::PathBuf, FileReviewOutcome)]) -> String {
    let mut out = String::new();
    for (path, outcome) in results {
        let path = path.to_string_lossy().replace('\\', "/");
        match outcome {
            FileReviewOutcome::Ready(findings) if findings.is_empty() => {}
            FileReviewOutcome::Ready(findings) => {
                out.push_str(&super::file_review::findings_to_markdown(&path, findings));
                out.push('\n');
            }
            FileReviewOutcome::Error(message) => {
                let heading = crate::i18n::t!("panels.file_review.markdown_heading", path = path)
                    .into_owned();
                out.push_str(&format!("# {heading}\n\n- {message}\n\n"));
            }
        }
    }
    out
}

/// One reviewed file as a row: its path, then how many findings it drew.
fn result_row(
    theme: AppTheme,
    path: &std::path::Path,
    outcome: &FileReviewOutcome,
    scaled_px: impl Fn(f32) -> Pixels + Copy,
) -> gpui::Div {
    let path = path.to_string_lossy().replace('\\', "/");
    let (count_text, color) = match outcome {
        FileReviewOutcome::Ready(findings) if findings.is_empty() => (
            crate::i18n::tr_str("panels.file_review_batch.clean").to_string(),
            theme.colors.foreground.secondary,
        ),
        FileReviewOutcome::Ready(findings) => (
            crate::i18n::t!("panels.file_review_batch.findings", count = findings.len())
                .into_owned(),
            theme.colors.status.warning.foreground,
        ),
        FileReviewOutcome::Error(_) => (
            crate::i18n::tr_str("panels.file_review_batch.failed").to_string(),
            theme.colors.status.danger.foreground,
        ),
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(scaled_px(8.0))
        .py_1()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .truncate()
                .text_color(theme.colors.foreground.primary)
                .child(path),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(color)
                .child(count_text),
        )
}

pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);
    let close = components::Button::new(
        "file_review_batch_close",
        crate::i18n::tr("panels.file_review_batch.close"),
    )
    .style(components::ButtonStyle::Outlined)
    .on_click(theme, cx, |this, _e, _w, cx| {
        this.close_popover(cx);
    })
    .debug_selector(|| "file_review_batch_close".to_string());

    let mut dialog = ConfirmDialog::new(
        crate::i18n::tr("panels.file_review_batch.title"),
        DIALOG_540_WIDTH,
    );
    let actions: gpui::Div;

    match this.file_review_batch.as_ref() {
        // Only start_file_review_batch opens this kind, and it always stores a
        // batch first; the arm keeps the panel total without a panic.
        None => {
            actions = div();
        }
        Some(batch) => {
            let total = batch.total();
            let done = batch.results.len();
            dialog = dialog.mono_value(
                theme,
                crate::i18n::t!(
                    "panels.file_review_batch.progress",
                    done = done,
                    total = total
                )
                .into_owned(),
            );

            if batch.is_done() {
                let results = batch.results.clone();
                let body = div()
                    .id("file_review_batch_results")
                    .debug_selector(|| "file_review_batch_results".to_string())
                    .max_h(scaled_px(280.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_1()
                    .children(
                        results
                            .iter()
                            .map(|(path, outcome)| result_row(theme, path, outcome, scaled_px)),
                    );
                dialog = dialog.section(body);

                let markdown = batch_to_markdown(&results);
                let copy = components::Button::new(
                    "file_review_batch_copy",
                    crate::i18n::tr("panels.file_review_batch.copy"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, window, cx| {
                    window.activate_window();
                    crate::clipboard::write_text(
                        cx,
                        markdown.clone(),
                        crate::clipboard::CopySource::FileReview,
                    );
                    this.push_toast(
                        components::ToastKind::Success,
                        crate::i18n::t!("panels.file_review_batch.copied").into_owned(),
                        cx,
                    );
                })
                .debug_selector(|| "file_review_batch_copy".to_string());
                actions = div().child(copy);
            } else {
                dialog = dialog.section(
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(theme.colors.foreground.secondary)
                        .debug_selector(|| "file_review_batch_generating".to_string())
                        .child(crate::i18n::tr("panels.file_review_batch.generating")),
                );
                let stop = components::Button::new(
                    "file_review_batch_stop",
                    crate::i18n::tr("panels.file_review_batch.stop"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, |this, _e, _w, cx| {
                    this.cancel_file_review_batch(cx);
                })
                .debug_selector(|| "file_review_batch_stop".to_string());
                actions = div().child(stop);
            }
        }
    }

    dialog.render(theme, close, actions, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(title: &str) -> ReviewFinding {
        ReviewFinding {
            severity: crate::ai_commit::ReviewSeverity::Warning,
            line: Some(7),
            title: title.into(),
            suggestion: String::new(),
        }
    }

    #[test]
    fn batch_to_markdown_gives_each_file_its_own_section() {
        let results = vec![
            (
                std::path::PathBuf::from("src/lib.rs"),
                FileReviewOutcome::Ready(vec![finding("Drops the error")]),
            ),
            (
                std::path::PathBuf::from("src/main.rs"),
                FileReviewOutcome::Ready(vec![finding("Shadowed binding")]),
            ),
        ];
        let markdown = batch_to_markdown(&results);
        assert!(
            markdown.contains("src/lib.rs"),
            "the first file heads its section: {markdown}"
        );
        assert!(
            markdown.contains("src/main.rs"),
            "so does the second: {markdown}"
        );
        assert!(
            markdown.contains("Drops the error") && markdown.contains("Shadowed binding"),
            "both files' findings survive: {markdown}"
        );
    }

    #[test]
    fn batch_to_markdown_skips_clean_files_and_keeps_failures() {
        let results = vec![
            (
                std::path::PathBuf::from("src/clean.rs"),
                FileReviewOutcome::Ready(Vec::new()),
            ),
            (
                std::path::PathBuf::from("src/broken.rs"),
                FileReviewOutcome::Error("boom".into()),
            ),
        ];
        let markdown = batch_to_markdown(&results);
        assert!(
            !markdown.contains("src/clean.rs"),
            "a file with nothing to say is omitted: {markdown}"
        );
        assert!(
            markdown.contains("src/broken.rs") && markdown.contains("boom"),
            "a failed file keeps its section, so the export cannot overclaim: {markdown}"
        );
    }
}
