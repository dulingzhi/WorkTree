use super::*;
use repositorytree_core::domain::{LfsPointer, LfsPointerChange};

impl MainPaneView {
    /// Render the LFS pointer-change panel shown in place of the text diff
    /// for `filter=lfs` paths. The worktree holds pointer files, so the
    /// meaningful change is the content oid/size pair each side points at —
    /// the same summary the C# version's `SetLFSChange` view shows.
    pub(super) fn render_lfs_pointer_change(
        &mut self,
        theme: AppTheme,
        change: &LfsPointerChange,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let oid_font = crate::font_preferences::current_editor_font_family(cx);

        let pointer_row =
            |label: &'static str, pointer: Option<&LfsPointer>| -> gpui::Div {
                let oid_label: SharedString = match pointer.and_then(|p| p.oid.as_deref()) {
                    Some(oid) => format!("sha256:{}", &oid[..oid.len().min(12)]).into(),
                    None => crate::i18n::tr("diff.lfs.oid_absent"),
                };
                let size_label = format_lfs_size(pointer.and_then(|p| p.size));
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.colors.foreground.primary)
                            .w(px(64.0))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(oid_font.clone())
                            .text_color(theme.colors.foreground.secondary)
                            .child(oid_label),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(size_label),
                    )
            };

        div()
            .id("diff_lfs_panel")
            .debug_selector(|| "diff_lfs_panel".to_string())
            .flex()
            .flex_col()
            .flex_1()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .max_w_full()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.colors.foreground.primary)
                            .child(crate::i18n::tr("diff.lfs.title")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr("diff.lfs.hint")),
                    )
                    .child(pointer_row(
                        crate::i18n::tr_str("diff.lfs.old"),
                        change.old.as_ref(),
                    ))
                    .child(pointer_row(
                        crate::i18n::tr_str("diff.lfs.new"),
                        change.new.as_ref(),
                    )),
            )
            .into_any_element()
    }
}

/// Content size in the B/KiB/MiB ladder the binary conflict panel uses.
fn format_lfs_size(size: Option<u64>) -> SharedString {
    match size {
        None => crate::i18n::tr("diff.lfs.size_absent"),
        Some(n) if n < 1024 => format!("{} B", n).into(),
        Some(n) if n < 1024 * 1024 => format!("{:.1} KiB", n as f64 / 1024.0).into(),
        Some(n) => format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0)).into(),
    }
}
