use super::*;
use worktree_core::domain::{LfsPointer, LfsPointerChange};

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
        let preview_section = self.render_lfs_image_preview_section(theme, change, cx);

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
                    ))
                    .when_some(preview_section, |panel, section| panel.child(section)),
            )
            .into_any_element()
    }

    /// The image preview under the pointer rows. Smudging may download the
    /// object from the LFS server, so the load starts only from the button —
    /// never on opening the file.
    fn render_lfs_image_preview_section(
        &mut self,
        theme: AppTheme,
        change: &LfsPointerChange,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        // Only an image path offers a preview, and only a present new side
        // has anything to show.
        change.new.as_ref()?;
        let (repo_id, target, path) = {
            let repo = self.active_repo()?;
            let target = repo.diff_state.diff_target.clone()?;
            let path = match &target {
                DiffTarget::WorkingTree { path, .. } => path.clone(),
                DiffTarget::Commit {
                    path: Some(path), ..
                }
                | DiffTarget::CommitRange {
                    path: Some(path), ..
                } => path.clone(),
                _ => return None,
            };
            (repo.id, target, path)
        };
        let format = crate::view::diff_utils::image_format_for_path(&path)?;
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let scaled_px =
            move |value: f32| crate::ui_scale::design_px_from_percent(value, ui_scale_percent);
        let preview = self
            .active_repo()
            .map(|repo| &repo.diff_state.lfs_image_preview);

        let body = match preview {
            Some(Loadable::NotLoaded) => div()
                .id("lfs_image_load")
                .debug_selector(|| "lfs_image_load".to_string())
                .child(
                    components::Button::new(
                        "lfs_image_load_btn",
                        crate::i18n::tr("diff.lfs.load_preview"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        this.store.dispatch(Msg::LoadLfsImagePreview {
                            repo_id,
                            target: target.clone(),
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
            Some(Loadable::Loading) => div()
                .id("lfs_image_loading")
                .debug_selector(|| "lfs_image_loading".to_string())
                .flex()
                .items_center()
                .gap(scaled_px(6.0))
                .child(crate::view::icons::svg_spinner(
                    ("lfs_image_spinner", repo_id.0),
                    theme.colors.foreground.secondary,
                    px(12.0),
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr("diff.lfs.loading_preview")),
                )
                .into_any_element(),
            Some(Loadable::Error(message)) => div()
                .id("lfs_image_error")
                .debug_selector(|| "lfs_image_error".to_string())
                .text_sm()
                .text_color(theme.colors.status.danger.foreground)
                .line_clamp(2)
                .child(message.clone())
                .into_any_element(),
            Some(Loadable::Ready(image)) => match image.as_ref().and_then(|image| image.new.as_ref())
            {
                Some(bytes) => gpui::img(std::sync::Arc::new(gpui::Image::from_bytes(
                    format,
                    bytes.clone(),
                )))
                    .max_w_full()
                    .max_h(scaled_px(320.0))
                    .object_fit(gpui::ObjectFit::Contain)
                    .debug_selector(|| "lfs_image_preview".to_string())
                    .into_any_element(),
                None => return None,
            },
            None => return None,
        };
        Some(
            div()
                .mt_2()
                .pt_2()
                .border_t_1()
                .border_color(theme.colors.stroke.subtle)
                .flex()
                .flex_col()
                .items_start()
                .gap(scaled_px(4.0))
                .child(body)
                .into_any_element(),
        )
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
