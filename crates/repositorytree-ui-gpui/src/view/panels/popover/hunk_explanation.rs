use super::*;

/// One explanation request's lifecycle. Held on the host — not in the kind —
/// because the reply lands after the popover has already rendered, and a
/// later visit to the same hunk starts a fresh request rather than replaying
/// an old answer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HunkExplanationPhase {
    Generating,
    Ready(String),
    Error(String),
}

/// The snapshot one explanation runs against.
pub(super) struct HunkExplanation {
    /// The unified patch captured when the request was made. The diff can
    /// reload under the popover (a stage toggle moves every line), but the
    /// answer explains this snapshot, and Retry resends it — re-deriving from
    /// the live diff would quietly explain a different hunk than the one
    /// shown above the body.
    pub(super) patch: String,
    pub(super) phase: HunkExplanationPhase,
}

/// The identity line shown above the body: which file, and which hunk of it,
/// the explanation covers. The patch's file-header block names the file
/// (`+++ b/…`, with `diff --git a/… b/…` as the fallback for odd headers) and
/// the first `@@` line names the hunk.
pub(super) fn patch_summary(patch: &str) -> String {
    let file = patch
        .lines()
        .find_map(|line| line.strip_prefix("+++ b/"))
        .map(str::to_string)
        .or_else(|| {
            patch.lines().find_map(|line| line.strip_prefix("diff --git a/")).and_then(|rest| {
                rest.split_once(" b/").map(|(path, _)| path.to_string())
            })
        })
        .unwrap_or_default();
    let hunk = patch
        .lines()
        .find(|line| line.starts_with("@@"))
        .unwrap_or_default();
    match (file.is_empty(), hunk.is_empty()) {
        (false, false) => format!("{file}  {hunk}"),
        (false, true) => file,
        _ => hunk.to_string(),
    }
}

impl PopoverHost {
    /// The hunk menu's "Explain this change": snapshot the hunk's patch, open
    /// the explanation popover anchored where the menu was, and start the
    /// request. Returns whether the popover opened, so the caller can leave
    /// its own close-path alone when it did.
    pub(super) fn start_hunk_explanation(
        &mut self,
        repo_id: RepoId,
        src_ix: usize,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(patch) = self.build_unified_patch_for_hunk_src_ix(repo_id, src_ix) else {
            self.push_toast(
                components::ToastKind::Error,
                crate::i18n::t!("toast.context_menu.patch_build_failed").into_owned(),
                cx,
            );
            return false;
        };
        // Open first: opening resets any earlier explanation, and the fresh
        // snapshot below is then the one the request runs against.
        let anchor = self.popover_anchor_point();
        self.open_popover_at(
            PopoverKind::HunkExplanation { repo_id, src_ix },
            anchor,
            window,
            cx,
        );
        self.hunk_explanation = Some(HunkExplanation {
            patch,
            phase: HunkExplanationPhase::Generating,
        });
        self.drive_hunk_explanation(cx);
        true
    }

    /// Send (or resend) the request for the stored snapshot. Shared by the
    /// menu entry and the popover's Retry button.
    fn drive_hunk_explanation(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(patch) = self.hunk_explanation.as_ref().map(|it| it.patch.clone()) else {
            return;
        };
        if let Some(explanation) = self.hunk_explanation.as_mut() {
            explanation.phase = HunkExplanationPhase::Generating;
        }

        // The request needs the network or a local CLI; test builds exercise
        // the popover's states directly and record what would have been sent.
        #[cfg(not(test))]
        {
            let settings = crate::ai_commit::current();
            let locale = rust_i18n::locale();
            cx.spawn(async move |host, cx| {
                let result =
                    crate::ai_commit::generate_explanation(&settings, &patch, &locale).await;
                let _ = host.update(cx, |host, cx| {
                    host.finish_hunk_explanation(result, cx);
                });
            })
            .detach();
        }
        #[cfg(test)]
        {
            self.hunk_explanation_test_requests += 1;
            self.hunk_explanation_test_last_patch = Some(patch);
        }
        cx.notify();
    }

    /// Retry from the popover's error state: same snapshot, fresh request.
    pub(super) fn retry_hunk_explanation(&mut self, cx: &mut gpui::Context<Self>) {
        self.drive_hunk_explanation(cx);
    }

    /// Land the reply. The popover may have been closed while the request
    /// ran — the state still updates (a later open resets it first), it just
    /// has nothing on screen to repaint.
    pub(super) fn finish_hunk_explanation(
        &mut self,
        result: std::result::Result<String, String>,
        cx: &mut gpui::Context<Self>,
    ) {
        let phase = match result {
            Ok(text) => HunkExplanationPhase::Ready(text),
            Err(message) => HunkExplanationPhase::Error(message),
        };
        if let Some(explanation) = self.hunk_explanation.as_mut() {
            explanation.phase = phase;
            cx.notify();
        }
    }
}

/// The answer body: explanation text, one child per line so paragraphs and
/// bullets keep their shape, capped and scrollable so a long answer cannot
/// push the popover off screen.
fn explanation_body(
    theme: AppTheme,
    text: &str,
    cx: &mut gpui::Context<PopoverHost>,
) -> impl IntoElement {
    let scaled_px = super::popover_scaled_px_fn(cx);
    div()
        .id("hunk_explanation_text")
        .debug_selector(|| "hunk_explanation_text".to_string())
        .max_h(scaled_px(280.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .text_sm()
        .text_color(theme.colors.foreground.secondary)
        .children(text.split('\n').map(|line| {
            if line.trim().is_empty() {
                div().h(scaled_px(8.0))
            } else {
                div().child(line.to_owned())
            }
        }))
}

pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    _src_ix: usize,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let close = components::Button::new(
        "hunk_explanation_close",
        crate::i18n::tr("panels.hunk_explanation.close"),
    )
    .style(components::ButtonStyle::Outlined)
    .on_click(theme, cx, |this, _e, _w, cx| {
        this.close_popover(cx);
    })
    .debug_selector(|| "hunk_explanation_close".to_string());

    let mut dialog = ConfirmDialog::new(
        crate::i18n::tr("panels.hunk_explanation.title"),
        DIALOG_540_WIDTH,
    );
    let actions: gpui::Div;

    match this.hunk_explanation.as_ref() {
        // Only start_hunk_explanation opens this kind, and it always stores a
        // snapshot first; the arm keeps the panel total without a panic.
        None => {
            actions = div();
        }
        Some(explanation) => {
            dialog = dialog.mono_value(theme, patch_summary(&explanation.patch));
            match &explanation.phase {
                HunkExplanationPhase::Generating => {
                    // Same body styling as `ConfirmDialog::text`, built by hand
                    // so the placeholder carries a debug selector for tests.
                    dialog = dialog.section(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .debug_selector(|| "hunk_explanation_generating".to_string())
                            .child(crate::i18n::tr("panels.hunk_explanation.generating")),
                    );
                    actions = div();
                }
                HunkExplanationPhase::Ready(text) => {
                    dialog = dialog.section(explanation_body(theme, text, cx));
                    actions = div();
                }
                HunkExplanationPhase::Error(message) => {
                    dialog = dialog.section(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(theme.colors.status.danger.foreground)
                            .debug_selector(|| "hunk_explanation_error".to_string())
                            .child(message.clone()),
                    );
                    let retry = components::Button::new(
                        "hunk_explanation_retry",
                        crate::i18n::tr("panels.hunk_explanation.retry"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.retry_hunk_explanation(cx);
                    })
                    .debug_selector(|| "hunk_explanation_retry".to_string());
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
    fn summary_names_the_file_and_hunk() {
        let patch = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -10,7 +10,9 @@\n context\n-old\n+new";
        assert_eq!(patch_summary(patch), "src/lib.rs  @@ -10,7 +10,9 @@");
    }

    #[test]
    fn summary_falls_back_to_the_git_line_and_survives_odd_patches() {
        // No +++ line: the `diff --git` pair still names the file.
        let patch = "diff --git a/deep/path.rs b/deep/path.rs\n@@ -1 +1 @@\n-a\n+b";
        assert_eq!(patch_summary(patch), "deep/path.rs  @@ -1 +1 @@");

        // An empty patch degrades to an empty line rather than panicking.
        assert_eq!(patch_summary(""), "");
    }
}
