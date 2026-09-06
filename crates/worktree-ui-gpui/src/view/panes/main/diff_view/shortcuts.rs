//! Diff pane keyboard dispatch: the focus carve-outs stay handwritten, and
//! everything behind them lives in two ordered shortcut tables scanned by
//! `run_diff_shortcut_table`.
//!
//! Table discipline: each row is (chord, guard, action) — the key/modifier
//! pattern, the view-state predicate that arms the row, and the effect that
//! consumes the keystroke. Rows are consulted in declaration order; the scan
//! stops at the first action that reports the keystroke handled. This is
//! exactly the semantics of the hand-rolled `if !handled && ...` chain the
//! tables replaced: row order is the old block order, every guard condition
//! kept its predicate and its priority, and an action returning `false`
//! falls through to the next row exactly like a block that left `handled`
//! unset.
use super::*;

type DiffShortcutChord = fn(&str, gpui::Modifiers) -> bool;
type DiffShortcutGuard =
    fn(&mut MainPaneView, &mut Window, &mut gpui::Context<MainPaneView>) -> bool;
type DiffShortcutAction = fn(
    &mut MainPaneView,
    &str,
    gpui::Modifiers,
    &mut Window,
    &mut gpui::Context<MainPaneView>,
) -> bool;

/// One row of a diff shortcut table; see the module docs for the scan
/// semantics.
struct DiffShortcutEntry {
    chord: DiffShortcutChord,
    guard: Option<DiffShortcutGuard>,
    action: DiffShortcutAction,
}

/// Ordered scan behind the old `!handled` chain: run the first row whose
/// chord matches and guard passes; keep scanning while actions report the
/// keystroke unhandled.
fn run_diff_shortcut_table(
    this: &mut MainPaneView,
    table: &[DiffShortcutEntry],
    key: &str,
    mods: gpui::Modifiers,
    window: &mut Window,
    cx: &mut gpui::Context<MainPaneView>,
) -> bool {
    for entry in table {
        if !(entry.chord)(key, mods) {
            continue;
        }
        if let Some(guard) = entry.guard
            && !guard(this, window, cx)
        {
            continue;
        }
        if (entry.action)(this, key, mods, window, cx) {
            return true;
        }
    }
    false
}

/// Chords answered before the file-preview gate: the escape chain, search
/// opening and match stepping, staging, file navigation, the working-tree and
/// any-target Ctrl/Cmd chords, the Alt+E editor toggle, and the two chords
/// the file preview itself answers.
static DIFF_SHORTCUTS_BEFORE_FILE_PREVIEW: &[DiffShortcutEntry] = &[
    // kdiff3 manual diff help: Escape abandons pending alignment marks
    // before reaching the resolver's other escape behaviors, so a
    // mis-marked line does not cost the user their selection or view.
    DiffShortcutEntry {
        chord: |key, mods| {
            key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function
        },
        guard: None,
        action: |this, _key, _mods, _window, cx| this.conflict_resolver_clear_alignment_marks(cx),
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function
        },
        guard: Some(|this, _window, _cx| this.diff_search_active),
        action: |this, _key, _mods, window, cx| {
            this.deactivate_diff_search(window, cx);
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function
        },
        guard: Some(|this, _window, _cx| {
            this.is_inline_submodule_diff_active() && this.active_repo_id().is_some()
        }),
        action: |this, _key, _mods, _window, _cx| {
            if let Some(repo_id) = this.active_repo_id() {
                this.store
                    .dispatch(Msg::CloseInlineSubmoduleDiff { repo_id });
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function
        },
        guard: Some(|this, _window, _cx| this.active_repo_id().is_some()),
        action: |this, _key, _mods, _window, cx| {
            if let Some(repo_id) = this.active_repo_id() {
                this.clear_status_multi_selection(repo_id, cx);
                this.clear_diff_selection_or_exit(repo_id, cx);
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| mods.secondary() && mods.number_of_modifiers() == 1 && key == "f",
        guard: None,
        action: |this, _key, _mods, window, cx| this.open_search_for_active_view(window, cx),
    },
    // Shift+F2/F3 step between *unresolved* conflicts — the resolved ones
    // are skipped, which is what separates this from plain F2/F3.
    //
    // It sits ahead of the diff-search row below deliberately: this is a
    // distinct chord, so letting it become "previous/next search match"
    // whenever the search box happens to be open would be surprising. The
    // resolver guard keeps that scoped — outside the conflict resolver
    // Shift+F2/F3 falls through and means exactly what it always did.
    DiffShortcutEntry {
        chord: |key, mods| {
            matches!(key, "f2" | "f3")
                && mods.shift
                && !mods.control
                && !mods.alt
                && !mods.platform
                && !mods.function
        },
        guard: Some(|this, _window, _cx| {
            this.is_conflict_resolver_active() && !this.conflict_resolver.nav_targets.is_empty()
        }),
        action: |this, key, _mods, _window, cx| {
            if key == "f2" {
                this.conflict_jump_prev_unresolved(cx);
            } else {
                this.conflict_jump_next_unresolved(cx);
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            matches!(key, "f2" | "f3")
                && !mods.control
                && !mods.alt
                && !mods.platform
                && !mods.function
        },
        guard: Some(|this, _window, _cx| this.diff_search_active),
        action: |this, key, _mods, _window, _cx| {
            if key == "f2" {
                this.diff_search_prev_match();
            } else {
                this.diff_search_next_match();
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            key == "space" && !mods.control && !mods.alt && !mods.platform && !mods.function
        },
        guard: Some(|this, window, cx| {
            !this.is_inline_submodule_diff_active()
                && !this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && !this
                    .diff_search_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && this.active_repo_id().is_some()
                && this.active_repo().is_some_and(|repo| {
                    matches!(
                        repo.diff_state.diff_target,
                        Some(DiffTarget::WorkingTree { .. })
                    )
                })
        }),
        action: MainPaneView::diff_shortcut_space_stage_or_unstage,
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            (key == "f1" || key == "f4")
                && !mods.control
                && !mods.alt
                && !mods.platform
                && !mods.function
        },
        guard: Some(|this, _window, _cx| this.active_repo_id().is_some()),
        action: |this, key, _mods, window, cx| {
            let direction = if key == "f1" { -1 } else { 1 };
            if let Some(repo_id) = this.active_repo_id() {
                this.try_select_adjacent_diff_file(repo_id, direction, window, cx)
            } else {
                false
            }
        },
    },
    DiffShortcutEntry {
        chord: |_key, mods| (mods.control || mods.platform) && !mods.alt && !mods.function,
        guard: Some(|this, window, cx| {
            !this.is_inline_submodule_diff_active()
                && !this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && !this
                    .diff_search_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && this.active_repo_id().is_some()
                && this.active_repo().is_some_and(|repo| {
                    matches!(
                        repo.diff_state.diff_target,
                        Some(DiffTarget::WorkingTree { .. })
                    )
                })
        }),
        action: MainPaneView::diff_shortcut_working_tree_chord,
    },
    DiffShortcutEntry {
        chord: |_key, mods| (mods.control || mods.platform) && !mods.alt && !mods.function,
        guard: Some(|this, window, cx| {
            !this.is_inline_submodule_diff_active()
                && !this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && !this
                    .diff_search_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && this.active_repo_id().is_some()
                && this
                    .active_repo()
                    .is_some_and(|repo| repo.diff_state.diff_target.is_some())
        }),
        action: MainPaneView::diff_shortcut_any_target_ctrl_chord,
    },
    // Ahead of the file-preview gate below, which would otherwise swallow it:
    // the toggle has to work from the content view as well as from a diff.
    // Behind the focused-editor carve-outs, so it never competes with what
    // the buffer is composing.
    // Not while a text field owns the keyboard: on macOS Option+E is the
    // acute-accent dead key, and the search and raw-diff inputs compose with
    // it exactly as the buffer does. The Ctrl/Cmd rows exclude the same two
    // inputs for the same reason.
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt
                && !mods.control
                && !mods.platform
                && !mods.function
                && !mods.shift
                && key == "e"
        },
        guard: Some(|this, window, cx| {
            let text_input_focused = this
                .diff_search_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
                || this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window);
            !text_input_focused
                && !this.is_conflict_resolver_active()
                && !this.is_markdown_preview_active()
                && this.can_edit_current_target()
        }),
        action: |this, _key, _mods, window, cx| {
            this.toggle_file_editor(window, cx);
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && !mods.shift
                && key == "c"
        },
        guard: Some(|this, window, cx| {
            this.is_file_preview_active()
                && !this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && this.diff_text_has_selection()
        }),
        action: |this, _key, _mods, _window, cx| {
            this.copy_selected_diff_text_to_clipboard(cx);
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            (mods.control || mods.platform) && !mods.alt && !mods.function && key == "a"
        },
        guard: Some(|this, window, cx| {
            this.is_file_preview_active()
                && !this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
        }),
        action: |this, _key, _mods, _window, _cx| {
            this.select_all_diff_text();
            true
        },
    },
];

/// Chords answered only outside the file preview: view-mode and navigation
/// Alt chords, plain F2/F3/F7 stepping, and the conflict-resolver pick and
/// alignment chords.
static DIFF_SHORTCUTS_AFTER_FILE_PREVIEW: &[DiffShortcutEntry] = &[
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt
                && !mods.control
                && !mods.platform
                && !mods.function
                && matches!(key, "i" | "s")
        },
        guard: None,
        action: MainPaneView::diff_shortcut_alt_view_mode,
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt && !mods.control && !mods.platform && !mods.function && key == "w"
        },
        guard: Some(|this, _window, _cx| {
            !this.is_markdown_preview_active() && !this.is_conflict_rendered_preview_active()
        }),
        action: |this, _key, _mods, _window, cx| {
            this.toggle_reveal_whitespace_chars(cx);
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt && !mods.control && !mods.platform && !mods.function && key == "b"
        },
        guard: Some(|this, _window, _cx| {
            !this.is_markdown_preview_active() && !this.is_conflict_rendered_preview_active()
        }),
        action: |this, _key, _mods, _window, cx| {
            let next = !this.annotate_enabled;
            let root_view = this.root_view.clone();
            cx.defer(move |cx| {
                if let Some(root) = root_view.upgrade() {
                    root.update(cx, |root, cx| {
                        root.set_annotate_enabled(next, cx);
                    });
                }
            });
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt && !mods.control && !mods.platform && !mods.function && key == "up"
        },
        guard: None,
        action: |this, _key, _mods, _window, cx| this.navigate_prev_diff_change(cx),
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt && !mods.control && !mods.platform && !mods.function && key == "down"
        },
        guard: None,
        action: |this, _key, _mods, _window, cx| this.navigate_next_diff_change(cx),
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt && !mods.control && !mods.platform && !mods.function && key == "left"
        },
        guard: Some(|this, _window, _cx| this.active_repo_id().is_some()),
        action: |this, _key, _mods, _window, _cx| {
            if let Some(repo_id) = this.active_repo_id() {
                this.store.dispatch(Msg::GlobalNavBack { repo_id });
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            mods.alt && !mods.control && !mods.platform && !mods.function && key == "right"
        },
        guard: Some(|this, _window, _cx| this.active_repo_id().is_some()),
        action: |this, _key, _mods, _window, _cx| {
            if let Some(repo_id) = this.active_repo_id() {
                this.store.dispatch(Msg::GlobalNavForward { repo_id });
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            matches!(key, "f2" | "f3" | "f7")
                && !mods.control
                && !mods.alt
                && !mods.platform
                && !mods.function
        },
        guard: None,
        action: |this, key, mods, _window, cx| {
            match key {
                "f2" => {
                    let _ = this.navigate_prev_search_match_or_diff_change(cx);
                }
                "f3" => {
                    let _ = this.navigate_next_search_match_or_diff_change(cx);
                }
                "f7" if mods.shift => {
                    let _ = this.navigate_prev_diff_change(cx);
                }
                "f7" => {
                    let _ = this.navigate_next_diff_change(cx);
                }
                _ => {}
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |_key, mods| !mods.control && !mods.alt && !mods.platform && !mods.function,
        guard: Some(|this, window, cx| {
            this.is_conflict_resolver_active()
                && !this
                    .diff_raw_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && !this
                    .conflict_resolver_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                // Single-letter picks must not swallow characters typed into
                // the search box (e.g. "d" would otherwise pick Both).
                && !this
                    .diff_search_input
                    .read(cx)
                    .focus_handle()
                    .is_focused(window)
                && this.conflict_resolver_has_active_pick_target()
        }),
        action: MainPaneView::diff_shortcut_conflict_quick_pick,
    },
    // KDiff3-compatible Ctrl+Shift+1/2/3: choose A/B/C on every delta,
    // including blocks that were selected automatically and have no
    // conflict markers.
    DiffShortcutEntry {
        chord: |_key, mods| {
            (mods.control || mods.platform) && mods.shift && !mods.alt && !mods.function
        },
        guard: Some(|this, _window, _cx| this.is_conflict_resolver_active()),
        action: |this, key, _mods, _window, cx| {
            if let Some(choice) = conflict_resolver::conflict_ctrl_pick_choice_for_key(
                key,
                this.conflict_resolver.view_mode,
            ) {
                this.conflict_resolver_choose_everywhere(choice, cx);
                true
            } else {
                false
            }
        },
    },
    // section 30: kdiff3-compatible Ctrl+1/2/3 pick aliases. When the output
    // editor is focused these are handled by the carve-out at the top of
    // `handle_diff_shortcut`; this row covers the case where focus is
    // elsewhere.
    DiffShortcutEntry {
        chord: |_key, mods| {
            (mods.control || mods.platform) && !mods.alt && !mods.function && !mods.shift
        },
        guard: Some(|this, _window, _cx| {
            this.is_conflict_resolver_active() && this.conflict_resolver_has_active_pick_target()
        }),
        action: |this, key, _mods, _window, cx| {
            if let Some(choice) = conflict_resolver::conflict_ctrl_pick_choice_for_key(
                key,
                this.conflict_resolver.view_mode,
            ) {
                this.conflict_resolver_pick_active_conflict(choice, cx);
                true
            } else {
                false
            }
        },
    },
    // kdiff3 manual diff help: Ctrl+Y pins the lines marked in the source
    // columns onto one another; Ctrl+Shift+Y drops every pin and returns
    // the file to its automatic alignment.
    DiffShortcutEntry {
        chord: |key, mods| {
            (mods.control || mods.platform) && !mods.alt && !mods.function && key == "y"
        },
        guard: Some(|this, _window, _cx| this.is_conflict_resolver_active()),
        action: |this, _key, mods, _window, cx| {
            if mods.shift {
                this.conflict_resolver_clear_manual_alignments(cx)
            } else {
                this.conflict_resolver_align_manually(cx)
            }
        },
    },
    // WorkTree resolver navigation: Ctrl+Home/End jump to the first/last
    // delta. (Previous/next *unresolved* conflict is Shift+F2/F3, handled
    // above — Ctrl+PgUp/PgDn belongs to the repository tabs.)
    DiffShortcutEntry {
        chord: |key, mods| {
            (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && !mods.shift
                && matches!(key, "home" | "end")
        },
        guard: Some(|this, _window, _cx| {
            this.is_conflict_resolver_active() && !this.conflict_resolver.nav_targets.is_empty()
        }),
        action: |this, key, _mods, _window, cx| {
            if key == "home" {
                this.conflict_jump_first(cx);
            } else {
                this.conflict_jump_last(cx);
            }
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && !mods.shift
                && key == "c"
        },
        guard: Some(|this, window, cx| {
            !this
                .diff_raw_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
                && this.diff_text_has_selection()
        }),
        action: |this, _key, _mods, _window, cx| {
            this.copy_selected_diff_text_to_clipboard(cx);
            true
        },
    },
    DiffShortcutEntry {
        chord: |key, mods| {
            (mods.control || mods.platform) && !mods.alt && !mods.function && key == "a"
        },
        guard: Some(|this, window, cx| {
            !this
                .diff_raw_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
        }),
        action: |this, _key, _mods, _window, _cx| {
            this.select_all_diff_text();
            true
        },
    },
];

impl MainPaneView {
    pub(crate) fn handle_diff_shortcut(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let key = keystroke.key.as_str();
        let mods = keystroke.modifiers;

        // While the editable buffer has focus every keystroke belongs to it, with
        // one exception: Ctrl/Cmd+S saves and returns to the originating view.
        // Outside the editor that chord stages the file, and both meanings can
        // coexist precisely because they are separated by focus.
        if self
            .file_editor_input
            .read(cx)
            .focus_handle()
            .is_focused(window)
        {
            if (mods.control || mods.platform)
                && !mods.alt
                && !mods.shift
                && !mods.function
                && key == "s"
            {
                self.save_file_editor_buffer_and_exit(window, cx);
                return true;
            }
            if key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function {
                self.toggle_file_editor(window, cx);
                return true;
            }
            // Deliberately *not* Alt+E: on macOS Option+E is the acute-accent
            // dead key, and swallowing it here would stop the buffer composing
            // `é`. Escape above is the way out from inside the editor; Alt+E
            // only enters it, from a view where nothing is being typed.
            return false;
        }

        // When the editable resolved-output pane is focused the user is typing
        // free text: every text-producing keystroke (space, a/b/c/d, etc.)
        // belongs to that editor, not to the diff/conflict shortcut table.
        // Letting them through here staged the conflict file on the first space
        // typed (StagePath → the file leaves Conflicted → the resolver closes
        // mid-edit). Three deliberate carve-outs:
        //   * Ctrl+1/2/3 pick aliases are chords with no text-input collision,
        //     so they stay live while editing (kdiff3 parity).
        //   * Shift+F2/F3 (previous/next unresolved conflict) likewise: an
        //     F-key produces no text and the editor binds only the unmodified
        //     `f2`/`f3`, so the chord is free here — and jumping to the next
        //     open conflict is exactly what you want *while* editing the
        //     merged result, which is why it is not left outside.
        //   * WorkTree's Ctrl+Home/End resolver bindings are intentionally NOT
        //     handled here, so the editor keeps them for cursor movement.
        if self
            .conflict_resolver_input
            .read(cx)
            .focus_handle()
            .is_focused(window)
        {
            if self.is_conflict_resolver_active()
                && (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && let Some(choice) = conflict_resolver::conflict_ctrl_pick_choice_for_key(
                    key,
                    self.conflict_resolver.view_mode,
                )
            {
                if mods.shift {
                    self.conflict_resolver_choose_everywhere(choice, cx);
                    return true;
                }
                if self.conflict_resolver_has_active_pick_target() {
                    self.conflict_resolver_pick_active_conflict(choice, cx);
                    return true;
                }
            }
            if self.is_conflict_resolver_active()
                && matches!(key, "f2" | "f3")
                && mods.shift
                && !mods.control
                && !mods.alt
                && !mods.platform
                && !mods.function
                && !self.conflict_resolver.nav_targets.is_empty()
            {
                if key == "f2" {
                    self.conflict_jump_prev_unresolved(cx);
                } else {
                    self.conflict_jump_next_unresolved(cx);
                }
                return true;
            }
            return false;
        }

        let handled = run_diff_shortcut_table(
            self,
            DIFF_SHORTCUTS_BEFORE_FILE_PREVIEW,
            key,
            mods,
            window,
            cx,
        );

        // The file preview answers only the two gated rows at the end of the
        // first table; everything behind it addresses a diff that is not on
        // screen.
        if self.is_file_preview_active() {
            return handled;
        }

        if !handled {
            return run_diff_shortcut_table(
                self,
                DIFF_SHORTCUTS_AFTER_FILE_PREVIEW,
                key,
                mods,
                window,
                cx,
            );
        }

        handled
    }

    /// Space on a working-tree diff: stage or unstage the shown file — or the
    /// multi-file status selection, which wins over it — and advance inside
    /// the section.
    fn diff_shortcut_space_stage_or_unstage(
        &mut self,
        _key: &str,
        _mods: gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(repo_id) = self.active_repo_id() else {
            return false;
        };
        let Some(repo) = self.active_repo() else {
            return false;
        };
        let Some(diff_target) = repo.diff_state.diff_target.clone() else {
            return false;
        };
        let DiffTarget::WorkingTree { path, area } = &diff_target else {
            return false;
        };
        let path = path.clone();
        let area = *area;
        let change_tracking_view = self.active_change_tracking_view(cx);
        let next_path_in_section = status_nav::status_navigation_context_for_repo(
            repo,
            &diff_target,
            change_tracking_view,
        )
        .and_then(|navigation| navigation.next_or_prev_path());
        let status_ready = repo.status_entries_for_area(area).is_some();

        // A multi-file status selection wins over the single shown file, so
        // the shortcut matches what the status row button and context menu
        // already do with the same selection.
        if let Some(paths) = self.status_selection_for_shortcut(repo_id, area, &path, cx) {
            if self.confirm_stage_conflict_markers(repo_id, area, paths.clone(), true, window, cx) {
                return true;
            }
            self.clear_status_selection_for_shortcut(repo_id, cx);
            self.stage_or_unstage_status_paths(repo_id, area, paths);
            self.rebuild_diff_cache(cx);
            return true;
        }

        if self.confirm_stage_conflict_markers(repo_id, area, vec![path.clone()], false, window, cx)
        {
            return true;
        }

        match (status_ready, area) {
            (true, DiffArea::Unstaged) => {
                self.store.dispatch(Msg::StagePath {
                    repo_id,
                    path: path.clone(),
                });
                if let Some(next_path) = next_path_in_section {
                    self.store.dispatch(Msg::SelectDiff {
                        repo_id,
                        target: DiffTarget::WorkingTree {
                            path: next_path,
                            area: DiffArea::Unstaged,
                        },
                    });
                } else {
                    self.clear_diff_selection_or_exit(repo_id, cx);
                }
            }
            (true, DiffArea::Staged) => {
                self.store.dispatch(Msg::UnstagePath {
                    repo_id,
                    path: path.clone(),
                });
                if let Some(next_path) = next_path_in_section {
                    self.store.dispatch(Msg::SelectDiff {
                        repo_id,
                        target: DiffTarget::WorkingTree {
                            path: next_path,
                            area: DiffArea::Staged,
                        },
                    });
                } else {
                    self.clear_diff_selection_or_exit(repo_id, cx);
                }
            }
            (false, DiffArea::Unstaged) => {
                self.store.dispatch(Msg::StagePath {
                    repo_id,
                    path: path.clone(),
                });
            }
            (false, DiffArea::Staged) => {
                self.store.dispatch(Msg::UnstagePath {
                    repo_id,
                    path: path.clone(),
                });
            }
        }
        self.rebuild_diff_cache(cx);
        true
    }

    /// The Ctrl/Cmd chords that address the shown working-tree file: stage,
    /// unstage, discard, history, external editor, copy path.
    fn diff_shortcut_working_tree_chord(
        &mut self,
        key: &str,
        mods: gpui::Modifiers,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(repo_id) = self.active_repo_id() else {
            return false;
        };
        let Some(repo) = self.active_repo() else {
            return false;
        };
        let Some(diff_target) = repo.diff_state.diff_target.clone() else {
            return false;
        };
        let DiffTarget::WorkingTree { path, area } = &diff_target else {
            return false;
        };
        let path = path.clone();
        let area = *area;
        let status_ready = repo.status_entries_for_area(area).is_some();

        let mut handled = false;
        match key {
            "s" if area == DiffArea::Unstaged && !mods.shift => {
                let change_tracking_view = self.active_change_tracking_view(cx);
                let next_path_in_section = status_nav::status_navigation_context_for_repo(
                    repo,
                    &diff_target,
                    change_tracking_view,
                )
                .and_then(|navigation| navigation.next_or_prev_path());

                // A multi-file status selection wins over the single shown
                // file, matching the status row button and context menu.
                // Resolved before confirming, or the dialog would describe —
                // and then stage — only the shown file out of the selection.
                if let Some(paths) = self.status_selection_for_shortcut(repo_id, area, &path, cx) {
                    if self.confirm_stage_conflict_markers(
                        repo_id,
                        area,
                        paths.clone(),
                        true,
                        window,
                        cx,
                    ) {
                        return true;
                    }
                    self.clear_status_selection_for_shortcut(repo_id, cx);
                    self.stage_or_unstage_status_paths(repo_id, area, paths);
                    self.rebuild_diff_cache(cx);
                    return true;
                }

                if self.confirm_stage_conflict_markers(
                    repo_id,
                    area,
                    vec![path.clone()],
                    false,
                    window,
                    cx,
                ) {
                    return true;
                }

                if status_ready {
                    self.store.dispatch(Msg::StagePath {
                        repo_id,
                        path: path.clone(),
                    });
                    if let Some(next_path) = next_path_in_section {
                        self.store.dispatch(Msg::SelectDiff {
                            repo_id,
                            target: DiffTarget::WorkingTree {
                                path: next_path,
                                area: DiffArea::Unstaged,
                            },
                        });
                    } else {
                        self.clear_diff_selection_or_exit(repo_id, cx);
                    }
                } else {
                    self.store.dispatch(Msg::StagePath {
                        repo_id,
                        path: path.clone(),
                    });
                }
                self.rebuild_diff_cache(cx);
                handled = true;
            }
            "u" if area == DiffArea::Staged && !mods.shift => {
                let change_tracking_view = self.active_change_tracking_view(cx);
                let next_path_in_section = status_nav::status_navigation_context_for_repo(
                    repo,
                    &diff_target,
                    change_tracking_view,
                )
                .and_then(|navigation| navigation.next_or_prev_path());

                // A multi-file status selection wins over the single shown
                // file, matching the status row button and context menu.
                if let Some(paths) = self.status_selection_for_shortcut(repo_id, area, &path, cx) {
                    if self.confirm_stage_conflict_markers(
                        repo_id,
                        area,
                        paths.clone(),
                        true,
                        window,
                        cx,
                    ) {
                        return true;
                    }
                    self.clear_status_selection_for_shortcut(repo_id, cx);
                    self.stage_or_unstage_status_paths(repo_id, area, paths);
                    self.rebuild_diff_cache(cx);
                    return true;
                }

                if status_ready {
                    self.store.dispatch(Msg::UnstagePath {
                        repo_id,
                        path: path.clone(),
                    });
                    if let Some(next_path) = next_path_in_section {
                        self.store.dispatch(Msg::SelectDiff {
                            repo_id,
                            target: DiffTarget::WorkingTree {
                                path: next_path,
                                area: DiffArea::Staged,
                            },
                        });
                    } else {
                        self.clear_diff_selection_or_exit(repo_id, cx);
                    }
                } else {
                    self.store.dispatch(Msg::UnstagePath {
                        repo_id,
                        path: path.clone(),
                    });
                }
                self.rebuild_diff_cache(cx);
                handled = true;
            }
            "d" if !mods.shift => {
                let bounds = window.window_bounds().get_bounds();
                let anchor = point(
                    (bounds.size.width * 0.5).max(px(64.0)),
                    (bounds.size.height * 0.25).max(px(24.0)),
                );
                self.open_popover_at(
                    PopoverKind::DiscardChangesConfirm {
                        repo_id,
                        area,
                        path: Some(path),
                    },
                    anchor,
                    window,
                    cx,
                );
                handled = true;
            }
            "h" if !mods.shift => {
                let bounds = window.window_bounds().get_bounds();
                let anchor = point(
                    (bounds.size.width * 0.5).max(px(64.0)),
                    (bounds.size.height * 0.25).max(px(24.0)),
                );
                self.open_popover_at(
                    PopoverKind::FileHistory {
                        repo_id,
                        path: path.clone(),
                        is_dir: false,
                    },
                    anchor,
                    window,
                    cx,
                );
                handled = true;
            }
            "e" if !mods.shift && crate::external_editor::configured_setting().is_some() => {
                let full_path = repo.spec.workdir.join(&path);
                let root_view = self.root_view.clone();
                let p = full_path;
                cx.defer(move |cx| {
                    if let Some(root) = root_view.upgrade() {
                        root.update(cx, |root, cx| {
                            root.open_path_in_external_code_editor(p, cx);
                        });
                    }
                });
                handled = true;
            }
            "c" if mods.shift => {
                crate::clipboard::write_text(
                    cx,
                    path.display().to_string(),
                    crate::clipboard::CopySource::FilePathShortcut,
                );
                handled = true;
            }
            _ => {}
        }
        handled
    }

    /// Ctrl/Cmd+E on a commit or range diff: open the shown file in the
    /// external editor. (The working-tree variant lives one row earlier, in
    /// `diff_shortcut_working_tree_chord`.)
    fn diff_shortcut_any_target_ctrl_chord(
        &mut self,
        key: &str,
        mods: gpui::Modifiers,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(repo) = self.active_repo() else {
            return false;
        };
        let Some(diff_target) = repo.diff_state.diff_target.clone() else {
            return false;
        };
        let path = match &diff_target {
            DiffTarget::WorkingTree { path, .. } => Some(path.clone()),
            DiffTarget::Commit { path, .. } => path.clone(),
            DiffTarget::CommitRange { path, .. } => path.clone(),
        };
        let mut handled = false;
        if let Some(path) = path {
            match key {
                "e" if !mods.shift && crate::external_editor::configured_setting().is_some() => {
                    let full_path = repo.spec.workdir.join(&path);
                    let root_view = self.root_view.clone();
                    let p = full_path;
                    cx.defer(move |cx| {
                        if let Some(root) = root_view.upgrade() {
                            root.update(cx, |root, cx| {
                                root.open_path_in_external_code_editor(p, cx);
                            });
                        }
                    });
                    handled = true;
                }
                _ => {}
            }
        }
        handled
    }

    /// Alt+I/Alt+S switch the diff between its inline and split layouts.
    ///
    /// The markdown diff preview renders both layouts (see
    /// `render_markdown_diff_preview`), so these switch it just like the text
    /// diff. The single-pane file preview has no old/new pair to split, so it
    /// stays excluded.
    fn diff_shortcut_alt_view_mode(
        &mut self,
        key: &str,
        _mods: gpui::Modifiers,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.is_conflict_resolver_active() {
            return false;
        }
        if self.active_conflict_target().is_some() {
            self.set_diff_view_mode(DiffViewMode::Split, cx);
            let root_view = self.root_view.clone();
            cx.defer(move |cx| {
                if let Some(root) = root_view.upgrade() {
                    root.update(cx, |root, cx| {
                        root.set_diff_view_mode(DiffViewMode::Split, cx);
                    });
                }
            });
            return true;
        }
        if !self.is_file_preview_active() {
            let new_mode = if key == "i" {
                DiffViewMode::Inline
            } else {
                DiffViewMode::Split
            };
            self.set_diff_view_mode(new_mode, cx);
            let root_view = self.root_view.clone();
            let mode = new_mode;
            cx.defer(move |cx| {
                if let Some(root) = root_view.upgrade() {
                    root.update(cx, |root, cx| {
                        root.set_diff_view_mode(mode, cx);
                    });
                }
            });
            return true;
        }
        false
    }

    /// Unmodified keys while the resolver has an active pick target: the
    /// single-letter quick picks, plus U to un-resolve the active conflict.
    fn diff_shortcut_conflict_quick_pick(
        &mut self,
        key: &str,
        _mods: gpui::Modifiers,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if let Some(choice) = conflict_resolver::conflict_quick_pick_choice_for_key(
            key,
            self.conflict_resolver.view_mode,
        ) {
            self.conflict_resolver_pick_active_conflict(choice, cx);
            return true;
        }
        if key == "u" {
            // section 30: U un-resolves the active conflict (pick or auto-solve).
            self.conflict_resolver_unresolve_active_conflict(cx);
            return true;
        }
        false
    }
}
