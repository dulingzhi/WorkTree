//! The native Jujutsu flavor (#76/#77): when the active repository has a
//! `.jj` directory (surfaced by the git store's `RepoCapabilities::is_jj`),
//! the repo content area swaps from the git three-pane layout to
//! `JjRepoView`, backed by the jj store from `gitcomet_state::jj_store`.
//!
//! The flavor switch is intentionally thin: the tab bar, action bar,
//! settings window, shortcuts, i18n, and theme all stay shared because they
//! belong to `GitCometView` — only the content card swaps. The git store
//! keeps the repo open underneath (the P0/P1 compat path owns tabs and
//! recents); #80 retires that overlap once the jj panels cover browsing.
//!
//! `JjRepoView` itself is the describe-as-you-go workspace: the working
//! copy (@) is pinned above a change list (`change_list`), and the
//! description bar drives `jj describe` / `jj new` on @. Everything here
//! compiles only with `--features jj`; git-only builds never see it.

use super::*;

mod change_list;
mod panels;

use change_list::{change_row_vms, render_change_row};
use gitcomet_jj_core::ChangeId;
use gitcomet_state::jj_store::{JjAppState, JjMsg, JjStore};
use gitcomet_state::msg::StoreEvent;
use panels::{bookmark_row_vms, op_row_vms, render_bookmark_row, render_op_row};

use super::splash::{CONTENT_CARD_BOTTOM_MARGIN_PX, CONTENT_CARD_GAP_PX};

/// The repo content area for a Jujutsu repository. Owns no store state —
/// it renders a snapshot of the jj store and forwards gestures as
/// messages, exactly like the git panes.
pub(crate) struct JjRepoView {
    store: Arc<JjStore>,
    state: Arc<JjAppState>,
    theme: AppTheme,
    /// The describe bar's text input. Enter submits `jj describe` on @.
    describe_input: Entity<components::TextInput>,
    _describe_input_subscription: gpui::Subscription,
    /// Which @ change the input text was last synced from. The input is
    /// only rewritten when @ moves to a new change — never while the user
    /// types into the same @.
    describe_synced_change: Option<ChangeId>,
    /// Selected list row, keyed by `ChangeId` (stable across rewrites,
    /// unlike commit ids) so paging never reselects a different change.
    selected_change: Option<ChangeId>,
    /// The revset filter bar's input. Enter applies it as the list's
    /// revset — `SetRevset` reloads from the top under a fresh epoch.
    revset_input: Entity<components::TextInput>,
    _revset_input_subscription: gpui::Subscription,
    /// The revset the input was last synced from (the revset twin of
    /// `describe_synced_change`): refreshes carrying the same revset
    /// never rewrite what the user is typing.
    revset_synced: Option<String>,
    /// Keyboard focus for the change list. ↑/↓ step the selection, Escape
    /// clears it; row clicks focus this handle so arrows work right after
    /// a click.
    list_focus: gpui::FocusHandle,
    /// The bookmark creation bar's input. Enter creates a bookmark from
    /// the name, targeting the selected change (or @).
    bookmark_input: Entity<components::TextInput>,
    _bookmark_input_subscription: gpui::Subscription,
    _poller: gpui::Task<()>,
    /// Held (not drained) under the test runtime, mirroring `Poller`.
    _held_events: Option<smol::channel::Receiver<StoreEvent>>,
}

impl JjRepoView {
    pub(super) fn new(
        store: Arc<JjStore>,
        events: smol::channel::Receiver<StoreEvent>,
        theme: AppTheme,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let state = store.snapshot();
        let describe_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("jj.describe.placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let _describe_input_subscription = cx.observe(&describe_input, |this, input, cx| {
            let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
            if enter_pressed {
                this.submit_describe(cx);
            }
        });
        let revset_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("jj.revset.placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let _revset_input_subscription = cx.observe(&revset_input, |this, input, cx| {
            let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
            if enter_pressed {
                this.apply_revset(cx);
            }
        });
        let bookmark_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("jj.bookmarks.placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let _bookmark_input_subscription = cx.observe(&bookmark_input, |this, input, cx| {
            let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
            if enter_pressed {
                this.create_bookmark(cx);
            }
        });
        let list_focus = cx.focus_handle();

        // The pane can outlive a mid-session creation (the pane is created
        // lazily on the render path), so the store may already hold a repo
        // — sync both bars from it before first render.
        let sync_inputs = |this: &mut Self, cx: &mut gpui::Context<Self>| {
            this.sync_describe_input(cx);
            this.sync_revset_input(cx);
        };

        let runtime = crate::ui_runtime::current();
        if !runtime.uses_live_store_poller() {
            let mut this = Self {
                store,
                state,
                theme,
                describe_input,
                _describe_input_subscription,
                describe_synced_change: None,
                selected_change: None,
                revset_input,
                _revset_input_subscription,
                revset_synced: None,
                list_focus,
                bookmark_input,
                _bookmark_input_subscription,
                _poller: gpui::Task::ready(()),
                _held_events: Some(events),
            };
            sync_inputs(&mut this, cx);
            return this;
        }

        let poller_store = Arc::clone(&store);
        let _poller = cx.spawn(async move |weak, cx| {
            loop {
                if events.recv().await.is_err() {
                    break;
                }
                while events.try_recv().is_ok() {}

                // Keep the store read off the UI thread, like `Poller`.
                let snapshot = if runtime.uses_background_compute() {
                    let store = Arc::clone(&poller_store);
                    smol::unblock(move || store.snapshot()).await
                } else {
                    poller_store.snapshot()
                };

                let _ = weak.update(cx, |this, cx| {
                    this.state = snapshot;
                    this.sync_describe_input(cx);
                    this.sync_revset_input(cx);
                    cx.notify();
                });
            }
        });

        let mut this = Self {
            store,
            state,
            theme,
            describe_input,
            _describe_input_subscription,
            describe_synced_change: None,
            selected_change: None,
            revset_input,
            _revset_input_subscription,
            revset_synced: None,
            list_focus,
            bookmark_input,
            _bookmark_input_subscription,
            _poller,
            _held_events: None,
        };
        sync_inputs(&mut this, cx);
        this
    }

    pub(super) fn set_theme(&mut self, theme: AppTheme, cx: &mut gpui::Context<Self>) {
        self.theme = theme;
        self.describe_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.revset_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.bookmark_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        cx.notify();
    }

    /// The working-copy change of the active repo, if loaded.
    fn working_copy(&self) -> Option<gitcomet_jj_core::JjChange> {
        self.state.active_repo().and_then(|repo| {
            repo.working_copy
                .clone()
                .or_else(|| repo.changes.iter().find(|c| c.is_working_copy).cloned())
        })
    }

    /// Rewrite the describe input only when @ moved to a different change
    /// (after describe/new/restore); while @ stays the same, typing is
    /// never clobbered.
    fn sync_describe_input(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(working_copy) = self.working_copy() else {
            return;
        };
        if self.describe_synced_change.as_ref() == Some(&working_copy.change_id) {
            return;
        }
        self.describe_synced_change = Some(working_copy.change_id.clone());
        let text = working_copy.description.clone();
        self.describe_input
            .update(cx, |input, cx| input.set_text(text, cx));
    }

    fn describe_message(&self, cx: &gpui::Context<Self>) -> Option<String> {
        let message = self.describe_input.read(cx).text().trim().to_string();
        (!message.is_empty()).then_some(message)
    }

    /// Enter / Describe button: set @'s description (`jj describe`).
    fn submit_describe(&mut self, cx: &mut gpui::Context<Self>) {
        self.describe_or_new(false, cx);
    }

    /// "New change" button: describe @ and open a fresh change on top
    /// (`jj new -m <message>`).
    fn start_new_change(&mut self, cx: &mut gpui::Context<Self>) {
        self.describe_or_new(true, cx);
    }

    fn describe_or_new(&mut self, new_change: bool, cx: &mut gpui::Context<Self>) {
        let Some(repo) = self.state.active_repo() else {
            return;
        };
        if repo.pending_command.is_some() {
            return;
        }
        let repo_id = repo.id;
        let message = self.describe_message(cx);
        if !new_change && message.is_none() {
            // Describing @ with an empty bar would erase its description;
            // treat it as a no-op.
            return;
        }
        if new_change {
            self.store.dispatch(JjMsg::NewChange { repo_id, message });
        } else if let Some(working_copy) = self.working_copy() {
            self.store.dispatch(JjMsg::DescribeChange {
                repo_id,
                change: working_copy.change_id,
                message: message.unwrap_or_default(),
            });
        }
    }

    fn load_more(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(repo) = self.state.active_repo() {
            self.store.dispatch(JjMsg::LoadMoreLog { repo_id: repo.id });
        }
        cx.notify();
    }

    /// Enter on the revset bar: reload the list under the bar's revset.
    /// Empty means `all()` — the reducer trims and bumps the epoch so
    /// in-flight pages for the old revset are dropped.
    fn apply_revset(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(repo) = self.state.active_repo() else {
            return;
        };
        let revset = self.revset_input.read(cx).text().trim().to_string();
        self.store.dispatch(JjMsg::SetRevset {
            repo_id: repo.id,
            revset: revset.clone(),
        });
        // Own the new value so the reload's refresh doesn't rewrite the bar.
        self.revset_synced = Some(revset);
    }

    /// Rewrite the revset bar only when the repo's revset changed outside
    /// the bar (initial load); refreshes under the same revset never
    /// touch what the user is typing.
    fn sync_revset_input(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(repo) = self.state.active_repo() else {
            return;
        };
        let revset = repo.log_revset.clone();
        if self.revset_synced.as_ref() == Some(&revset) {
            return;
        }
        self.revset_synced = Some(revset.clone());
        self.revset_input
            .update(cx, |input, cx| input.set_text(revset, cx));
    }

    fn handle_list_key_down(&mut self, event: &gpui::KeyDownEvent, cx: &mut gpui::Context<Self>) {
        match event.keystroke.key.as_ref() {
            "up" => self.move_selection(-1, cx),
            "down" => self.move_selection(1, cx),
            "escape" => {
                if self.selected_change.take().is_some() {
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    /// Step the selection `delta` rows through the rendered rows (which
    /// exclude @). Focus follows the click that selects a row, so this
    /// only runs while the list itself holds focus — inputs keep their
    /// keys.
    fn move_selection(&mut self, delta: i32, cx: &mut gpui::Context<Self>) {
        let Some(repo) = self.state.active_repo() else {
            return;
        };
        let ids: Vec<ChangeId> = repo
            .changes
            .iter()
            .filter(|change| !change.is_working_copy)
            .map(|change| change.change_id.clone())
            .collect();
        if let Some(next) = next_selected_change(&ids, self.selected_change.as_ref(), delta) {
            self.selected_change = Some(next);
            cx.notify();
        }
    }

    /// Enter / Create button: bookmark the selected change — or @ when
    /// nothing is selected — under the bar's name.
    fn create_bookmark(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(repo) = self.state.active_repo() else {
            return;
        };
        if repo.pending_command.is_some() {
            return;
        }
        let name = self.bookmark_input.read(cx).text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let Some(target) = self
            .selected_change
            .clone()
            .or_else(|| self.working_copy().map(|wc| wc.change_id))
        else {
            return;
        };
        self.store.dispatch(JjMsg::BookmarkCreate {
            repo_id: repo.id,
            name,
            target,
        });
        self.bookmark_input
            .update(cx, |input, cx| input.set_text("", cx));
        cx.notify();
    }

    fn delete_bookmark(&mut self, name: &str, cx: &mut gpui::Context<Self>) {
        if let Some(repo) = self.state.active_repo() {
            if repo.pending_command.is_none() {
                self.store.dispatch(JjMsg::BookmarkDelete {
                    repo_id: repo.id,
                    name: name.to_string(),
                });
            }
        }
        cx.notify();
    }

    /// Undo the latest operation (`jj op revert <latest>` records a new
    /// operation with the inverse effect, so undoing is itself undoable).
    fn undo_operation(&mut self, cx: &mut gpui::Context<Self>) {
        if let Some(repo) = self.state.active_repo() {
            if repo.pending_command.is_none() && !repo.ops.is_empty() {
                self.store.dispatch(JjMsg::OpUndo { repo_id: repo.id });
            }
        }
        cx.notify();
    }
}

impl Render for JjRepoView {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let repo = self.state.active_repo();
        let pending_command = repo.and_then(|repo| repo.pending_command);
        let now = std::time::SystemTime::now();

        let body: gpui::Div = match repo {
            None => div().child(crate::i18n::tr("jj.loading")),
            Some(repo) if repo.open_error.is_some() => div().p_3().child(
                div()
                    .rounded(px(theme.radii.panel))
                    .border_1()
                    .border_color(theme.colors.status.danger.border)
                    .bg(theme.colors.surface.raised)
                    .p_3()
                    .text_sm()
                    .text_color(theme.colors.foreground.primary)
                    .child(crate::i18n::t!(
                        "jj.open_failed",
                        error = repo.open_error.clone().unwrap_or_default()
                    )),
            ),
            Some(repo) => {
                let mut card = div().flex().flex_col().gap_3().p_3();

                if let Some(reason) = repo.watch_degraded.clone() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.status.warning.foreground)
                            .child(crate::i18n::t!("jj.status.degraded_watch", reason = reason)),
                    );
                }

                // The pinned @ workspace: change id, flags, describe bar.
                if let Some(wc) = self.working_copy() {
                    let busy = pending_command.is_some();
                    let mut flags = String::new();
                    if wc.conflicted {
                        flags.push_str(&crate::i18n::tr("jj.working_copy.conflicted"));
                    }
                    if wc.divergent {
                        if !flags.is_empty() {
                            flags.push_str(" · ");
                        }
                        flags.push_str(&crate::i18n::tr("jj.working_copy.divergent"));
                    }
                    let describe_button = components::Button::new(
                        "jj_describe_submit",
                        crate::i18n::tr("jj.describe.submit"),
                    )
                    .style(components::ButtonStyle::Filled)
                    .disabled(busy)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.submit_describe(cx);
                    });
                    let new_button = components::Button::new(
                        "jj_describe_new",
                        crate::i18n::tr("jj.describe.new_change"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .disabled(busy)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.start_new_change(cx);
                    });

                    card = card.child(
                        div()
                            .id("jj_working_copy_card")
                            .debug_selector(|| "jj_working_copy_card".to_string())
                            .rounded(px(theme.radii.panel))
                            .border_1()
                            .border_color(theme.colors.stroke.default)
                            .bg(theme.colors.surface.raised)
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme.colors.foreground.secondary)
                                            .child(crate::i18n::tr("jj.working_copy.title")),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.colors.foreground.emphasis)
                                            .child(wc.change_id.0.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.colors.foreground.secondary)
                                            .child(wc.commit_id.0.clone()),
                                    )
                                    .when(!flags.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.colors.status.warning.foreground)
                                                .child(flags),
                                        )
                                    })
                                    .when_some(pending_command, |d, operation| {
                                        d.child(
                                            div()
                                                .ml_auto()
                                                .flex_none()
                                                .text_xs()
                                                .text_color(theme.colors.foreground.secondary)
                                                .child(crate::i18n::t!(
                                                    "jj.describe.busy",
                                                    operation = operation
                                                )),
                                        )
                                    }),
                            )
                            .child(self.describe_input.clone())
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(describe_button)
                                    .child(new_button),
                            ),
                    );
                }

                // Status strip: conflicts embedded in @, and the most
                // recent failed command. The resolve flow lands in #80;
                // for now the paths are surfaced where describe happens.
                if !repo.conflicts.is_empty() {
                    let mut conflict_paths = div().flex().flex_col().gap_1();
                    for conflict in &repo.conflicts {
                        conflict_paths = conflict_paths.child(
                            div()
                                .text_sm()
                                .truncate()
                                .text_color(theme.colors.foreground.primary)
                                .child(conflict.path.clone()),
                        );
                    }
                    card = card.child(
                        div()
                            .id("jj_conflicts_card")
                            .debug_selector(|| "jj_conflicts_card".to_string())
                            .rounded(px(theme.radii.panel))
                            .border_1()
                            .border_color(theme.colors.status.warning.border)
                            .bg(theme.colors.surface.raised)
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.status.warning.foreground)
                                    .child(crate::i18n::t!(
                                        "jj.conflicts.count",
                                        count = repo.conflicts.len()
                                    )),
                            )
                            .child(conflict_paths),
                    );
                }
                if let Some((operation, error)) = repo.last_command_error.clone() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.status.danger.foreground)
                            .child(crate::i18n::t!(
                                "jj.status.last_command_failed",
                                operation = operation,
                                error = error
                            )),
                    );
                }

                // The change list (working copy pinned above, skipped here),
                // under a revset filter bar.
                let rows = change_row_vms(repo, now);
                let selected = self.selected_change.clone();
                let list_focus = self.list_focus.clone();

                let revset_bar = div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.revset_input.clone())
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::t!("jj.changes.count", count = rows.len())),
                    );

                let mut list = div().flex().flex_col();
                if repo.log_loading && rows.is_empty() {
                    list = list.child(
                        div()
                            .p_3()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr("jj.loading")),
                    );
                }
                for row in &rows {
                    let is_selected = selected.as_ref() == Some(&row.change_id);
                    let row_change = row.change_id.clone();
                    list = list.child(render_change_row(
                        row,
                        is_selected,
                        theme,
                        cx.listener(move |this, _e, window, cx| {
                            this.selected_change = Some(row_change.clone());
                            // Clicking a row also puts the list in keyboard
                            // focus so ↑/↓ work immediately after.
                            window.focus(&this.list_focus, cx);
                            cx.notify();
                        }),
                    ));
                }
                if !repo.log_loading && rows.is_empty() {
                    list = list.child(
                        div()
                            .p_3()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr("jj.changes.empty")),
                    );
                }
                if let Some(error) = repo.log_error.clone() {
                    list = list.child(
                        div()
                            .p_2()
                            .text_xs()
                            .text_color(theme.colors.status.danger.foreground)
                            .child(error),
                    );
                }
                card = card.child(revset_bar).child(
                    div()
                        .id("jj_change_list")
                        .debug_selector(|| "jj_change_list".to_string())
                        .track_focus(&list_focus)
                        .key_context("JjChangeList")
                        .on_key_down(cx.listener(
                            |this, event: &gpui::KeyDownEvent, _window, cx| {
                                this.handle_list_key_down(event, cx);
                            },
                        ))
                        .rounded(px(theme.radii.panel))
                        .border_1()
                        .border_color(theme.colors.stroke.default)
                        .overflow_hidden()
                        .child(list),
                );

                if repo.next_cursor.is_some() {
                    card = card.child(
                        components::Button::new(
                            "jj_load_more",
                            crate::i18n::tr("jj.changes.load_more"),
                        )
                        .style(components::ButtonStyle::Outlined)
                        .disabled(repo.log_loading || pending_command.is_some())
                        .on_click(theme, cx, |this, _e, _w, cx| {
                            this.load_more(cx);
                        }),
                    );
                }

                // Bookmarks: a create bar (targeting the selection, or @)
                // and one row per bookmark; only local bookmarks carry a
                // delete affordance — remote refs belong to their remote.
                let busy = pending_command.is_some();
                let bookmark_rows = bookmark_row_vms(repo);
                let mut bookmark_list = div().flex().flex_col();
                for row in &bookmark_rows {
                    let name = row.name.clone();
                    let delete_button = row.is_local.then(|| {
                        components::Button::new(
                            SharedString::from(format!("jj_bookmark_delete_{}", row.display_name)),
                            crate::i18n::tr("jj.bookmarks.delete"),
                        )
                        .style(components::ButtonStyle::Transparent)
                        .disabled(busy)
                        .on_click(theme, cx, move |this, _e, _w, cx| {
                            this.delete_bookmark(&name, cx);
                        })
                        .into_any_element()
                    });
                    bookmark_list =
                        bookmark_list.child(render_bookmark_row(row, theme, delete_button));
                }
                if bookmark_rows.is_empty() {
                    bookmark_list = bookmark_list.child(
                        div()
                            .p_3()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr("jj.bookmarks.empty")),
                    );
                }
                let create_button = components::Button::new(
                    "jj_bookmark_create",
                    crate::i18n::tr("jj.bookmarks.create"),
                )
                .style(components::ButtonStyle::Filled)
                .disabled(busy)
                .on_click(theme, cx, |this, _e, _w, cx| {
                    this.create_bookmark(cx);
                });
                card = card.child(
                    div()
                        .id("jj_bookmarks_card")
                        .debug_selector(|| "jj_bookmarks_card".to_string())
                        .rounded(px(theme.radii.panel))
                        .border_1()
                        .border_color(theme.colors.stroke.default)
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .p_3()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.colors.foreground.secondary)
                                        .child(crate::i18n::tr("jj.bookmarks.title")),
                                )
                                .child(
                                    div()
                                        .ml_auto()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(theme.colors.foreground.secondary)
                                        .child(crate::i18n::t!(
                                            "jj.bookmarks.count",
                                            count = bookmark_rows.len()
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .pb_2()
                                .child(self.bookmark_input.clone())
                                .child(create_button),
                        )
                        .child(bookmark_list),
                );

                // Operation log: recent operations, newest first, with the
                // undo entry point in the header.
                let op_rows = op_row_vms(repo, now);
                let mut op_list = div().flex().flex_col();
                for row in &op_rows {
                    op_list = op_list.child(render_op_row(row, theme));
                }
                if op_rows.is_empty() {
                    op_list = op_list.child(
                        div()
                            .p_3()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr("jj.op_log.empty")),
                    );
                }
                let undo_button =
                    components::Button::new("jj_op_undo", crate::i18n::tr("jj.op_log.undo"))
                        .style(components::ButtonStyle::Danger)
                        .disabled(busy || op_rows.is_empty())
                        .on_click(theme, cx, |this, _e, _w, cx| {
                            this.undo_operation(cx);
                        });
                card = card.child(
                    div()
                        .id("jj_op_log_card")
                        .debug_selector(|| "jj_op_log_card".to_string())
                        .rounded(px(theme.radii.panel))
                        .border_1()
                        .border_color(theme.colors.stroke.default)
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .p_3()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.colors.foreground.secondary)
                                        .child(crate::i18n::tr("jj.op_log.title")),
                                )
                                .child(div().ml_auto().flex_none().child(undo_button)),
                        )
                        .child(op_list),
                );

                card
            }
        };

        div()
            .id("jj_repo_view")
            .debug_selector(|| "jj_repo_view".to_string())
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.colors.surface.canvas)
            .child(
                div()
                    .id("jj_repo_scroll")
                    .debug_selector(|| "jj_repo_scroll".to_string())
                    .flex()
                    .flex_col()
                    .w_full()
                    .h_full()
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body.flex_1()),
            )
    }
}

impl GitCometView {
    /// The jj flavor of the content area, or `None` when the git layout
    /// should render. Called from `center_content` after the splash checks.
    pub(super) fn jj_center_content(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        if !renders_full_chrome(self.view_mode) {
            return None;
        }
        if !self.jj_flavor_active() {
            return None;
        }
        let store = self.jj_store.clone()?;

        // The pane is view construction, so it happens here where a window
        // exists (the describe input needs one); the store and its event
        // receiver come from `sync_jj_flavor` on the state-apply path.
        let pane = match self.jj_pane.clone() {
            Some(pane) => pane,
            None => {
                let events = self.jj_events.take()?;
                let pane = cx
                    .new(|cx| JjRepoView::new(Arc::clone(&store), events, self.theme, window, cx));
                self.jj_pane = Some(pane.clone());
                cx.notify();
                pane
            }
        };

        let theme = self.theme;
        let content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(self.open_repo_panel(cx))
            .child(stable_cached_fixed_height_view(
                self.action_bar.clone(),
                action_bar_height(cx),
            ))
            .child(
                // Same card silhouette as the git layout: the jj pane owns
                // the whole card (no sidebar/details split yet).
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .min_w(px(0.0))
                    .flex()
                    .mb(px(CONTENT_CARD_BOTTOM_MARGIN_PX))
                    .mr(px(CONTENT_CARD_GAP_PX))
                    .rounded(px(theme.radii.panel))
                    .border_1()
                    .border_color(theme.colors.stroke.default)
                    .overflow_hidden()
                    .bg(theme.colors.surface.canvas)
                    .child(stable_cached_fill_view(pane)),
            );
        Some(content.into_any_element())
    }

    /// Whether the active repo renders the jj flavor: it must carry the
    /// `.jj` capability bit set on open.
    pub(super) fn jj_flavor_active(&self) -> bool {
        self.state.active_repo.is_some_and(|repo_id| {
            self.state
                .repos
                .iter()
                .any(|repo| repo.id == repo_id && repo.capabilities.is_jj)
        })
    }

    /// Keeps the jj store in step with the git store after every applied
    /// snapshot: opens/activates the active jj workdir, and mirrors repo
    /// closes so watchers stop. Pane creation happens on the render path
    /// (`jj_center_content`), where a window is available.
    pub(super) fn sync_jj_flavor(&mut self, cx: &mut gpui::Context<Self>) {
        let active_jj_workdir = self.state.active_repo.and_then(|repo_id| {
            self.state
                .repos
                .iter()
                .find(|repo| repo.id == repo_id && repo.capabilities.is_jj)
                .map(|repo| repo.spec.workdir.clone())
        });

        if active_jj_workdir.is_some() && self.jj_store.is_none() {
            let (store, events) = gitcomet_state::jj_store::JjStore::new(Arc::new(
                gitcomet_state::jj_store::CliJjBackend,
            ));
            self.jj_store = Some(Arc::new(store));
            self.jj_events = Some(events);
            cx.notify();
        }

        if let Some(store) = self.jj_store.as_ref() {
            if let Some(workdir) = active_jj_workdir {
                // The store dedups opens by workdir; activation follows the
                // git tab selection.
                match store
                    .snapshot()
                    .repos
                    .iter()
                    .find(|repo| repo.spec.workdir == workdir)
                {
                    Some(repo) => {
                        store.dispatch(JjMsg::SetActiveRepo { repo_id: repo.id });
                    }
                    None => store.dispatch(JjMsg::OpenRepo { workdir }),
                }
            }

            // Mirror closes: a workdir no longer present in the git store's
            // repos means its tab was closed here too.
            let git_workdirs: Vec<std::path::PathBuf> = self
                .state
                .repos
                .iter()
                .map(|repo| repo.spec.workdir.clone())
                .collect();
            for repo_id in jj_repos_missing_from_git(&store.snapshot(), &git_workdirs) {
                store.dispatch(JjMsg::CloseRepo { repo_id });
            }
        }
    }
}

/// Jj-side repos whose workdir no longer exists among the git store's
/// repos — the set whose tabs were closed and whose jj handles should be
/// released.
fn jj_repos_missing_from_git(jj: &JjAppState, git_workdirs: &[std::path::PathBuf]) -> Vec<RepoId> {
    jj.repos
        .iter()
        .filter(|repo| !git_workdirs.contains(&repo.spec.workdir))
        .map(|repo| repo.id)
        .collect()
}

/// One keyboard step through the rendered rows: `delta` rows down (+) or
/// up (−) from `current`, clamped at the ends (no wrap). With nothing
/// selected — or a selection that left the list — Down starts at the top
/// and Up at the bottom, so both arrows always land on a visible row.
fn next_selected_change(
    ids: &[ChangeId],
    current: Option<&ChangeId>,
    delta: i32,
) -> Option<ChangeId> {
    if ids.is_empty() {
        return None;
    }
    let index = match current.and_then(|current| ids.iter().position(|id| id == current)) {
        Some(index) => index as i64,
        None => return if delta > 0 { ids.first() } else { ids.last() }.cloned(),
    };
    let next = (index + delta as i64).clamp(0, ids.len() as i64 - 1) as usize;
    Some(ids[next].clone())
}

/// A minimal `JjRepoState` for tests, shared with `change_list`'s tests.
#[cfg(test)]
fn test_jj_repo_state(id: u64, path: &str) -> gitcomet_state::jj_store::JjRepoState {
    gitcomet_state::jj_store::JjRepoState {
        id: RepoId(id),
        spec: gitcomet_core::domain::RepoSpec {
            workdir: std::path::PathBuf::from(path),
        },
        open_error: None,
        refresh_epoch: 0,
        log_revset: String::new(),
        changes: Vec::new(),
        next_cursor: None,
        log_loading: false,
        log_error: None,
        load_error: None,
        working_copy: None,
        bookmarks: Vec::new(),
        bookmarks_loading: false,
        conflicts: Vec::new(),
        ops: Vec::new(),
        ops_loading: false,
        pending_command: None,
        last_command_error: None,
        last_network_output: None,
        watch_degraded: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcomet_core::error::{Error, ErrorKind};
    use gitcomet_core::services::{CommandOutput, Result};
    use gitcomet_jj_core::{JjBookmark, JjCommitId, JjLogPage, JjLogQuery, JjOp, JjRepository};
    use gitcomet_state::jj_store::backend::JjBackend;
    use std::time::{Duration, Instant};

    /// An in-memory jj repository that records calls and answers from
    /// fixed fixtures — the same shape as the state crate's store tests,
    /// so the panel tests here run without jj installed.
    struct FakeJjRepository {
        spec: gitcomet_core::domain::RepoSpec,
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl FakeJjRepository {
        fn new(workdir: &str) -> Arc<Self> {
            Arc::new(Self {
                spec: gitcomet_core::domain::RepoSpec {
                    workdir: std::path::PathBuf::from(workdir),
                },
                calls: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn record(&self, label: String) {
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(label);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }

        fn has_call(&self, prefix: &str) -> bool {
            self.calls().iter().any(|c| c.starts_with(prefix))
        }
    }

    fn change(name: &str, working_copy: bool) -> gitcomet_jj_core::JjChange {
        gitcomet_jj_core::JjChange {
            change_id: ChangeId(name.to_string()),
            commit_id: JjCommitId(format!("c{name}")),
            divergent: false,
            conflicted: false,
            is_working_copy: working_copy,
            bookmarks: Vec::new(),
            author_name: "A".to_string(),
            author_email: "a@a".to_string(),
            committed_at_unix: 1,
            description: name.to_string(),
        }
    }

    impl JjRepository for FakeJjRepository {
        fn spec(&self) -> &gitcomet_core::domain::RepoSpec {
            &self.spec
        }

        fn snapshot(&self) -> Result<()> {
            self.record("snapshot".to_string());
            Ok(())
        }

        fn log(&self, query: &JjLogQuery) -> Result<JjLogPage> {
            self.record(format!("log:{}", query.revset));
            Ok(JjLogPage {
                changes: vec![change("at", true), change("base", false)],
                next_cursor: None,
            })
        }

        fn describe(&self, change: &ChangeId, message: &str) -> Result<()> {
            self.record(format!("describe:{}:{message}", change.0));
            Ok(())
        }

        fn new_change(&self, message: Option<&str>) -> Result<gitcomet_jj_core::JjChange> {
            self.record(format!("new:{message:?}"));
            Ok(change("new-at", true))
        }

        fn abandon(&self, change: &ChangeId) -> Result<()> {
            self.record(format!("abandon:{}", change.0));
            Ok(())
        }

        fn squash(&self, from: &ChangeId, into: Option<&ChangeId>) -> Result<()> {
            self.record(format!("squash:{}:{:?}", from.0, into.map(|c| &c.0)));
            Ok(())
        }

        fn split(&self, _change: &ChangeId) -> Result<()> {
            Err(Error::new(ErrorKind::Unsupported("fake split")))
        }

        fn bookmarks(&self) -> Result<Vec<JjBookmark>> {
            self.record("bookmarks".to_string());
            Ok(vec![JjBookmark {
                name: "main".to_string(),
                remote: None,
                target_commit_id: JjCommitId("cbase".to_string()),
                conflicted: false,
            }])
        }

        fn bookmark_create(&self, name: &str, _target: &ChangeId) -> Result<()> {
            self.record(format!("bookmark_create:{name}"));
            Ok(())
        }

        fn bookmark_delete(&self, name: &str) -> Result<()> {
            self.record(format!("bookmark_delete:{name}"));
            Ok(())
        }

        fn bookmark_rename(&self, old_name: &str, new_name: &str) -> Result<()> {
            self.record(format!("bookmark_rename:{old_name}:{new_name}"));
            Ok(())
        }

        fn bookmark_track(&self, name: &str, remote: Option<&str>) -> Result<()> {
            self.record(format!("bookmark_track:{name}:{remote:?}"));
            Ok(())
        }

        fn op_log(&self, _limit: usize) -> Result<Vec<JjOp>> {
            self.record("op_log".to_string());
            Ok(vec![JjOp {
                op_id: "op1".to_string(),
                description: "add workspace".to_string(),
                user: "A <a@a>".to_string(),
                started_at_unix: 1,
            }])
        }

        fn op_undo(&self) -> Result<()> {
            self.record("op_undo".to_string());
            Ok(())
        }

        fn op_restore(&self, op_id: &str) -> Result<()> {
            self.record(format!("op_restore:{op_id}"));
            Ok(())
        }

        fn conflicts(&self) -> Result<Vec<gitcomet_jj_core::JjConflict>> {
            self.record("conflicts".to_string());
            Ok(Vec::new())
        }

        fn fetch_all_with_output(&self) -> Result<CommandOutput> {
            self.record("fetch_all".to_string());
            Ok(CommandOutput::empty_success("jj git fetch --all-remotes"))
        }

        fn push_tracked_with_output(&self) -> Result<CommandOutput> {
            self.record("push".to_string());
            Ok(CommandOutput::empty_success("jj git push"))
        }
    }

    struct FakeJjBackend {
        repo: std::sync::Mutex<Option<Arc<FakeJjRepository>>>,
    }

    impl JjBackend for FakeJjBackend {
        fn open(&self, _workdir: &std::path::Path) -> Result<Arc<dyn JjRepository>> {
            self.repo
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .map(|repo| repo as Arc<dyn JjRepository>)
                .ok_or_else(|| Error::new(ErrorKind::Backend("fake repo taken".to_string())))
        }
    }

    fn wait_until(description: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if ready() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {description}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn jj_repos_missing_from_git_flags_only_closed_workdirs() {
        let jj = JjAppState {
            repos: vec![
                test_jj_repo_state(1, "/tmp/jj-a"),
                test_jj_repo_state(2, "/tmp/jj-closed"),
            ],
            active_repo: None,
        };
        let git_workdirs = vec![
            std::path::PathBuf::from("/tmp/jj-a"),
            std::path::PathBuf::from("/tmp/git-b"),
        ];

        assert_eq!(
            jj_repos_missing_from_git(&jj, &git_workdirs),
            vec![RepoId(2)]
        );
    }

    #[test]
    fn jj_repos_missing_from_git_is_empty_when_all_still_open() {
        let jj = JjAppState {
            repos: vec![test_jj_repo_state(1, "/tmp/jj-a")],
            active_repo: Some(RepoId(1)),
        };
        let git_workdirs = vec![std::path::PathBuf::from("/tmp/jj-a")];

        assert!(jj_repos_missing_from_git(&jj, &git_workdirs).is_empty());
    }

    #[test]
    fn next_selected_change_steps_and_clamps_without_wrapping() {
        let ids: Vec<ChangeId> = ["a", "b", "c"]
            .iter()
            .map(|s| ChangeId(s.to_string()))
            .collect();
        let id = |s: &str| ChangeId(s.to_string());

        // From nothing, Down enters at the top and Up at the bottom.
        assert_eq!(next_selected_change(&ids, None, 1), Some(id("a")));
        assert_eq!(next_selected_change(&ids, None, -1), Some(id("c")));

        // Steps move one row at a time and clamp at both ends.
        assert_eq!(next_selected_change(&ids, Some(&id("a")), 1), Some(id("b")));
        assert_eq!(next_selected_change(&ids, Some(&id("b")), 1), Some(id("c")));
        assert_eq!(next_selected_change(&ids, Some(&id("c")), 1), Some(id("c")));
        assert_eq!(
            next_selected_change(&ids, Some(&id("a")), -1),
            Some(id("a"))
        );

        // A selection that left the list (rewritten away, filtered out)
        // behaves like no selection: both arrows land on a visible row.
        assert_eq!(
            next_selected_change(&ids, Some(&id("gone")), 1),
            Some(id("a"))
        );
        assert_eq!(
            next_selected_change(&ids, Some(&id("gone")), -1),
            Some(id("c"))
        );

        assert_eq!(next_selected_change(&[], None, 1), None);
    }

    /// Enter on the revset bar reloads the list under the bar's revset.
    #[gpui::test]
    fn revset_bar_applies_the_filter_on_enter(cx: &mut gpui::TestAppContext) {
        let repo = FakeJjRepository::new("/tmp/fake-jj-revset");
        let backend = Arc::new(FakeJjBackend {
            repo: std::sync::Mutex::new(Some(Arc::clone(&repo))),
        });
        let (store, events) = JjStore::new(backend);
        let store = Arc::new(store);
        store.dispatch(JjMsg::OpenRepo {
            workdir: std::path::PathBuf::from("/tmp/fake-jj-revset"),
        });
        wait_until("repo to load", || {
            store
                .snapshot()
                .active_repo()
                .is_some_and(|repo| repo.working_copy.is_some())
        });

        let (view, cx) = cx.add_window_view(|window, cx| {
            JjRepoView::new(
                Arc::clone(&store),
                events,
                AppTheme::gitcomet_light(),
                window,
                cx,
            )
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.revset_input
                    .update(cx, |input, cx| input.set_text("main..@", cx));
                view.apply_revset(cx);
            });
        });
        wait_until("the list to reload under the revset", || {
            repo.has_call("log:main..@")
        });

        // The store's log_revset and the bar agree after applying.
        cx.update(|_window, app| {
            let snapshot = store.snapshot();
            let revset = snapshot
                .active_repo()
                .map(|repo| repo.log_revset.clone())
                .unwrap_or_default();
            let bar = view.read(app).revset_input.read(app).text().to_string();
            assert_eq!(revset, "main..@");
            assert_eq!(bar, "main..@");
        });
    }

    /// ↑/↓ step the selection through the list (skipping @), and Escape
    /// clears it.
    #[gpui::test]
    fn arrow_keys_move_the_selection(cx: &mut gpui::TestAppContext) {
        let repo = FakeJjRepository::new("/tmp/fake-jj-keys");
        let backend = Arc::new(FakeJjBackend {
            repo: std::sync::Mutex::new(Some(Arc::clone(&repo))),
        });
        let (store, events) = JjStore::new(backend);
        let store = Arc::new(store);
        store.dispatch(JjMsg::OpenRepo {
            workdir: std::path::PathBuf::from("/tmp/fake-jj-keys"),
        });
        wait_until("repo to load with changes", || {
            store
                .snapshot()
                .active_repo()
                .is_some_and(|repo| repo.changes.len() >= 2)
        });

        let (view, cx) = cx.add_window_view(|window, cx| {
            JjRepoView::new(
                Arc::clone(&store),
                events,
                AppTheme::gitcomet_light(),
                window,
                cx,
            )
        });
        cx.run_until_parked();
        // Build the element tree so the list's key handler is registered.
        cx.update(|window, app| {
            let _ = window.draw(app);
        });

        cx.update(|window, app| {
            let focus = view.read(app).list_focus.clone();
            window.focus(&focus, app);
        });
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("down");
        cx.update(|_window, app| {
            // Two Downs from nothing: "base" (the first non-@ row), then
            // clamped — the fixture has only one non-@ change.
            assert_eq!(
                view.read(app).selected_change,
                Some(ChangeId("base".to_string()))
            );
        });

        cx.simulate_keystrokes("escape");
        cx.update(|_window, app| {
            assert_eq!(view.read(app).selected_change, None);
        });
    }

    /// Bookmark create/delete and operation undo reach the backend:
    /// create targets the selected change (@ when nothing is selected),
    /// delete spells the bare local name, undo reverts the latest op.
    #[gpui::test]
    fn bookmark_and_undo_gestures_reach_the_backend(cx: &mut gpui::TestAppContext) {
        let repo = FakeJjRepository::new("/tmp/fake-jj-bm");
        let backend = Arc::new(FakeJjBackend {
            repo: std::sync::Mutex::new(Some(Arc::clone(&repo))),
        });
        let (store, events) = JjStore::new(backend);
        let store = Arc::new(store);
        store.dispatch(JjMsg::OpenRepo {
            workdir: std::path::PathBuf::from("/tmp/fake-jj-bm"),
        });
        wait_until("repo to load with a bookmark and an op", || {
            store.snapshot().active_repo().is_some_and(|repo| {
                repo.working_copy.is_some() && !repo.bookmarks.is_empty() && !repo.ops.is_empty()
            })
        });

        let (view, cx) = cx.add_window_view(|window, cx| {
            JjRepoView::new(
                Arc::clone(&store),
                events,
                AppTheme::gitcomet_light(),
                window,
                cx,
            )
        });
        cx.run_until_parked();

        // Create from the bar: no selection → target is @ ("at").
        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.bookmark_input
                    .update(cx, |input, cx| input.set_text("topic", cx));
                view.create_bookmark(cx);
                // The bar clears once the create is dispatched.
                assert_eq!(view.bookmark_input.read(cx).text(), "");
            });
        });
        let mutation_settled = || {
            store
                .snapshot()
                .active_repo()
                .is_some_and(|repo| repo.pending_command.is_none())
        };
        wait_until("bookmark create to run and settle", || {
            repo.has_call("bookmark_create:topic") && mutation_settled()
        });

        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                // A create with a selected change targets the selection.
                view.selected_change = Some(ChangeId("base".to_string()));
                view.bookmark_input
                    .update(cx, |input, cx| input.set_text("base-tip", cx));
                view.create_bookmark(cx);
            });
        });
        wait_until("bookmark create to target the selection and settle", || {
            repo.has_call("bookmark_create:base-tip") && mutation_settled()
        });

        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                // Empty names never dispatch.
                view.bookmark_input
                    .update(cx, |input, cx| input.set_text("", cx));
                view.create_bookmark(cx);
                view.delete_bookmark("main", cx);
            });
        });
        wait_until("bookmark delete to run and settle", || {
            repo.has_call("bookmark_delete:main") && mutation_settled()
        });

        // A separate gesture: the store serializes mutations per repo (a
        // gesture while one is pending is dropped, not queued), so each
        // dispatch waits for the previous one to settle.
        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.undo_operation(cx);
            });
        });
        wait_until("op undo to run", || repo.has_call("op_undo"));
    }

    /// The describe bar's gestures reach the store as mutations on @:
    /// Enter/Describe maps to `jj describe`, the "New change" button to
    /// `jj new -m`, and an empty bar never describes (it would erase @'s
    /// description).
    #[gpui::test]
    fn describe_bar_gestures_dispatch_to_the_working_copy(cx: &mut gpui::TestAppContext) {
        let repo = FakeJjRepository::new("/tmp/fake-jj-panel");
        let backend = Arc::new(FakeJjBackend {
            repo: std::sync::Mutex::new(Some(Arc::clone(&repo))),
        });
        let (store, events) = JjStore::new(backend);
        let store = Arc::new(store);
        store.dispatch(JjMsg::OpenRepo {
            workdir: std::path::PathBuf::from("/tmp/fake-jj-panel"),
        });
        wait_until("repo to load with a working copy", || {
            store
                .snapshot()
                .active_repo()
                .is_some_and(|repo| repo.working_copy.is_some())
        });

        let (view, cx) = cx.add_window_view(|window, cx| {
            JjRepoView::new(
                Arc::clone(&store),
                events,
                AppTheme::gitcomet_light(),
                window,
                cx,
            )
        });
        cx.run_until_parked();

        // The bar prefills @'s current description.
        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.sync_describe_input(cx);
                assert_eq!(
                    view.describe_input.read(cx).text(),
                    "at",
                    "the bar prefills @'s description"
                );

                // Describing with an empty bar is a no-op, not an erasure:
                // nothing is dispatched, so nothing can land.
                view.describe_input
                    .update(cx, |input, cx| input.set_text("", cx));
                view.submit_describe(cx);
            });
        });
        assert!(
            !repo.has_call("describe:"),
            "empty describe must not dispatch, calls: {:?}",
            repo.calls()
        );

        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.describe_input
                    .update(cx, |input, cx| input.set_text("hello change", cx));
                view.submit_describe(cx);
            });
        });
        wait_until("describe to run on @", || {
            repo.has_call("describe:at:hello change")
        });

        // `jj new` carries the bar's text as the new change's message.
        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.describe_input
                    .update(cx, |input, cx| input.set_text("next up", cx));
                view.start_new_change(cx);
            });
        });
        wait_until("jj new to run with the message", || {
            repo.has_call("new:Some(\"next up\")")
        });
    }

    /// While @ stays the same change, snapshot refreshes never rewrite
    /// what the user typed; when @ moves (after `jj new`), the bar syncs
    /// to the new change's description.
    #[gpui::test]
    fn describe_input_syncs_only_when_the_working_copy_changes(cx: &mut gpui::TestAppContext) {
        let repo = FakeJjRepository::new("/tmp/fake-jj-sync");
        let backend = Arc::new(FakeJjBackend {
            repo: std::sync::Mutex::new(Some(Arc::clone(&repo))),
        });
        let (store, events) = JjStore::new(backend);
        let store = Arc::new(store);
        store.dispatch(JjMsg::OpenRepo {
            workdir: std::path::PathBuf::from("/tmp/fake-jj-sync"),
        });
        wait_until("repo to load with a working copy", || {
            store
                .snapshot()
                .active_repo()
                .is_some_and(|repo| repo.working_copy.is_some())
        });

        let (view, cx) = cx.add_window_view(|window, cx| {
            JjRepoView::new(
                Arc::clone(&store),
                events,
                AppTheme::gitcomet_light(),
                window,
                cx,
            )
        });
        cx.run_until_parked();

        cx.update(|_window, app| {
            view.update(app, |view, cx| {
                view.sync_describe_input(cx);
                assert_eq!(view.describe_input.read(cx).text(), "at");

                view.describe_input
                    .update(cx, |input, cx| input.set_text("user typing", cx));
                view.sync_describe_input(cx);
                assert_eq!(
                    view.describe_input.read(cx).text(),
                    "user typing",
                    "same @ must not clobber the input"
                );
            });
        });
    }
}
