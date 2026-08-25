//! The native Jujutsu flavor (#76): when the active repository has a `.jj`
//! directory (surfaced by the git store's `RepoCapabilities::is_jj`), the
//! repo content area swaps from the git three-pane layout to `JjRepoView`,
//! backed by the jj store from `gitcomet_state::jj_store`.
//!
//! The flavor switch is intentionally thin: the tab bar, action bar,
//! settings window, shortcuts, i18n, and theme all stay shared because they
//! belong to `GitCometView` — only the content card swaps. The git store
//! keeps the repo open underneath (the P0/P1 compat path owns tabs and
//! recents); #80 retires that overlap once the jj panels cover browsing.
//!
//! Everything here compiles only with `--features jj`; git-only builds
//! never see it.

use super::*;

use gitcomet_state::jj_store::{JjAppState, JjStore};
use gitcomet_state::msg::StoreEvent;

use super::splash::{CONTENT_CARD_BOTTOM_MARGIN_PX, CONTENT_CARD_GAP_PX};

/// The repo content area for a Jujutsu repository. Owns no store state —
/// it renders a snapshot of the jj store and forwards gestures as
/// messages, exactly like the git panes. The panels (#77–#79) extend this
/// view; for now it shows the working copy, the change count, and the
/// load/error states, which is enough to prove the flavor switch wiring.
pub(crate) struct JjRepoView {
    state: Arc<JjAppState>,
    theme: AppTheme,
    _poller: gpui::Task<()>,
    /// Held (not drained) under the test runtime, mirroring `Poller`.
    _held_events: Option<smol::channel::Receiver<StoreEvent>>,
}

impl JjRepoView {
    pub(super) fn new(
        store: Arc<JjStore>,
        events: smol::channel::Receiver<StoreEvent>,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let state = store.snapshot();

        let runtime = crate::ui_runtime::current();
        if !runtime.uses_live_store_poller() {
            return Self {
                state,
                theme,
                _poller: gpui::Task::ready(()),
                _held_events: Some(events),
            };
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
                    cx.notify();
                });
            }
        });

        Self {
            state,
            theme,
            _poller,
            _held_events: None,
        }
    }

    pub(super) fn set_theme(&mut self, theme: AppTheme, cx: &mut gpui::Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}

impl Render for JjRepoView {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let _ = cx;
        let theme = self.theme;
        let repo = self.state.active_repo();

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

                // Working copy: the @ change jj always shows at the top.
                let working_copy = repo
                    .working_copy
                    .clone()
                    .or_else(|| repo.changes.iter().find(|c| c.is_working_copy).cloned());
                if let Some(wc) = working_copy {
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
                    card = card.child(
                        div()
                            .rounded(px(theme.radii.panel))
                            .border_1()
                            .border_color(theme.colors.stroke.default)
                            .bg(theme.colors.surface.raised)
                            .p_3()
                            .flex()
                            .flex_col()
                            .gap_1()
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
                                    .text_color(theme.colors.foreground.primary)
                                    .child(wc.change_id.0.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.colors.foreground.primary)
                                    .child(if wc.description.is_empty() {
                                        crate::i18n::tr("jj.working_copy.empty_description")
                                            .to_string()
                                    } else {
                                        wc.description.clone()
                                    }),
                            )
                            .when(!flags.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.status.warning.foreground)
                                        .child(flags),
                                )
                            }),
                    );
                }

                // Change count + more-available hint. The full list lands
                // with the change panel (#77).
                card = card.child(if repo.log_loading {
                    div()
                        .text_sm()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr("jj.loading"))
                } else if repo.changes.is_empty() {
                    div()
                        .text_sm()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr("jj.changes.empty"))
                } else {
                    let count = repo.changes.len();
                    let mut row = div()
                        .text_sm()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::t!("jj.changes.count", count = count));
                    if repo.next_cursor.is_some() {
                        row = row
                            .child(" · ")
                            .child(crate::i18n::tr("jj.changes.more_available"));
                    }
                    row
                });

                if let Some(error) = repo.log_error.clone() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.status.danger.foreground)
                            .child(error),
                    );
                }

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
    pub(super) fn jj_center_content(&mut self, cx: &mut gpui::Context<Self>) -> Option<AnyElement> {
        if !renders_full_chrome(self.view_mode) {
            return None;
        }
        if !self.jj_flavor_active() {
            return None;
        }
        // The pane appears on the state apply after the first jj repo
        // opens; until then keep the git layout (it renders the compat
        // banner), so one frame of it is fine.
        let pane = self.jj_pane.clone()?;

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
    /// closes so watchers stop.
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
            let store = Arc::new(store);
            let pane = cx.new(|cx| JjRepoView::new(Arc::clone(&store), events, self.theme, cx));
            self.jj_store = Some(store);
            self.jj_pane = Some(pane);
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
                        store.dispatch(gitcomet_state::jj_store::JjMsg::SetActiveRepo {
                            repo_id: repo.id,
                        });
                    }
                    None => store.dispatch(gitcomet_state::jj_store::JjMsg::OpenRepo { workdir }),
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
                store.dispatch(gitcomet_state::jj_store::JjMsg::CloseRepo { repo_id });
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

#[cfg(test)]
mod tests {
    use super::*;
    use gitcomet_core::domain::RepoSpec;
    use gitcomet_state::jj_store::JjRepoState;

    fn jj_repo_state(id: u64, path: &str) -> JjRepoState {
        JjRepoState {
            id: RepoId(id),
            spec: RepoSpec {
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

    #[test]
    fn jj_repos_missing_from_git_flags_only_closed_workdirs() {
        let jj = JjAppState {
            repos: vec![
                jj_repo_state(1, "/tmp/jj-a"),
                jj_repo_state(2, "/tmp/jj-closed"),
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
            repos: vec![jj_repo_state(1, "/tmp/jj-a")],
            active_repo: Some(RepoId(1)),
        };
        let git_workdirs = vec![std::path::PathBuf::from("/tmp/jj-a")];

        assert!(jj_repos_missing_from_git(&jj, &git_workdirs).is_empty());
    }
}
