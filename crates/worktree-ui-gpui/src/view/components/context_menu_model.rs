use super::*;
use crate::view::panels::ContextMenuAction;
use gpui::SharedString;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Clone)]
pub(in crate::view) enum ContextMenuItem {
    Separator,
    Header(ContextMenuText),
    /// Muted helper text placed directly under the menu's header.
    Description(ContextMenuText),
    Label(ContextMenuText),
    Entry {
        label: SharedString,
        icon: Option<SharedString>,
        shortcut: Option<SharedString>,
        disabled: bool,
        action: Box<ContextMenuAction>,
    },
    /// A collapsible group of entries rendered inline: activating the row
    /// reveals the children indented beneath it. Menu lists that would run
    /// long (the repo tab's external tools) fold into one row this way.
    Submenu {
        /// Stable key tracking the group's open state across rebuilds.
        id: SharedString,
        label: SharedString,
        icon: Option<SharedString>,
        children: Vec<ContextMenuItem>,
    },
    /// A caption plus a segmented control, for settings whose options are
    /// mutually exclusive and read better side by side than as a checked list
    /// (the merge tool's view mode). Segments are clicked, not
    /// keyboard-selected, so the row is skipped by arrow navigation.
    Segmented {
        label: SharedString,
        segments: Vec<ContextMenuSegment>,
    },
}

/// One option inside a [`ContextMenuItem::Segmented`] row.
#[derive(Clone)]
pub(in crate::view) struct ContextMenuSegment {
    /// Stable element id, also used as the debug selector.
    pub(in crate::view) id: SharedString,
    pub(in crate::view) label: SharedString,
    pub(in crate::view) tooltip: Option<SharedString>,
    pub(in crate::view) selected: bool,
    pub(in crate::view) action: ContextMenuAction,
}

#[derive(Clone)]
pub(in crate::view) struct ContextMenuModel {
    pub(in crate::view) items: Vec<ContextMenuItem>,
    /// Render shortcut labels as individual keycaps, matching the Command
    /// Palette. Most context menus use shortcuts as single-key mnemonics, so
    /// this remains opt-in.
    pub(in crate::view) shortcut_keycaps: bool,
    /// Optional hover tooltip per entry index (e.g. the full commit message in the
    /// browse-history menu). Sparse — most menus leave this empty.
    pub(in crate::view) entry_tooltips: FxHashMap<usize, SharedString>,
    /// Stable debug selectors for menus whose entries predate the shared context-menu
    /// renderer. Sparse so ordinary menus continue deriving selectors from labels.
    pub(in crate::view) entry_debug_selectors: FxHashMap<usize, SharedString>,
}

impl ContextMenuModel {
    pub(in crate::view) fn new(items: Vec<ContextMenuItem>) -> Self {
        Self {
            items,
            shortcut_keycaps: false,
            entry_tooltips: FxHashMap::default(),
            entry_debug_selectors: FxHashMap::default(),
        }
    }

    pub(in crate::view) fn with_shortcut_keycaps(mut self) -> Self {
        self.shortcut_keycaps = true;
        self
    }

    pub(in crate::view) fn with_entry_tooltips(
        mut self,
        entry_tooltips: FxHashMap<usize, SharedString>,
    ) -> Self {
        self.entry_tooltips = entry_tooltips;
        self
    }

    pub(in crate::view) fn with_entry_debug_selectors(
        mut self,
        entry_debug_selectors: FxHashMap<usize, SharedString>,
    ) -> Self {
        self.entry_debug_selectors = entry_debug_selectors;
        self
    }
}

/// The model flattened into the rows a menu actually shows: top-level items
/// with the children of open submenus spliced in beneath their parent.
/// Selection indices (`context_menu_selected_ix`) refer to positions here,
/// because opening or closing a submenu changes which rows exist.
#[derive(Clone)]
pub(in crate::view) struct ContextMenuRows {
    rows: Vec<(ContextMenuItem, u8)>,
}

impl ContextMenuRows {
    pub(in crate::view) fn from_model(
        model: &ContextMenuModel,
        open_submenus: &FxHashSet<SharedString>,
    ) -> Self {
        let mut rows = Vec::with_capacity(model.items.len());
        fn flatten_into(
            target: &mut Vec<(ContextMenuItem, u8)>,
            items: Vec<ContextMenuItem>,
            depth: u8,
            open_submenus: &FxHashSet<SharedString>,
        ) {
            for item in items {
                match item {
                    ContextMenuItem::Submenu {
                        id,
                        label,
                        icon,
                        children,
                    } => {
                        let is_open = open_submenus.contains(&id);
                        // The pushed row keeps no children: rendering only
                        // needs the row itself, and the open set decides
                        // whether the children were spliced in below.
                        target.push((
                            ContextMenuItem::Submenu {
                                id,
                                label,
                                icon,
                                children: Vec::new(),
                            },
                            depth,
                        ));
                        if is_open {
                            flatten_into(target, children, depth + 1, open_submenus);
                        }
                    }
                    other => target.push((other, depth)),
                }
            }
        }
        flatten_into(&mut rows, model.items.clone(), 0, open_submenus);
        Self { rows }
    }

    pub(in crate::view) fn get(&self, ix: usize) -> Option<&(ContextMenuItem, u8)> {
        self.rows.get(ix)
    }

    pub(in crate::view) fn into_iter(self) -> impl Iterator<Item = (ContextMenuItem, u8)> {
        self.rows.into_iter()
    }

    pub(in crate::view) fn iter(&self) -> impl Iterator<Item = &(ContextMenuItem, u8)> + '_ {
        self.rows.iter()
    }

    pub(in crate::view) fn is_selectable(&self, ix: usize) -> bool {
        match self.rows.get(ix) {
            Some((ContextMenuItem::Entry { disabled, .. }, _)) => !*disabled,
            // A submenu row toggles its children; it is always actionable.
            Some((ContextMenuItem::Submenu { .. }, _)) => true,
            _ => false,
        }
    }

    pub(in crate::view) fn first_selectable(&self) -> Option<usize> {
        (0..self.rows.len()).find(|&ix| self.is_selectable(ix))
    }

    pub(in crate::view) fn last_selectable(&self) -> Option<usize> {
        (0..self.rows.len())
            .rev()
            .find(|&ix| self.is_selectable(ix))
    }

    pub(in crate::view) fn next_selectable(
        &self,
        from: Option<usize>,
        dir: isize,
    ) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let Some(mut ix) = from else {
            return if dir >= 0 {
                self.first_selectable()
            } else {
                self.last_selectable()
            };
        };

        let n = self.rows.len() as isize;
        for _ in 0..self.rows.len() {
            ix = ((ix as isize + dir).rem_euclid(n)) as usize;
            if self.is_selectable(ix) {
                return Some(ix);
            }
        }
        None
    }
}
