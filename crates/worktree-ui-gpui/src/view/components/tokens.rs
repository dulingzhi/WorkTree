use crate::density::Density;
use crate::ui_scale::UiScale;

pub const CONTROL_HEIGHT_PX: f32 = 22.0;
/// Medium control height
pub const CONTROL_HEIGHT_MD_PX: f32 = 28.0;

/// Default horizontal padding for text buttons.
pub const CONTROL_PAD_X_PX: f32 = 10.0;
/// Default vertical padding for text buttons.
pub const CONTROL_PAD_Y_PX: f32 = 3.0;

/// Horizontal padding for icon-only buttons.
pub const ICON_PAD_X_PX: f32 = 6.0;

/// Horizontal inset applied to a list row's selection/hover highlight so the
/// rounded background reads as an inset pill/card rather than a full-bleed band.
pub const ROW_HIGHLIGHT_INSET_PX: f32 = 6.0;

/// Height of the divider between a split button's two halves. Deliberately far
/// short of the control height: each half now draws its own hover border, so a
/// full-height rule would read as a third frame rather than a seam.
pub const SPLIT_BUTTON_DIVIDER_HEIGHT_PX: f32 = 11.0;

/// Trailing close/remove affordance shared by repository tabs and the picker
/// rows that can drop an entry: a small hit box holding a danger-tinted X,
/// whose plate is the danger colour at these alphas. Both live off the same
/// tokens so the two buttons stay visually identical.
pub const REMOVE_BUTTON_ICON: &str = "icons/repo_tab_close.svg";
pub const REMOVE_BUTTON_SIZE_PX: f32 = 18.0;
pub const REMOVE_BUTTON_ICON_SIZE_PX: f32 = 12.0;
pub const REMOVE_BUTTON_HOVER_ALPHA: f32 = 0.18;
pub const REMOVE_BUTTON_PRESSED_ALPHA: f32 = 0.26;

pub fn control_height(scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(CONTROL_HEIGHT_PX)
}

pub fn control_height_md(scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(CONTROL_HEIGHT_MD_PX)
}

pub fn control_pad_x(scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(CONTROL_PAD_X_PX)
}

pub fn control_pad_y(scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(CONTROL_PAD_Y_PX)
}

pub fn icon_pad_x(scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(ICON_PAD_X_PX)
}

pub fn split_button_divider_height(scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(SPLIT_BUTTON_DIVIDER_HEIGHT_PX)
}

// Row-rhythm metrics per density tier. `Compact` preserves the pre-density
// spacing; `Comfortable` is the default breathing room. Every value is a
// design px that still flows through the percentage UI scale.

/// Commit rows in the history list, including the pinned uncommitted row.
const HISTORY_ROW_COMPACT_PX: f32 = 28.0;
const HISTORY_ROW_COMFORTABLE_PX: f32 = 32.0;
/// Sidebar content rows: branches (local and remote), worktrees, stashes,
/// tags, and the file browser.
const LIST_ROW_COMPACT_PX: f32 = 24.0;
const LIST_ROW_COMFORTABLE_PX: f32 = 26.0;
/// Sidebar section headers — two px taller than the rows beneath them so the
/// grouping reads before the items do.
const SECTION_HEADER_COMPACT_PX: f32 = 24.0;
const SECTION_HEADER_COMFORTABLE_PX: f32 = 28.0;
/// Status/commit file rows in the details pane and its sections.
const FILE_ROW_COMPACT_PX: f32 = 24.0;
const FILE_ROW_COMFORTABLE_PX: f32 = 26.0;
/// Blank space above the first sidebar row so it does not kiss the filter
/// bar.
const SIDEBAR_TOP_INSET_COMPACT_PX: f32 = 2.0;
const SIDEBAR_TOP_INSET_COMFORTABLE_PX: f32 = 6.0;
/// Blank space between sidebar sections — drawn as padding above each section
/// header's label (the sidebar's `uniform_list` gives every row the height it
/// measures for row zero, so a spacer row of its own height cannot apply
/// there). The one rhythm metric compact tightens beyond its pre-density
/// value — the sections still read apart at 6px, and dense-repository
/// scanning is exactly what compact is for.
const SIDEBAR_SECTION_GAP_COMPACT_PX: f32 = 6.0;
const SIDEBAR_SECTION_GAP_COMFORTABLE_PX: f32 = 10.0;

pub fn history_row_height_px(density: Density) -> f32 {
    match density {
        Density::Compact => HISTORY_ROW_COMPACT_PX,
        Density::Comfortable => HISTORY_ROW_COMFORTABLE_PX,
    }
}

pub fn list_row_height_px(density: Density) -> f32 {
    match density {
        Density::Compact => LIST_ROW_COMPACT_PX,
        Density::Comfortable => LIST_ROW_COMFORTABLE_PX,
    }
}

pub fn section_header_height_px(density: Density) -> f32 {
    match density {
        Density::Compact => SECTION_HEADER_COMPACT_PX,
        Density::Comfortable => SECTION_HEADER_COMFORTABLE_PX,
    }
}

pub fn file_row_height_px(density: Density) -> f32 {
    match density {
        Density::Compact => FILE_ROW_COMPACT_PX,
        Density::Comfortable => FILE_ROW_COMFORTABLE_PX,
    }
}

pub fn sidebar_top_inset_px(density: Density) -> f32 {
    match density {
        Density::Compact => SIDEBAR_TOP_INSET_COMPACT_PX,
        Density::Comfortable => SIDEBAR_TOP_INSET_COMFORTABLE_PX,
    }
}

pub fn sidebar_section_gap_px(density: Density) -> f32 {
    match density {
        Density::Compact => SIDEBAR_SECTION_GAP_COMPACT_PX,
        Density::Comfortable => SIDEBAR_SECTION_GAP_COMFORTABLE_PX,
    }
}

pub fn history_row_height(density: Density, scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(history_row_height_px(density))
}

pub fn list_row_height(density: Density, scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(list_row_height_px(density))
}

pub fn section_header_height(density: Density, scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(section_header_height_px(density))
}

pub fn file_row_height(density: Density, scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(file_row_height_px(density))
}

pub fn sidebar_top_inset(density: Density, scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(sidebar_top_inset_px(density))
}

pub fn sidebar_section_gap(density: Density, scale: impl Into<UiScale>) -> gpui::Pixels {
    scale.into().px(sidebar_section_gap_px(density))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comfortable_tiers_are_roomier_than_compact() {
        for (comfortable, compact) in [
            (
                history_row_height_px(Density::Comfortable),
                history_row_height_px(Density::Compact),
            ),
            (
                list_row_height_px(Density::Comfortable),
                list_row_height_px(Density::Compact),
            ),
            (
                section_header_height_px(Density::Comfortable),
                section_header_height_px(Density::Compact),
            ),
            (
                file_row_height_px(Density::Comfortable),
                file_row_height_px(Density::Compact),
            ),
            (
                sidebar_top_inset_px(Density::Comfortable),
                sidebar_top_inset_px(Density::Compact),
            ),
            (
                sidebar_section_gap_px(Density::Comfortable),
                sidebar_section_gap_px(Density::Compact),
            ),
        ] {
            assert!(comfortable > compact);
        }
    }

    #[test]
    fn section_headers_stay_taller_than_rows_in_both_tiers() {
        for density in [Density::Comfortable, Density::Compact] {
            assert!(section_header_height_px(density) >= list_row_height_px(density));
        }
    }

    #[test]
    fn compact_tier_preserves_the_pre_density_rhythm() {
        // These were the row heights before density existed; compact keeps the
        // dense look bit-for-bit for the heights that were already consistent.
        assert_eq!(history_row_height_px(Density::Compact), 28.0);
        assert_eq!(list_row_height_px(Density::Compact), 24.0);
        assert_eq!(section_header_height_px(Density::Compact), 24.0);
        assert_eq!(file_row_height_px(Density::Compact), 24.0);
        assert_eq!(sidebar_top_inset_px(Density::Compact), 2.0);
    }

    #[test]
    fn compact_tier_tightens_the_section_gap() {
        // The gap between sidebar sections is the one metric compact shrinks
        // past its pre-density value (10px); it is the explicit point of the
        // compact tier's tighter section rhythm.
        assert_eq!(sidebar_section_gap_px(Density::Compact), 6.0);
        assert_eq!(sidebar_section_gap_px(Density::Comfortable), 10.0);
    }
}
