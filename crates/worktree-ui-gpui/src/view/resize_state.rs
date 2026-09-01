use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HistoryColResizeHandle {
    Branch,
    Graph,
    Author,
    Date,
    Sha,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct HistoryColResizeState {
    pub(super) handle: HistoryColResizeHandle,
    pub(super) start_x: Pixels,
    pub(super) start_width: Pixels,
    pub(super) current_width: Pixels,
    pub(super) drag_delta_sign: f32,
    pub(super) min_width: Pixels,
    pub(super) static_max_width: Pixels,
    pub(super) other_fixed_width: Pixels,
    pub(super) bounds_available_width: Pixels,
    pub(super) max_width: Pixels,
    pub(super) visible_columns: (bool, bool, bool),
}

pub(super) struct ResizeDragGhost;

impl Render for ResizeDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div().w(px(0.0)).h(px(0.0))
    }
}

pub(super) use ResizeDragGhost as HistoryColResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PaneResizeHandle {
    Sidebar,
    Details,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaneResizeState {
    pub(super) handle: PaneResizeHandle,
    pub(super) start_x: Pixels,
    pub(super) start_width: Pixels,
    pub(super) other_width: Pixels,
    pub(super) drag_delta_sign: f32,
    pub(super) bounds_total_w: Pixels,
    pub(super) bounds_sidebar_collapsed: bool,
    pub(super) bounds_details_collapsed: bool,
    pub(super) min_width: Pixels,
    pub(super) max_width: Pixels,
}

impl PaneResizeState {
    #[inline]
    pub(super) fn new(
        handle: PaneResizeHandle,
        start_x: Pixels,
        start_sidebar: Pixels,
        start_details: Pixels,
        total_w: Pixels,
        sidebar_collapsed: bool,
        details_collapsed: bool,
    ) -> Self {
        let (min_width, start_width, other_width, other_collapsed, drag_delta_sign) = match handle {
            PaneResizeHandle::Sidebar => (
                px(super::SIDEBAR_MIN_PX),
                start_sidebar,
                start_details,
                details_collapsed,
                1.0,
            ),
            PaneResizeHandle::Details => (
                px(super::DETAILS_MIN_PX),
                start_details,
                start_sidebar,
                sidebar_collapsed,
                -1.0,
            ),
        };
        let (_, max_width) = super::pane_resize_drag_width_bounds_for_other_pane(
            min_width,
            other_width,
            other_collapsed,
            total_w,
            sidebar_collapsed,
            details_collapsed,
        );
        Self {
            handle,
            start_x,
            start_width,
            other_width,
            drag_delta_sign,
            bounds_total_w: total_w,
            bounds_sidebar_collapsed: sidebar_collapsed,
            bounds_details_collapsed: details_collapsed,
            min_width,
            max_width,
        }
    }

    #[inline]
    pub(super) fn drag_width_bounds(
        &self,
        total_w: Pixels,
        sidebar_collapsed: bool,
        details_collapsed: bool,
    ) -> (Pixels, Pixels) {
        if self.bounds_total_w == total_w
            && self.bounds_sidebar_collapsed == sidebar_collapsed
            && self.bounds_details_collapsed == details_collapsed
        {
            (self.min_width, self.max_width)
        } else {
            let other_collapsed = match self.handle {
                PaneResizeHandle::Sidebar => details_collapsed,
                PaneResizeHandle::Details => sidebar_collapsed,
            };
            super::pane_resize_drag_width_bounds_for_other_pane(
                self.min_width,
                self.other_width,
                other_collapsed,
                total_w,
                sidebar_collapsed,
                details_collapsed,
            )
        }
    }
}

pub(super) use ResizeDragGhost as PaneResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffSplitResizeHandle {
    Divider,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DiffSplitResizeState {
    pub(super) handle: DiffSplitResizeHandle,
    pub(super) start_x: Pixels,
    pub(super) start_ratio: f32,
}

pub(super) use ResizeDragGhost as DiffSplitResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum AnnotateResizeHandle {
    Divider,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::view) struct AnnotateResizeState {
    pub(in crate::view) start_x: Pixels,
    pub(in crate::view) start_width: f32,
}

pub(in crate::view) use ResizeDragGhost as AnnotateResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConflictVSplitResizeHandle {
    Divider,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ConflictVSplitResizeState {
    pub(super) start_y: Pixels,
    pub(super) start_ratio: f32,
}

pub(super) use ResizeDragGhost as ConflictVSplitResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StatusSectionResizeHandle {
    ChangeTrackingAndStaged,
    UntrackedAndUnstaged,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct StatusSectionResizeState {
    pub(super) handle: StatusSectionResizeHandle,
    pub(super) start_y: Pixels,
    pub(super) start_height: Pixels,
}

#[allow(unused_imports)]
pub(super) use ResizeDragGhost as StatusSectionResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConflictHSplitResizeHandle {
    First,
    Second,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ConflictHSplitResizeState {
    pub(super) handle: ConflictHSplitResizeHandle,
    pub(super) start_x: Pixels,
    pub(super) start_ratios: [f32; 2],
}

pub(super) use ResizeDragGhost as ConflictHSplitResizeDragGhost;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ConflictDiffSplitResizeHandle {
    Divider,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ConflictDiffSplitResizeState {
    pub(super) start_x: Pixels,
    pub(super) start_ratio: f32,
}

pub(super) use ResizeDragGhost as ConflictDiffSplitResizeDragGhost;

#[cfg(test)]
mod resize_drag_ghost_tests {
    use super::{
        ConflictDiffSplitResizeDragGhost, ConflictHSplitResizeDragGhost,
        ConflictVSplitResizeDragGhost, DiffSplitResizeDragGhost, HistoryColResizeDragGhost,
        PaneResizeDragGhost, ResizeDragGhost, StatusSectionResizeDragGhost,
    };
    use std::any::TypeId;

    #[test]
    fn all_resize_drag_ghost_aliases_use_shared_type() {
        let shared = TypeId::of::<ResizeDragGhost>();

        assert_eq!(TypeId::of::<HistoryColResizeDragGhost>(), shared);
        assert_eq!(TypeId::of::<PaneResizeDragGhost>(), shared);
        assert_eq!(TypeId::of::<DiffSplitResizeDragGhost>(), shared);
        assert_eq!(TypeId::of::<ConflictVSplitResizeDragGhost>(), shared);
        assert_eq!(TypeId::of::<StatusSectionResizeDragGhost>(), shared);
        assert_eq!(TypeId::of::<ConflictHSplitResizeDragGhost>(), shared);
        assert_eq!(TypeId::of::<ConflictDiffSplitResizeDragGhost>(), shared);
    }
}
