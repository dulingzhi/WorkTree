//! `PopoverHost` sync: pushing settings and modes out to the panes.

use super::super::*;
use super::popover_host::PopoverHost;
use super::kinds::PopoverKind;

// @split-module: impl_sync
impl PopoverHost {
    pub(in crate::view::panels::popover) fn sync_titlebar_app_menu_state(
        &self,
        cx: &mut gpui::Context<Self>,
    ) {
        let root_view = self.root_view.clone();
        let app_menu_open = matches!(self.popover, Some(PopoverKind::AppMenu));
        let repo_picker_open = matches!(self.popover, Some(PopoverKind::RepoPicker));
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.title_bar.update(cx, |title_bar, cx| {
                    title_bar.set_app_menu_open(app_menu_open, cx);
                    title_bar.set_repo_picker_open(repo_picker_open, cx);
                });
            });
        });
    }

    pub(in crate::view::panels::popover) fn sync_pane_date_settings(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let (format, timezone, show_timezone) =
            (self.date_time_format, self.timezone, self.show_timezone);
        self.details_pane.update(cx, |pane, cx| {
            pane.set_date_settings(format, timezone, show_timezone, cx);
        });
        self.reflog_pane.update(cx, |pane, cx| {
            pane.set_date_settings(format, timezone, show_timezone, cx);
        });
    }

    pub(in crate::view) fn sync_change_tracking_view(
        &mut self,
        next: ChangeTrackingView,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.change_tracking_view == next {
            return;
        }

        self.change_tracking_view = next;
        if matches!(self.popover, Some(PopoverKind::ChangeTrackingSettings)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_commit_push_after_enabled(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.commit_push_after_enabled == enabled {
            return;
        }

        self.commit_push_after_enabled = enabled;
        if matches!(self.popover, Some(PopoverKind::CommitOptionsMenu { .. })) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_push_pull_retry_enabled(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.push_pull_retry_enabled == enabled {
            return;
        }

        self.push_pull_retry_enabled = enabled;
        if matches!(self.popover, Some(PopoverKind::PushPicker)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_commit_amend_enabled(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.commit_amend_enabled == enabled {
            return;
        }

        self.commit_amend_enabled = enabled;
        if matches!(self.popover, Some(PopoverKind::CommitOptionsMenu { .. })) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_content_mode(
        &mut self,
        next: DiffContentMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_content_mode == next {
            return;
        }

        self.diff_content_mode = next;
        if matches!(self.popover, Some(PopoverKind::DiffContentModeSettings)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_whitespace_mode(
        &mut self,
        next: DiffWhitespaceMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_whitespace_mode == next {
            return;
        }

        self.diff_whitespace_mode = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_reveal_whitespace_chars(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_reveal_whitespace_chars == next {
            return;
        }

        self.diff_reveal_whitespace_chars = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_word_wrap(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_word_wrap == next {
            return;
        }

        self.diff_word_wrap = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_show_line_numbers(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_show_line_numbers == next {
            return;
        }

        self.diff_show_line_numbers = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }
}
