//! The MR push prompt's AI description section.
//!
//! The description does not travel with the push: GitLab's push options
//! have no reliable multiline channel, so this is a prepare-and-copy
//! affordance — generate from the commits about to be merged, edit if
//! needed, paste into the merge request.

use super::*;

/// The description block under the push options: header with the AI
/// generate action, an editable multiline field, and an inline error row.
pub(super) fn section(
    this: &mut PopoverHost,
    theme: AppTheme,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let scaled_px = super::popover_scaled_px_fn(cx);
    let generating = this.mr_push.mr_push_description_generating;
    let spinner_id = this
        .popover
        .as_ref()
        .map(|kind| match kind {
            PopoverKind::MergeRequestPushPrompt { repo_id } => repo_id.0,
            _ => 0,
        })
        .unwrap_or(0);

    let mut header = div().flex().items_center().gap(scaled_px(6.0)).child(
        div()
            .text_xs()
            .text_color(theme.colors.foreground.secondary)
            .child(crate::i18n::tr("input.mr_push.description")),
    );

    if generating {
        header = header.child(
            div()
                .id("mr_push_description_generating")
                .debug_selector(|| "mr_push_description_generating".to_string())
                .flex()
                .items_center()
                .gap(scaled_px(4.0))
                .child(crate::view::icons::svg_spinner(
                    ("mr_push_description_spinner", spinner_id),
                    theme.colors.foreground.secondary,
                    px(12.0),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr("input.mr_push.generating")),
                ),
        );
    }

    header = header.child(
        div()
            .ml_auto()
            .flex()
            .items_center()
            .gap(scaled_px(6.0))
            // Button ids do not surface in debug_bounds; the wrappers carry
            // the selectors the tests (and smoke tooling) click through.
            .child(
                div()
                    .debug_selector(|| "mr_push_copy_description".to_string())
                    .child(
                        components::Button::new(
                            "mr_push_copy_description_btn",
                            crate::i18n::tr("input.mr_push.copy"),
                        )
                        .on_click(theme, cx, |this, _e, window, cx| {
                            let text = this
                                .mr_push
                                .mr_push_description_input
                                .read_with(cx, |input, _| input.text().to_string());
                            if text.trim().is_empty() {
                                return;
                            }
                            window.activate_window();
                            crate::clipboard::write_text(
                                cx,
                                text,
                                crate::clipboard::CopySource::MrDescription,
                            );
                            this.push_toast(
                                components::ToastKind::Success,
                                crate::i18n::t!("input.mr_push.copied").into_owned(),
                                cx,
                            );
                        }),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "mr_push_generate_description".to_string())
                    .child(
                        components::Button::new(
                            "mr_push_generate_description_btn",
                            crate::i18n::tr("input.mr_push.generate"),
                        )
                        .start_slot(crate::view::icons::svg_icon(
                            "icons/sparkle.svg",
                            theme.colors.accent.foreground,
                            px(13.0),
                        ))
                        .disabled(generating)
                        .on_click(theme, cx, |this, _e, _window, cx| {
                            this.start_mr_description_generation(cx);
                        }),
                    ),
            ),
    );

    let mut section = div()
        .mt_1()
        .border_t_1()
        .border_color(theme.colors.stroke.subtle)
        .pt(scaled_px(4.0))
        .px_2()
        .pb_1()
        .flex()
        .flex_col()
        .gap(scaled_px(4.0))
        .child(header)
        .child(
            div()
                .id("mr_push_description_row")
                .debug_selector(|| "mr_push_description_input".to_string())
                .w_full()
                .min_w(px(0.0))
                .max_h(scaled_px(140.0))
                .overflow_y_scroll()
                .child(this.mr_push.mr_push_description_input.clone()),
        );

    if let Some(error) = this.mr_push.mr_push_description_error.clone() {
        section = section.child(
            div()
                .id("mr_push_description_error")
                .debug_selector(|| "mr_push_description_error".to_string())
                .px_1()
                .text_xs()
                .text_color(theme.colors.status.danger.foreground)
                .line_clamp(2)
                .child(error),
        );
    }

    section
}
