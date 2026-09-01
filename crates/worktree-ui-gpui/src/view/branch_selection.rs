use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct SelectedBranch {
    pub(in crate::view) repo_id: RepoId,
    pub(in crate::view) section: BranchSection,
    pub(in crate::view) name: String,
}

pub(in crate::view) fn selected_branch_label_color(theme: AppTheme) -> gpui::Rgba {
    theme.colors.foreground.emphasis
}

pub(in crate::view) fn selected_branch_row_bg(theme: AppTheme) -> gpui::Rgba {
    with_alpha(
        theme.colors.foreground.primary,
        if theme.is_dark { 0.16 } else { 0.10 },
    )
}

/// Which ref a history row should mark as the one the sidebar selected.
/// Carries the branch identity rather than its rendered label: the same branch
/// is drawn as `main` or `HEAD → main` depending on the row, so matching on
/// display text silently missed whichever form the row happened to use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct SelectedHistoryBranch {
    pub(in crate::view) section: BranchSection,
    pub(in crate::view) name: SharedString,
}

pub(in crate::view) fn selected_branch_for_history_row(
    selected_branch: Option<&SelectedBranch>,
    repo_id: RepoId,
    selected: bool,
) -> Option<SelectedHistoryBranch> {
    if !selected {
        return None;
    }

    let selected_branch = selected_branch?;
    if selected_branch.repo_id != repo_id {
        return None;
    }

    Some(SelectedHistoryBranch {
        section: selected_branch.section,
        name: SharedString::from(selected_branch.name.clone()),
    })
}
