use super::*;
use crate::i18n::tr_str;
#[cfg(test)]
use crate::view::panes::{StatusSectionActionSelection, status_section_action_selection};
use gpui::{AnyElement, Div};

pub(in crate::view) const STATUS_SECTION_MIN_HEIGHT_PX: f32 = 80.0;

use crate::view::commit_message_text::{TextHighlights, commit_link_style};
type MessageLinks = Arc<[components::MessageLink]>;
type CommitMessageLinkHighlights = (TextHighlights, MessageLinks);

pub(in crate::view) fn merge_active(repo: Option<&RepoState>) -> bool {
    repo.is_some_and(|r| matches!(&r.merge_commit_message, Loadable::Ready(Some(_))))
}

pub(in crate::view) fn commit_allowed(is_merge_active: bool, staged_count: usize) -> bool {
    staged_count > 0 || is_merge_active
}

/// Author identity block: avatar + name + muted email, with the authored date
/// as a relative label (absolute date lives in the "Commit date" row below).
pub(in crate::view) fn commit_details_author_row(
    theme: AppTheme,
    ui_scale: crate::ui_scale::UiScale,
    details: &worktree_core::domain::CommitDetails,
    cx: &mut gpui::App,
) -> Option<Div> {
    if details.author_name.is_empty() && details.author_email.is_empty() {
        return None;
    }
    let display_name = if details.author_name.is_empty() {
        details.author_email.clone()
    } else {
        details.author_name.clone()
    };
    let author_email = (!details.author_email.is_empty()).then(|| details.author_email.as_str());
    // Attach the load-watcher for the remote avatar, if any — gpui only
    // notifies whichever single view first requested a remote image, so a
    // surface that lost that race would keep its initials stand-in until an
    // unrelated repaint.
    if let Some(url) = crate::avatar_source::avatar_url(author_email) {
        crate::avatar_source::ensure_avatar_loaded(&url, cx);
    }
    let authored_relative = (details.authored_at_unix != 0).then(|| {
        crate::view::date_time::format_relative_time(
            details.authored_at_unix,
            std::time::SystemTime::now(),
        )
    });

    Some(
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .min_w(px(0.0))
            .child(components::author_avatar_image(
                theme,
                ui_scale,
                &display_name,
                author_email,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_sm()
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .child(display_name),
                    )
                    .when(!details.author_email.is_empty(), |column| {
                        column.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .line_clamp(1)
                                .whitespace_nowrap()
                                .child(details.author_email.clone()),
                        )
                    }),
            )
            .when_some(authored_relative, |row, relative| {
                row.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(relative),
                )
            }),
    )
}

pub(in crate::view) const MULTI_COMMIT_ROW_HEIGHT_PX: f32 = 44.0;

/// How much of the comparison body the compared-commit cards may fill before
/// they start scrolling instead of growing. A range comparison has two
/// endpoints, but a multi-selection comparison has one card per selected
/// commit, and an unbounded column of those would push the changed-file list —
/// the part the user actually came for — off the bottom of the pane. At half,
/// the two lists split the body evenly once the selection is large enough to
/// need it, whatever height the pane happens to have.
pub(in crate::view) const COMPARISON_CARDS_MAX_BODY_FRACTION: f32 = 0.5;

/// Floor for the comparison's changed-file section — a label plus a row or two
/// of list. Keeps the capped card block above it from claiming the whole pane
/// when the pane is shorter than the card cap allows for.
pub(in crate::view) const RANGE_FILES_SECTION_MIN_HEIGHT_PX: f32 = 44.0;

pub(in crate::view) fn commit_details_selectable_row(
    theme: AppTheme,
    key: &'static str,
    value: AnyElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(key),
        )
        .child(div().w_full().min_w(px(0.0)).text_sm().child(value))
}

pub(in crate::view) fn commit_details_monospace_value(
    input: Entity<components::TextInput>,
) -> AnyElement {
    commit_details_monospace_element(input.into_any_element())
}

pub(in crate::view) fn commit_details_monospace_element(value: AnyElement) -> AnyElement {
    div()
        .font_family(crate::view::UI_MONOSPACE_FONT_FAMILY)
        .child(value)
        .into_any_element()
}

pub(in crate::view) fn commit_message_link_highlights(
    message: &str,
    theme: AppTheme,
) -> CommitMessageLinkHighlights {
    use crate::text_selection::MessageLinkKind;

    let style = commit_link_style(theme);
    let found = crate::text_selection::commit_message_link_ranges(message);
    let highlights = found
        .iter()
        .map(|link| (link.range.clone(), style))
        .collect::<Vec<_>>();
    let links = found
        .into_iter()
        .map(|link| {
            let text = &message[link.range.clone()];
            let target = match link.kind {
                MessageLinkKind::CommitSha => components::LinkTarget::Commit {
                    commit_id: CommitId(text.to_ascii_lowercase().into()),
                    allow_navigate: true,
                },
                MessageLinkKind::Url => components::LinkTarget::Url(text.to_owned().into()),
            };
            components::MessageLink {
                range: link.range,
                target,
            }
        })
        .collect::<Vec<_>>();

    (highlights, Arc::from(links))
}

/// The whole of a SHA field is one link, without scanning: the field holds
/// nothing but the id.
pub(in crate::view) fn commit_sha_field_links(
    sha: &str,
    interactive: bool,
    allow_navigate: bool,
) -> MessageLinks {
    if interactive {
        Arc::from([components::MessageLink {
            range: 0..sha.len(),
            target: components::LinkTarget::Commit {
                commit_id: CommitId(sha.to_string().into()),
                allow_navigate,
            },
        }])
    } else {
        Arc::<[components::MessageLink]>::from([])
    }
}

pub(in crate::view) fn commit_sha_field_highlights(value: &str, theme: AppTheme) -> TextHighlights {
    if value.is_empty() || value == "—" {
        Vec::new()
    } else {
        vec![(0..value.len(), commit_link_style(theme))]
    }
}

pub(in crate::view) fn min_change_tracking_stack_height(
    split_change_tracking: bool,
    handle_h: Pixels,
) -> Pixels {
    let section_min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
    if split_change_tracking {
        section_min_h * 2.0 + handle_h
    } else {
        section_min_h
    }
}

pub(in crate::view) fn clamp_vertical_split_height(
    requested_top: Pixels,
    total_height: Pixels,
    min_top: Pixels,
    min_bottom: Pixels,
) -> Pixels {
    if total_height <= px(0.0) {
        return px(0.0);
    }

    let min_total = min_top + min_bottom;
    if total_height <= min_total {
        return (total_height - min_bottom).max(px(0.0));
    }

    requested_top.max(min_top).min(total_height - min_bottom)
}

pub(in crate::view) fn resolved_vertical_split_height(
    requested_top: Option<Pixels>,
    total_height: Pixels,
    min_top: Pixels,
    min_bottom: Pixels,
) -> Pixels {
    if total_height <= px(0.0) {
        return px(0.0);
    }

    let default_top = (total_height * 0.5)
        .max(min_top)
        .min((total_height - min_bottom).max(px(0.0)));
    clamp_vertical_split_height(
        requested_top.unwrap_or(default_top),
        total_height,
        min_top,
        min_bottom,
    )
}

pub(in crate::view) fn visible_bounds_probe() -> Div {
    // Use a fill probe to capture the clipped viewport bounds for a container.
    // Unioning child bounds can stay larger than the visible area after window resizes.
    div().absolute().top_0().left_0().size_full()
}

/// Which wording the changed-files section headers draw. The full labels stop
/// fitting once the details pane is dragged narrow, and an over-wide action
/// group shoves the section title out of the panel — so the labels collapse to
/// initials before that happens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum StatusActionLabels {
    Full,
    Compact,
}

/// Average glyph advances at `text_sm` (14px at 100% zoom), regular and bold.
/// Budgeted rather than measured, the same way the history columns decide what
/// to drop (`view/panes/history.rs`).
///
/// Calibrated against the shipped UI font rather than guessed: at 100% zoom
/// `Stage all changes` renders 108px of ink over 17 characters (6.35/char) and
/// the bold `Unstaged` renders 59px over 8 (7.4/char). Guessed values that ran
/// a little high kept the header on the short labels while ~20px of room was
/// still going spare, so keep these honest — the header truncates its title
/// gracefully if a budget ever falls slightly short, but a budget that runs
/// long silently withholds the full wording.
const STATUS_ACTION_CHAR_WIDTH_PX: f32 = 6.4;
const STATUS_HEADER_TITLE_CHAR_WIDTH_PX: f32 = 7.4;
/// The header's own `px_2`, and the `gap_2` between the title and the action
/// group and between the buttons themselves.
const STATUS_HEADER_PAD_X_PX: f32 = 8.0;
const STATUS_HEADER_GAP_PX: f32 = 8.0;
/// A change-tracking dropdown title's `px_1` either side, its `gap_1`, and the
/// 12px chevron.
const STATUS_HEADER_DROPDOWN_EXTRA_PX: f32 = 24.0;
const STATUS_HEADER_SPINNER_PX: f32 = 14.0;

fn status_action_button_width_px(label_chars: usize) -> f32 {
    // `control_pad_x` each side, plus the 1px border every style reserves.
    2.0 * components::CONTROL_PAD_X_PX + 2.0 + label_chars as f32 * STATUS_ACTION_CHAR_WIDTH_PX
}

pub(in crate::view) fn status_action_labels_for_width(
    available_width: Pixels,
    title_chars: usize,
    title_is_dropdown: bool,
    action_label_chars: &[usize],
    has_spinner: bool,
    ui_scale_percent: u32,
) -> StatusActionLabels {
    if available_width <= px(0.0) || action_label_chars.is_empty() {
        return StatusActionLabels::Full;
    }

    let mut needed = 2.0 * STATUS_HEADER_PAD_X_PX
        + title_chars as f32 * STATUS_HEADER_TITLE_CHAR_WIDTH_PX
        + if title_is_dropdown {
            STATUS_HEADER_DROPDOWN_EXTRA_PX
        } else {
            0.0
        }
        // Between the title and the action group.
        + STATUS_HEADER_GAP_PX;
    if has_spinner {
        needed += STATUS_HEADER_SPINNER_PX + STATUS_HEADER_GAP_PX;
    }
    for (ix, chars) in action_label_chars.iter().enumerate() {
        if ix > 0 {
            needed += STATUS_HEADER_GAP_PX;
        }
        needed += status_action_button_width_px(*chars);
    }

    if crate::ui_scale::design_px_from_percent(needed, ui_scale_percent) <= available_width {
        StatusActionLabels::Full
    } else {
        StatusActionLabels::Compact
    }
}

/// Clipped forms of the action verbs. A bare initial was ambiguous — `S` and
/// `U` read as the same family, and the two of them plus `D` gave no clue which
/// button did what. These stay pronounceable at a glance.
fn status_action_short_word(word: &'static str) -> &'static str {
    match word {
        "Stage" => "Stg",
        "Discard" => "Disc",
        "Unstage" => "Ustg",
        other => other,
    }
}

/// `Stage (3)` → `Stg (3)`, `Stage all changes` → `All`.
pub(in crate::view) fn status_action_count_label(
    labels: StatusActionLabels,
    word: &'static str,
    count: usize,
) -> String {
    match labels {
        StatusActionLabels::Full => format!("{word} ({count})"),
        StatusActionLabels::Compact => {
            format!("{} ({count})", status_action_short_word(word))
        }
    }
}

pub(in crate::view) fn status_action_all_label(
    labels: StatusActionLabels,
    full: &'static str,
) -> &'static str {
    match labels {
        StatusActionLabels::Full => full,
        StatusActionLabels::Compact => tr_str("layout.status.all"),
    }
}

pub(in crate::view) fn status_action_file_count(count: usize) -> &'static str {
    if count == 1 {
        tr_str("layout.status.file_word")
    } else {
        tr_str("layout.status.files_word")
    }
}

/// Latin-budgeted width units for a locale-dependent label: CJK and other
/// full-width glyphs render at roughly double the budgeted Latin advance, so
/// they count as two. The header budget must measure the translated text, or
/// a short translated label would claim the room its longer English source
/// needs and the full wording would be withheld for no reason.
pub(in crate::view) fn label_width_chars(text: &str) -> usize {
    text.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::{Branch, CommitId, LogPage, RepoSpec};
    use worktree_state::model::{Loadable, RepoId, RepoState};

    fn test_repo() -> RepoState {
        RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        )
    }

    /// Label lengths the unstaged header asks about when three files are picked:
    /// `Stage (3)`, `Discard (3)`, `Stage all changes`.
    fn unstaged_header_with_selection() -> [usize; 3] {
        [
            "Stage (3)".len(),
            "Discard (3)".len(),
            "Stage all changes".len(),
        ]
    }

    #[test]
    fn status_action_labels_stay_full_in_a_wide_panel() {
        assert_eq!(
            status_action_labels_for_width(
                px(600.0),
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Full
        );
    }

    /// Guards the calibration in one direction only: a budget that runs long
    /// withholds the full wording while there is visibly room for it, which is
    /// the failure this pins. The number comes from measuring the shipped font
    /// — `Stage (3)`, `Discard (3)` and `Stage all changes` plus their padding,
    /// gaps and the `Unstaged` dropdown title need ~424px of real ink and box.
    #[test]
    fn status_action_labels_expand_as_soon_as_the_row_really_fits() {
        assert_eq!(
            status_action_labels_for_width(
                px(430.0),
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Full,
            "the full labels fit at this width in the real app, so the header must show them"
        );
    }

    #[test]
    fn status_action_labels_shrink_once_the_panel_is_narrow() {
        assert_eq!(
            status_action_labels_for_width(
                px(200.0),
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Compact
        );
    }

    #[test]
    fn status_action_labels_survive_narrower_without_a_selection() {
        // With nothing selected the header carries one button, so the width that
        // forces the three-button header to shrink is still comfortable here.
        let width = px(260.0);
        assert_eq!(
            status_action_labels_for_width(
                width,
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Compact
        );
        assert_eq!(
            status_action_labels_for_width(
                width,
                "Unstaged".len(),
                true,
                &["Stage all changes".len()],
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Full
        );
    }

    #[test]
    fn status_action_labels_account_for_the_in_flight_spinner() {
        // Sized to fit the buttons and title exactly, so the spinner is the only
        // thing that can push it over.
        let mut width = px(0.0);
        for candidate in (200..=600).step_by(2) {
            width = px(candidate as f32);
            if status_action_labels_for_width(
                width,
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ) == StatusActionLabels::Full
            {
                break;
            }
        }
        assert_eq!(
            status_action_labels_for_width(
                width,
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                true,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Compact,
            "the spinner's own width has to count against the budget"
        );
    }

    #[test]
    fn status_action_labels_shrink_earlier_when_zoomed_in() {
        let width = px(500.0);
        assert_eq!(
            status_action_labels_for_width(
                width,
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Full
        );
        assert_eq!(
            status_action_labels_for_width(
                width,
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                200,
            ),
            StatusActionLabels::Compact
        );
    }

    #[test]
    fn status_action_labels_default_to_full_before_the_panel_is_measured() {
        assert_eq!(
            status_action_labels_for_width(
                px(0.0),
                "Unstaged".len(),
                true,
                &unstaged_header_with_selection(),
                false,
                crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
            ),
            StatusActionLabels::Full
        );
    }

    #[test]
    fn status_action_labels_rewrite_the_wording() {
        assert_eq!(
            status_action_count_label(StatusActionLabels::Full, "Discard", 12),
            "Discard (12)"
        );
        assert_eq!(
            status_action_count_label(StatusActionLabels::Compact, "Stage", 3),
            "Stg (3)"
        );
        assert_eq!(
            status_action_count_label(StatusActionLabels::Compact, "Discard", 12),
            "Disc (12)"
        );
        assert_eq!(
            status_action_count_label(StatusActionLabels::Compact, "Unstage", 1),
            "Ustg (1)"
        );
        assert_eq!(
            status_action_all_label(StatusActionLabels::Full, "Unstage all changes"),
            "Unstage all changes"
        );
        assert_eq!(
            status_action_all_label(StatusActionLabels::Compact, "Unstage all changes"),
            "All"
        );
    }

    fn file_status(path: &str, kind: FileStatusKind) -> FileStatus {
        FileStatus {
            path: PathBuf::from(path),
            kind,
            conflict: None,
        }
    }

    fn repo_with_status(status: RepoStatus) -> RepoState {
        let mut repo = test_repo();
        repo.worktree_status = Loadable::Ready(Arc::new(status.unstaged.clone()));
        repo.worktree_status_rev = 1;
        repo.staged_status = Loadable::Ready(Arc::new(status.staged.clone()));
        repo.staged_status_rev = 1;
        repo.status = Loadable::Ready(status.into());
        repo.status_rev = 1;
        repo
    }

    fn branch(name: &str, target: &str) -> Branch {
        Branch {
            name: name.to_string(),
            target: CommitId(target.into()),
            upstream: None,
            divergence: None,
        }
    }

    #[test]
    fn commit_allowed_when_staged_changes_exist() {
        assert!(commit_allowed(false, 1));
    }

    #[test]
    fn commit_allowed_when_merge_is_active_without_staged_changes() {
        let mut repo = test_repo();
        repo.merge_commit_message = Loadable::Ready(Some("Merge branch 'feature'".to_string()));
        assert!(commit_allowed(merge_active(Some(&repo)), 0));
    }

    #[test]
    fn commit_not_allowed_without_staged_changes_or_merge() {
        assert!(!commit_allowed(false, 0));
    }

    #[test]
    fn amend_allowed_when_filtered_log_is_empty_but_head_branch_exists() {
        let mut repo = test_repo();
        repo.head_branch = Loadable::Ready("main".to_string());
        repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "abc123")]));
        repo.log = Loadable::Ready(Arc::new(LogPage {
            commits: Vec::new(),
            next_cursor: None,
        }));

        assert!(DetailsPaneView::can_submit_commit(
            Some(&repo),
            "message",
            true
        ));
    }

    #[test]
    fn amend_not_allowed_on_unborn_head_branch() {
        let mut repo = test_repo();
        repo.head_branch = Loadable::Ready("main".to_string());
        repo.branches = Loadable::Ready(Arc::new(Vec::new()));
        repo.log = Loadable::Ready(Arc::new(LogPage {
            commits: Vec::new(),
            next_cursor: None,
        }));

        assert!(!DetailsPaneView::can_submit_commit(
            Some(&repo),
            "message",
            true
        ));
    }

    #[test]
    fn amend_allowed_on_detached_head_without_visible_log_entry() {
        let mut repo = test_repo();
        repo.head_branch = Loadable::Ready("HEAD".to_string());
        repo.log = Loadable::Ready(Arc::new(LogPage {
            commits: Vec::new(),
            next_cursor: None,
        }));

        assert!(DetailsPaneView::can_submit_commit(
            Some(&repo),
            "message",
            true
        ));
    }

    #[test]
    fn split_height_clamps_to_minimum_section_heights() {
        let min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
        let total_h = px(400.0);

        let top_clamped = clamp_vertical_split_height(px(-300.0), total_h, min_h, min_h);
        let bottom_clamped = clamp_vertical_split_height(px(900.0), total_h, min_h, min_h);

        assert_eq!(top_clamped, min_h);
        assert_eq!(bottom_clamped, total_h - min_h);
    }

    #[test]
    fn resolved_split_height_defaults_to_half_when_unset() {
        let min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
        let total_h = px(400.0);

        assert_eq!(
            resolved_vertical_split_height(None, total_h, min_h, min_h),
            px(200.0)
        );
    }

    #[test]
    fn split_change_tracking_min_height_includes_inner_handle() {
        assert_eq!(
            min_change_tracking_stack_height(false, px(PANE_RESIZE_HANDLE_PX)),
            px(STATUS_SECTION_MIN_HEIGHT_PX)
        );
        assert_eq!(
            min_change_tracking_stack_height(true, px(PANE_RESIZE_HANDLE_PX)),
            px((STATUS_SECTION_MIN_HEIGHT_PX * 2.0) + PANE_RESIZE_HANDLE_PX)
        );
    }

    #[test]
    fn restored_status_section_heights_clamp_to_visible_minimums() {
        assert_eq!(
            DetailsPaneView::sanitized_restored_change_tracking_height(
                ChangeTrackingView::Combined,
                Some(1),
            ),
            Some(px(STATUS_SECTION_MIN_HEIGHT_PX))
        );
        assert_eq!(
            DetailsPaneView::sanitized_restored_change_tracking_height(
                ChangeTrackingView::SplitUntracked,
                Some(1),
            ),
            Some(px(
                (STATUS_SECTION_MIN_HEIGHT_PX * 2.0) + PANE_RESIZE_HANDLE_PX
            ))
        );
        assert_eq!(
            DetailsPaneView::sanitized_restored_untracked_height(Some(1)),
            Some(px(STATUS_SECTION_MIN_HEIGHT_PX))
        );
    }

    #[test]
    fn status_section_action_selection_falls_back_to_active_combined_unstaged_row() {
        let repo = repo_with_status(RepoStatus {
            unstaged: vec![file_status("src/lib.rs", FileStatusKind::Modified)],
            staged: Vec::new(),
        });
        let diff_target = DiffTarget::WorkingTree {
            path: PathBuf::from("src/lib.rs"),
            area: DiffArea::Unstaged,
        };

        let selection = status_section_action_selection(
            &repo,
            Some(&diff_target),
            None,
            StatusSection::CombinedUnstaged,
        );

        assert_eq!(
            selection,
            StatusSectionActionSelection {
                paths: vec![PathBuf::from("src/lib.rs")],
                from_explicit_selection: false,
            }
        );
        assert_eq!(selection.popover_path(), Some(PathBuf::from("src/lib.rs")));
    }

    #[test]
    fn status_section_action_selection_limits_active_row_to_matching_split_section() {
        let repo = repo_with_status(RepoStatus {
            unstaged: vec![
                file_status("new.txt", FileStatusKind::Untracked),
                file_status("src/lib.rs", FileStatusKind::Modified),
            ],
            staged: Vec::new(),
        });
        let diff_target = DiffTarget::WorkingTree {
            path: PathBuf::from("new.txt"),
            area: DiffArea::Unstaged,
        };

        let untracked = status_section_action_selection(
            &repo,
            Some(&diff_target),
            None,
            StatusSection::Untracked,
        );
        let unstaged = status_section_action_selection(
            &repo,
            Some(&diff_target),
            None,
            StatusSection::Unstaged,
        );

        assert_eq!(
            untracked,
            StatusSectionActionSelection {
                paths: vec![PathBuf::from("new.txt")],
                from_explicit_selection: false,
            }
        );
        assert!(unstaged.paths.is_empty());
    }

    #[test]
    fn status_section_action_selection_prefers_explicit_selection_over_active_row() {
        let selected_a = PathBuf::from("src/lib.rs");
        let selected_b = PathBuf::from("src/main.rs");
        let repo = repo_with_status(RepoStatus {
            unstaged: vec![
                file_status(
                    selected_a.to_string_lossy().as_ref(),
                    FileStatusKind::Modified,
                ),
                file_status(
                    selected_b.to_string_lossy().as_ref(),
                    FileStatusKind::Modified,
                ),
            ],
            staged: Vec::new(),
        });
        let diff_target = DiffTarget::WorkingTree {
            path: PathBuf::from("src/other.rs"),
            area: DiffArea::Unstaged,
        };
        let selection = StatusMultiSelection {
            unstaged: vec![selected_a.clone(), selected_b.clone()],
            ..Default::default()
        };

        let action_selection = status_section_action_selection(
            &repo,
            Some(&diff_target),
            Some(&selection),
            StatusSection::CombinedUnstaged,
        );

        assert_eq!(
            action_selection,
            StatusSectionActionSelection {
                paths: vec![selected_a, selected_b],
                from_explicit_selection: true,
            }
        );
        assert_eq!(action_selection.popover_path(), None);
    }
}
