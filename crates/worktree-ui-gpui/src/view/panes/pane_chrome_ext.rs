//! Chrome plumbing shared by the pane views: every pane holds a
//! `WeakEntity<WorkTreeView>` back to the root and used to repeat the same
//! popover/context-menu delegation trio plus the base of `set_theme`. This
//! extension trait factors that delegation out, so each pane keeps only the
//! `root_view`/`theme_slot` accessors.

use super::super::{AppTheme, PopoverKind, WorkTreeView};
use gpui::{Bounds, Context, Pixels, Point, SharedString, WeakEntity, Window};

pub(in crate::view) trait PaneChromeExt: Sized + 'static {
    fn root_view(&self) -> &WeakEntity<WorkTreeView>;

    fn theme_slot(&mut self) -> &mut AppTheme;

    /// Popovers are owned by the root view; a direct `root_view.update()` can
    /// panic on the root→pane→root path, so hop through `cx.defer` and
    /// re-acquire the window from its handle.
    fn open_popover_at(
        &mut self,
        kind: PopoverKind,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root_view = self.root_view().clone();
        let window_handle = window.window_handle();
        cx.defer(move |cx| {
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = root_view.update(cx, |root, cx| {
                    root.open_popover_at(kind, anchor, window, cx);
                });
            });
        });
    }

    /// Bounds-anchored variant of [`Self::open_popover_at`].
    fn open_popover_for_bounds(
        &mut self,
        kind: PopoverKind,
        anchor_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root_view = self.root_view().clone();
        let window_handle = window.window_handle();
        cx.defer(move |cx| {
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = root_view.update(cx, |root, cx| {
                    root.open_popover_for_bounds(kind, anchor_bounds, window, cx);
                });
            });
        });
    }

    fn activate_context_menu_invoker(&mut self, invoker: SharedString, cx: &mut Context<Self>) {
        let _ = self.root_view().update(cx, move |root, cx| {
            root.set_active_context_menu_invoker(Some(invoker), cx);
        });
    }

    /// The shared core of every pane's `set_theme`: store the theme and
    /// request a re-render. Panes with themed child views keep their own
    /// `set_theme`, which calls this base first and then propagates.
    fn set_theme(&mut self, theme: AppTheme, cx: &mut Context<Self>) {
        *self.theme_slot() = theme;
        cx.notify();
    }
}
