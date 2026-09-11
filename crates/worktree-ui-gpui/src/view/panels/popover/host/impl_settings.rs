//! `PopoverHost` settings: theme, pinned branches, filters and the persist
//! scheduling around them.

use super::super::*;
use super::popover_host::PopoverHost;

// @split-module: impl_settings
impl PopoverHost {
    pub(in crate::view) fn set_theme(&mut self, theme: AppTheme, cx: &mut gpui::Context<Self>) {
        self.theme = theme;

        let inputs: Vec<_> = self.all_text_inputs().cloned().collect();
        for input in inputs {
            input.update(cx, |input, cx| input.set_theme(theme, cx));
        }

        cx.notify();
    }

    pub(in crate::view) fn set_pinned_branches(
        &mut self,
        pinned: std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<String>>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.pinned_branches_by_repo == pinned {
            return;
        }
        self.pinned_branches_by_repo = pinned;
        cx.notify();
    }

    pub(in crate::view) fn set_branch_filter_query(
        &mut self,
        query: String,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.branch_filter_query == query {
            return;
        }
        self.branch_filter_query = query;
        cx.notify();
    }

    pub(in crate::view) fn set_collapsed_items(
        &mut self,
        collapsed: std::collections::BTreeMap<
            std::path::PathBuf,
            std::collections::BTreeSet<String>,
        >,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.collapsed_items_by_repo == collapsed {
            return;
        }
        self.collapsed_items_by_repo = collapsed;
        cx.notify();
    }

    pub(in crate::view) fn set_date_time_format(
        &mut self,
        next: DateTimeFormat,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.date_time_format == next {
            return;
        }
        self.date_time_format = next;
        self.main_pane
            .update(cx, |pane, cx| pane.set_date_time_format(next, cx));
        self.sync_pane_date_settings(cx);
        self.schedule_ui_settings_persist(cx);
    }

    pub(in crate::view) fn set_timezone(&mut self, next: Timezone, cx: &mut gpui::Context<Self>) {
        if self.timezone == next {
            return;
        }
        self.timezone = next;
        self.main_pane
            .update(cx, |pane, cx| pane.set_timezone(next, cx));
        self.sync_pane_date_settings(cx);
        self.schedule_ui_settings_persist(cx);
    }

    pub(in crate::view) fn set_show_timezone(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.show_timezone == enabled {
            return;
        }
        self.show_timezone = enabled;
        self.main_pane
            .update(cx, |pane, cx| pane.set_show_timezone(enabled, cx));
        self.sync_pane_date_settings(cx);
        self.schedule_ui_settings_persist(cx);
    }

    pub(in crate::view) fn set_theme_mode(
        &mut self,
        next: ThemeMode,
        appearance: gpui::WindowAppearance,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.theme_mode == next {
            return;
        }

        self.theme_mode = next.clone();
        self.set_theme(next.resolve_theme(appearance), cx);
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.set_theme_mode(next.clone(), appearance, cx);
            });
        });
    }

    pub(in crate::view::panels::popover) fn schedule_ui_settings_persist(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let mode = self.theme_mode.clone();
        let fmt = self.date_time_format;
        let tz = self.timezone;
        let show_tz = self.show_timezone;
        let root_view = self.root_view.clone();
        cx.spawn(
            async move |_host: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let _ = root_view.update(cx, |root, cx| {
                    root.theme_mode = mode;
                    root.date_time_format = fmt;
                    root.timezone = tz;
                    root.show_timezone = show_tz;
                    root.schedule_ui_settings_persist(cx);
                });
            },
        )
        .detach();
    }
}
