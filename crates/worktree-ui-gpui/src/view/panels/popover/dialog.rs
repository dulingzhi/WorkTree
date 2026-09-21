use super::*;

/// Cancel/submit focus-handle pair shared by every prompt dialog.
pub(in crate::view::panels) struct DialogFocus {
    pub(in crate::view::panels) cancel: FocusHandle,
    pub(in crate::view::panels) submit: FocusHandle,
}

impl DialogFocus {
    pub(super) fn new(cx: &mut gpui::Context<PopoverHost>) -> Self {
        Self {
            cancel: cx.focus_handle().tab_index(0).tab_stop(true),
            submit: cx.focus_handle().tab_index(0).tab_stop(true),
        }
    }
}

pub(in crate::view) fn focusable_toggle_row<V: 'static>(
    id: &'static str,
    debug_selector: &'static str,
    theme: AppTheme,
    focus_handle: &FocusHandle,
    cx: &mut gpui::Context<V>,
) -> gpui::Stateful<gpui::Div> {
    let focus_handle = focus_handle.clone().tab_index(0).tab_stop(true);
    let hover_bg = theme.hover_overlay();
    let active_bg = theme.active_overlay();
    div()
        .id(id)
        .debug_selector(move || debug_selector.to_string())
        .w_full()
        .px_2()
        .py_1()
        .flex()
        .items_center()
        .justify_between()
        .rounded(px(theme.radii.row))
        .border_1()
        .border_color(gpui::transparent_black())
        .track_focus(&focus_handle)
        .cursor(CursorStyle::PointingHand)
        .hover(move |s| s.bg(hover_bg))
        .active(move |s| s.bg(active_bg))
        .focus(move |s| {
            s.bg(theme.colors.interaction.focus_background)
                .border_color(theme.colors.interaction.focus_ring)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_this, _e: &MouseDownEvent, window, cx| {
                window.focus(&focus_handle, cx);
            }),
        )
}

pub(in crate::view::panels) fn hotkey_hint(
    theme: AppTheme,
    debug_selector: &'static str,
    label: impl Into<SharedString>,
) -> gpui::Div {
    div()
        .debug_selector(move || debug_selector.to_string())
        .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child(label.into())
}

/// Shared Cancel button for confirm dialogs and prompt popovers: consistent
/// label, outlined style, and "Esc" hint. Attach the dismiss handler with
/// `.on_click(...)` at the call site.
pub(in crate::view::panels) fn cancel_button_labeled(
    id: &'static str,
    hint_debug_selector: &'static str,
    label: impl Into<SharedString>,
    theme: AppTheme,
) -> components::Button {
    components::Button::new(id, label)
        .separated_end_slot(hotkey_hint(theme, hint_debug_selector, "Esc"))
        .style(components::ButtonStyle::Outlined)
}

pub(in crate::view::panels) fn cancel_button(
    id: &'static str,
    hint_debug_selector: &'static str,
    theme: AppTheme,
) -> components::Button {
    cancel_button_labeled(
        id,
        hint_debug_selector,
        crate::i18n::tr("ui.common.cancel"),
        theme,
    )
}

/// Cancel button whose click simply closes the popover.
pub(in crate::view::panels) fn dialog_cancel_button(
    id: &'static str,
    hint_debug_selector: &'static str,
    theme: AppTheme,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Stateful<gpui::Div> {
    cancel_button(id, hint_debug_selector, theme).on_click(theme, cx, |this, _e, _w, cx| {
        this.close_popover(cx);
    })
}

pub(in crate::view::panels) fn dialog_divider(theme: AppTheme) -> gpui::Div {
    div().border_t_1().border_color(theme.colors.stroke.default)
}

/// Shared scaffolding for confirm-style dialogs: title, divider, body
/// sections, divider, then a footer with a cancel button on the left and the
/// action button(s) on the right. Width comes from the same `PopoverWidthSpec`
/// constants used by `popover_width_spec`, so the two can't drift apart.
pub(in crate::view::panels) struct ConfirmDialog {
    title: SharedString,
    width: PopoverWidthSpec,
    sections: Vec<AnyElement>,
}

impl ConfirmDialog {
    pub(in crate::view::panels) fn new(
        title: impl Into<SharedString>,
        width: PopoverWidthSpec,
    ) -> Self {
        Self {
            title: title.into(),
            width,
            sections: Vec::new(),
        }
    }

    /// Muted body paragraph.
    pub(in crate::view::panels) fn text(
        mut self,
        theme: AppTheme,
        text: impl Into<SharedString>,
    ) -> Self {
        self.sections.push(
            div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(text.into())
                .into_any_element(),
        );
        self
    }

    /// Smaller muted footnote.
    pub(in crate::view::panels) fn note(
        mut self,
        theme: AppTheme,
        text: impl Into<SharedString>,
    ) -> Self {
        self.sections.push(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(text.into())
                .into_any_element(),
        );
        self
    }

    /// Monospace value line (branch name, path, stash ref…).
    pub(in crate::view::panels) fn mono_value(
        mut self,
        theme: AppTheme,
        text: impl Into<SharedString>,
    ) -> Self {
        self.sections.push(
            div()
                .px_2()
                .py_1()
                .text_sm()
                .child(
                    div()
                        .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                        .text_color(theme.colors.foreground.secondary)
                        .child(text.into()),
                )
                .into_any_element(),
        );
        self
    }

    /// Monospace git command preview.
    pub(in crate::view::panels) fn command(
        mut self,
        theme: AppTheme,
        text: impl Into<SharedString>,
    ) -> Self {
        self.sections.push(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                .text_color(theme.colors.foreground.secondary)
                .child(text.into())
                .into_any_element(),
        );
        self
    }

    pub(in crate::view::panels) fn divider(mut self, theme: AppTheme) -> Self {
        self.sections.push(dialog_divider(theme).into_any_element());
        self
    }

    /// Escape hatch for dialog-specific body content.
    pub(in crate::view::panels) fn section(mut self, section: impl IntoElement) -> Self {
        self.sections.push(section.into_any_element());
        self
    }

    pub(in crate::view::panels) fn render(
        self,
        theme: AppTheme,
        cancel: impl IntoElement,
        actions: impl IntoElement,
        cx: &mut gpui::Context<PopoverHost>,
    ) -> gpui::Div {
        let ui_scale = popover_ui_scale(cx);
        div()
            .flex()
            .flex_col()
            .min_w(self.width.preferred_px(ui_scale))
            .child(popover_title(self.title))
            .child(dialog_divider(theme))
            .children(self.sections)
            .child(dialog_divider(theme))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(cancel)
                    .child(actions),
            )
    }
}

/// Whether the create/rename prompt's name field holds something worth
/// submitting.
///
/// Not just "non-empty": the prompt can open pre-filled with a group prefix
/// (`feat/`), and git rejects a ref ending in `/`. Without this the Create
/// button would be live the instant that prompt opens, and pressing it would
/// produce an error toast instead of the prompt simply declining.
pub(in crate::view::panels) fn is_submittable_branch_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && !name.ends_with('/')
}

pub(in crate::view::panels) fn popover_title(title: impl Into<SharedString>) -> gpui::Div {
    let title: SharedString = title.into();
    div()
        .px_2()
        .py_1()
        .text_sm()
        .font_weight(FontWeight::BOLD)
        .child(title)
}

pub(in crate::view::panels) fn input_label(theme: AppTheme, label: &'static str) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child(label)
}

impl PopoverHost {
    pub(super) fn prompt_tab_navigation_enabled(&self) -> bool {
        matches!(
            self.popover,
            Some(PopoverKind::CreateBranchFromRefPrompt { .. })
                | Some(PopoverKind::RenameBranchPrompt { .. })
                | Some(PopoverKind::CheckoutRemoteBranchPrompt { .. })
                | Some(PopoverKind::StashPrompt { .. })
                | Some(PopoverKind::StashBranchPrompt { .. })
                | Some(PopoverKind::CommitPrompt { .. })
                | Some(PopoverKind::CloneRepo)
                | Some(PopoverKind::CreateTagPrompt { .. })
                | Some(PopoverKind::SquashPrompt { .. })
                | Some(PopoverKind::AutosquashConfirm { .. })
                | Some(PopoverKind::MergePreview { .. })
                | Some(PopoverKind::RepoHooks { .. })
                | Some(PopoverKind::PushSetUpstreamPrompt { .. })
                | Some(PopoverKind::RepoSettingsPrompt { .. })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Remote(RemotePopoverKind::AddPrompt),
                    ..
                })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Remote(RemotePopoverKind::EditUrlPrompt { .. }),
                    ..
                })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Remote(RemotePopoverKind::SshKeyPrompt { .. }),
                    ..
                })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                    ..
                })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::AddPrompt),
                    ..
                })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Submodule(
                        SubmodulePopoverKind::ChangePointerPrompt { .. }
                    ),
                    ..
                })
        ) || self.popover.as_ref().is_some_and(popover_is_confirm_dialog)
    }

    pub(super) fn wrap_prompt_focus(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if forward {
            window.focus(&self.prompt_tab_group_focus_handle, cx);
            window.focus_next(cx);
        } else {
            window.focus(&self.prompt_tab_wrap_end_focus_handle, cx);
            window.focus_prev(cx);
        }
    }

    pub(super) fn focus_next_prompt_field(
        &mut self,
        _: &crate::view::PopoverPromptTabNext,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.prompt_tab_navigation_enabled() {
            return;
        }

        window.focus_next(cx);
        if !self
            .prompt_tab_group_focus_handle
            .contains_focused(window, cx)
        {
            self.wrap_prompt_focus(true, window, cx);
        }
        cx.stop_propagation();
    }

    pub(super) fn focus_prev_prompt_field(
        &mut self,
        _: &crate::view::PopoverPromptTabPrev,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.prompt_tab_navigation_enabled() {
            return;
        }

        window.focus_prev(cx);
        if !self
            .prompt_tab_group_focus_handle
            .contains_focused(window, cx)
        {
            self.wrap_prompt_focus(false, window, cx);
        }
        cx.stop_propagation();
    }
}
