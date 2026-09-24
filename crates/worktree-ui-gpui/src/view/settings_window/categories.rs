//! Navigation model: the left-nav categories, the expandable sections and
//! the root/licenses view switch, plus the nav rendering.

use super::*;
use gpui::Stateful;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsSection {
    Theme,
    Language,
    AvatarSource,
    UiScale,
    UiDensity,
    UiFont,
    EditorFont,
    ExternalCodeEditor,
    AiCommitMessage,
    DateFormat,
    Timezone,
    TerminalExternal,
    TerminalActionBar,
    ChangeTracking,
    DiffContentMode,
    Diff,
    DiffViewMode,
    GitLogDefaultMode,
    GitLogColumns,
    GitLogTagFetch,
    MergeTool,
}

impl SettingsSection {
    /// The left-nav category that owns this expandable section. Expanding a
    /// section always happens from within its owning category's page, so this
    /// mapping keeps the visible page and the expanded row in sync.
    pub(super) fn category(self) -> SettingsCategory {
        match self {
            Self::Theme
            | Self::Language
            | Self::AvatarSource
            | Self::UiScale
            | Self::UiDensity
            | Self::UiFont
            | Self::EditorFont
            | Self::ExternalCodeEditor
            | Self::AiCommitMessage
            | Self::DateFormat
            | Self::Timezone => SettingsCategory::General,
            Self::TerminalExternal | Self::TerminalActionBar => SettingsCategory::Terminal,
            Self::MergeTool => SettingsCategory::MergeTool,
            Self::ChangeTracking => SettingsCategory::ChangeTracking,
            Self::DiffContentMode | Self::Diff | Self::DiffViewMode => SettingsCategory::Diff,
            Self::GitLogDefaultMode | Self::GitLogColumns | Self::GitLogTagFetch => {
                SettingsCategory::GitLog
            }
        }
    }
}

/// A top-level settings grouping, shown as a row in the left-hand navigation.
/// Each category maps to one of the existing settings cards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsCategory {
    General,
    Terminal,
    ChangeTracking,
    Diff,
    FileEditing,
    GitLog,
    Tags,
    GitExecutable,
    GpgSigning,
    MergeTool,
    Environment,
    Links,
    Storage,
}

impl SettingsCategory {
    pub(super) const ALL: &'static [SettingsCategory] = &[
        SettingsCategory::General,
        SettingsCategory::Terminal,
        SettingsCategory::ChangeTracking,
        SettingsCategory::Diff,
        SettingsCategory::FileEditing,
        SettingsCategory::GitLog,
        SettingsCategory::Tags,
        SettingsCategory::GitExecutable,
        SettingsCategory::GpgSigning,
        SettingsCategory::MergeTool,
        SettingsCategory::Environment,
        SettingsCategory::Links,
        SettingsCategory::Storage,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::General => tr_str("settings.nav.general"),
            Self::Terminal => tr_str("settings.nav.terminal"),
            Self::ChangeTracking => tr_str("settings.nav.change_tracking"),
            Self::Diff => tr_str("settings.nav.diff"),
            Self::FileEditing => tr_str("settings.nav.file_editing"),
            Self::GitLog => tr_str("settings.nav.git_log"),
            Self::Tags => tr_str("settings.nav.tags"),
            Self::GitExecutable => tr_str("settings.nav.git_executable"),
            Self::GpgSigning => tr_str("settings.nav.gpg_signing"),
            Self::MergeTool => tr_str("settings.nav.merge_tool"),
            Self::Environment => tr_str("settings.nav.environment"),
            Self::Links => tr_str("settings.nav.links"),
            Self::Storage => tr_str("settings.nav.storage"),
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::General => "icons/cog.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::ChangeTracking => "icons/file.svg",
            Self::Diff => "icons/swap.svg",
            Self::FileEditing => "icons/pencil.svg",
            Self::GitLog => "icons/history.svg",
            Self::Tags => "icons/tag.svg",
            Self::GitExecutable => "icons/git_branch.svg",
            Self::GpgSigning => "icons/check.svg",
            Self::MergeTool => "icons/git_merge.svg",
            Self::Environment => "icons/computer.svg",
            Self::Links => "icons/link.svg",
            Self::Storage => "icons/disk.svg",
        }
    }

    pub(super) fn nav_id(self) -> &'static str {
        match self {
            Self::General => "settings_window_nav_general",
            Self::Terminal => "settings_window_nav_terminal",
            Self::ChangeTracking => "settings_window_nav_change_tracking",
            Self::Diff => "settings_window_nav_diff",
            Self::FileEditing => "settings_window_nav_file_editing",
            Self::GitLog => "settings_window_nav_git_log",
            Self::Tags => "settings_window_nav_tags",
            Self::GitExecutable => "settings_window_nav_git_executable",
            Self::GpgSigning => "settings_window_nav_gpg_signing",
            Self::MergeTool => "settings_window_nav_merge_tool",
            Self::Environment => "settings_window_nav_environment",
            Self::Links => "settings_window_nav_links",
            Self::Storage => "settings_window_nav_storage",
        }
    }

    /// Lowercase text (title plus the labels of the settings on the page) used
    /// to decide whether a category matches the nav search query.
    fn search_haystack(self) -> &'static str {
        match self {
            Self::General => {
                "general theme language ui language date format ui scale ui font editor font \
                 ligatures external code editor ai commit messages provider api key model \
                 endpoint date timezone appearance"
            }
            Self::Terminal => "terminal external terminal action bar terminal button opens",
            Self::ChangeTracking => "change tracking untracked files",
            Self::Diff => {
                "diff mode scroll sync show whitespace changes reveal whitespace characters \
                 word wrap show line numbers unified split"
            }
            Self::FileEditing => {
                "file editing edit file auto save autosave save automatically editor"
            }
            Self::GitLog => {
                "git log default history mode history columns relative dates show tags graph \
                 author sha"
            }
            Self::Tags => "tags automatically fetch tags",
            Self::GitExecutable => "git executable custom path system path version",
            Self::GpgSigning => {
                "gpg signing commit signing sign commits key program user.signingkey \
                 gpg.program verified signature"
            }
            Self::MergeTool => {
                "merge tool external mergetool conflict resolution kdiff3 meld beyond compare \
                 p4merge vs code sublime merge araxis winmerge tortoisegit filemerge vimdiff \
                 custom command trust exit code merge.tool"
            }
            Self::Environment => "environment build operating system app version",
            Self::Links => {
                "links theme guide github license open source licenses professional edition \
                 waitlist"
            }
            Self::Storage => "storage cache history cache disk space clear cache size entries path",
        }
    }

    pub(super) fn matches_query(self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        self.search_haystack().contains(query.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsView {
    Root,
    OpenSourceLicenses,
}

impl SettingsWindowView {
    pub(super) fn select_category(
        &mut self,
        category: SettingsCategory,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.selected_category == category {
            return;
        }
        self.selected_category = category;
        // Collapse any expanded row so the new page starts clean, and scroll
        // the content pane back to the top.
        self.expanded_section = None;
        self.settings_window_scroll
            .set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }

    pub(super) fn toggle_section(
        &mut self,
        section: SettingsSection,
        cx: &mut gpui::Context<Self>,
    ) {
        self.expanded_section = if self.expanded_section == Some(section) {
            None
        } else {
            Some(section)
        };
        // Expanding the AI section is the natural moment to (re)check the
        // selected source: the files and PATH entries it reads may have
        // changed since the window was last open.
        if self.expanded_section == Some(SettingsSection::AiCommitMessage)
            && self.ai_commit_source != crate::ai_commit_sources::AiSource::Manual
        {
            self.refresh_ai_commit_availability(cx);
        }
        // Same for the merge tool preset: what is on PATH may have changed
        // since the window was last open.
        if self.expanded_section == Some(SettingsSection::MergeTool) {
            self.refresh_merge_tool_availability(cx);
        }
        cx.notify();
    }

    pub(super) fn show_root(&mut self, cx: &mut gpui::Context<Self>) {
        if self.current_view == SettingsView::Root {
            return;
        }

        self.current_view = SettingsView::Root;
        cx.notify();
    }

    pub(super) fn show_open_source_licenses(&mut self, cx: &mut gpui::Context<Self>) {
        if self.current_view == SettingsView::OpenSourceLicenses {
            return;
        }

        self.current_view = SettingsView::OpenSourceLicenses;
        self.expanded_section = None;
        cx.notify();
    }

    fn settings_nav_item(
        &self,
        category: SettingsCategory,
        selected: bool,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> Stateful<gpui::Div> {
        let icon_color = if selected {
            theme.colors.accent.foreground
        } else {
            theme.colors.foreground.secondary
        };
        div()
            .id(category.nav_id())
            .debug_selector(move || category.nav_id().to_string())
            .w_full()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(theme.radii.row))
            .cursor(CursorStyle::PointingHand)
            .overflow_hidden()
            .when(selected, |d| {
                d.bg(theme.colors.interaction.pressed_background)
            })
            .when(!selected, |d| {
                d.hover(move |s| s.bg(theme.colors.interaction.hover_background))
            })
            .child(
                div()
                    .flex_shrink_0()
                    .child(svg_icon(category.icon(), icon_color, px(15.0))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_sm()
                    .when(selected, |d| d.font_weight(FontWeight::MEDIUM))
                    .text_color(theme.colors.foreground.primary)
                    .line_clamp(1)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(category.label()),
            )
            .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                this.select_category(category, cx);
            }))
    }

    pub(super) fn render_settings_nav(
        &self,
        active: SettingsCategory,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let query = self.search_query.clone();

        let mut list = div()
            .id("settings_window_nav_list")
            .debug_selector(|| "settings_window_nav_list".to_string())
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.nav_scroll)
            .flex()
            .flex_col()
            .gap(px(1.0));

        let mut any_match = false;
        for category in SettingsCategory::ALL.iter().copied() {
            if !category.matches_query(&query) {
                continue;
            }
            any_match = true;
            list = list.child(self.settings_nav_item(category, category == active, theme, cx));
        }

        if !any_match {
            list = list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.colors.foreground.secondary)
                    .child(tr_str("settings.nav.no_match")),
            );
        }

        div()
            .id("settings_window_nav")
            .debug_selector(|| "settings_window_nav".to_string())
            .flex_none()
            .w(px(200.0))
            .h_full()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(theme.colors.surface.chrome)
            .child(
                div()
                    .id("settings_window_nav_search")
                    .flex_none()
                    .w_full()
                    .child(self.search_input.clone()),
            )
            .child(list)
    }
}
