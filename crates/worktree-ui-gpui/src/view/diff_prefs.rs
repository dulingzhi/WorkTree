#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ChangeTrackingView {
    #[default]
    Combined,
    SplitUntracked,
}

impl ChangeTrackingView {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Combined => "combined",
            Self::SplitUntracked => "split_untracked",
        }
    }

    pub(super) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "combined" => Some(Self::Combined),
            "split_untracked" => Some(Self::SplitUntracked),
            _ => None,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Combined => {
                crate::i18n::tr_str("ui.label.change_tracking.combined_with_unstaged")
            }
            Self::SplitUntracked => {
                crate::i18n::tr_str("ui.label.change_tracking.separate_section")
            }
        }
    }

    pub(super) fn menu_label(self) -> &'static str {
        match self {
            Self::Combined => crate::i18n::tr_str("ui.label.change_tracking.combine_with_unstaged"),
            Self::SplitUntracked => {
                crate::i18n::tr_str("ui.label.change_tracking.show_separate_untracked_block")
            }
        }
    }

    pub(super) fn settings_label(self) -> &'static str {
        match self {
            Self::Combined => crate::i18n::tr_str("ui.label.change_tracking.combined"),
            Self::SplitUntracked => {
                crate::i18n::tr_str("ui.label.change_tracking.separate_section")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum DiffScrollSync {
    Vertical,
    Horizontal,
    None,
    #[default]
    Both,
}

impl DiffScrollSync {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
            Self::None => "none",
            Self::Both => "both",
        }
    }

    pub(super) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "vertical" => Some(Self::Vertical),
            "horizontal" => Some(Self::Horizontal),
            "none" => Some(Self::None),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Vertical => crate::i18n::tr_str("ui.label.diff.scroll_sync.vertical"),
            Self::Horizontal => crate::i18n::tr_str("ui.label.diff.scroll_sync.horizontal"),
            Self::None => crate::i18n::tr_str("ui.label.diff.scroll_sync.none"),
            Self::Both => crate::i18n::tr_str("ui.label.diff.scroll_sync.both"),
        }
    }

    pub(super) const fn includes_vertical(self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }

    pub(super) const fn includes_horizontal(self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum DiffContentMode {
    #[default]
    Full,
    Collapsed,
}

impl DiffContentMode {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Full => "content",
            Self::Collapsed => "changed_lines_only",
        }
    }

    pub(super) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "content" => Some(Self::Full),
            "changed_lines_only" => Some(Self::Collapsed),
            _ => None,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Full => crate::i18n::tr_str("ui.label.diff.content.full"),
            Self::Collapsed => crate::i18n::tr_str("ui.label.diff.content.collapsed"),
        }
    }

    pub(super) fn settings_label(self) -> &'static str {
        self.label()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DiffWhitespaceMode {
    #[default]
    Show,
    Ignore,
}

impl DiffWhitespaceMode {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Ignore => "ignore",
        }
    }

    pub(crate) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "show" => Some(Self::Show),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }

    pub(crate) const fn toggled(self) -> Self {
        match self {
            Self::Show => Self::Ignore,
            Self::Ignore => Self::Show,
        }
    }
}
