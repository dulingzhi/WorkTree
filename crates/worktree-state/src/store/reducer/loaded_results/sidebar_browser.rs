use super::util::{EffectAccumulator, push_diagnostic};
use crate::model::{
    AppState, DiagnosticKind, Loadable, RepoId, RepoLoadsInFlight, RepoState, SidebarDataRequest,
    SidebarMode,
};
use crate::msg::Effect;
use std::path::PathBuf;
use std::sync::Arc;
use worktree_core::domain::{CommitId, FileEntry, FileSource};

pub(in crate::store::reducer) fn append_ensure_sidebar_data_effects(
    repo_state: &mut RepoState,
    effects: &mut impl EffectAccumulator,
) {
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return;
    }

    let repo_id = repo_state.id;
    let request = repo_state.sidebar_data_request;

    if request.worktrees && matches!(repo_state.worktrees, Loadable::NotLoaded) {
        repo_state.set_worktrees(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::WORKTREES)
        {
            effects.push_effect(Effect::LoadWorktrees { repo_id });
        }
    }

    if request.submodules && matches!(repo_state.submodules, Loadable::NotLoaded) {
        repo_state.set_submodules(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::SUBMODULES)
        {
            effects.push_effect(Effect::LoadSubmodules { repo_id });
        }
    }

    if request.stashes && matches!(repo_state.stashes, Loadable::NotLoaded) {
        repo_state.set_stashes(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::STASHES)
        {
            effects.push_effect(Effect::LoadStashes { repo_id, limit: 50 });
        }
    }

    if request.tags && matches!(repo_state.tags, Loadable::NotLoaded) {
        repo_state.set_tags(Loadable::Loading);
        if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
            effects.push_effect(Effect::LoadTags { repo_id });
        }
    }
}

pub(in crate::store::reducer) fn ensure_sidebar_data(
    state: &mut AppState,
    repo_id: RepoId,
    request: SidebarDataRequest,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    repo_state.set_sidebar_data_request(request);
    let mut effects = Vec::new();
    append_ensure_sidebar_data_effects(repo_state, &mut effects);
    effects
}

pub(in crate::store::reducer) fn load_file_browser(
    state: &mut AppState,
    repo_id: RepoId,
    source: FileSource,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    let source_changed = repo_state.file_browser.source != source;
    repo_state.file_browser.source = source;
    // Blank the tree only when there is nothing worth keeping: rows from another
    // source would be actively wrong, but a same-source refresh can leave them up.
    if source_changed || !matches!(repo_state.file_browser.entries, Loadable::Ready(_)) {
        repo_state.file_browser.entries = Loadable::Loading;
    }
    repo_state.file_browser.bump_rev();
    request_file_browser_load(repo_state).into_iter().collect()
}

/// Expand every directory on the way to `path` so the file explorer can show it.
///
/// Also clears the search query: the filtered view builds its rows from matches
/// and force-expands their ancestors, ignoring `expanded_dirs` entirely, so a
/// reveal into a filtered tree would scroll to a row index that does not mean
/// what the caller computed.
pub(in crate::store::reducer) fn reveal_file_browser_path(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // `ancestors()` yields the path itself first — skip it, a file is not a
    // directory to expand — and stops before the empty root component.
    for ancestor in path.ancestors().skip(1) {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        repo_state
            .file_browser
            .expanded_dirs
            .insert(Arc::new(ancestor.to_path_buf()));
    }
    if !repo_state.file_browser.search_query.is_empty() {
        repo_state.file_browser.search_query.clear();
    }
    repo_state.file_browser.bump_rev();
    Vec::new()
}

/// Whether a query actually filters the file tree, and so force-expands every
/// directory and ignores `expanded_dirs`.
///
/// The search input is multiline and stores what was typed verbatim, so a lone
/// space is a non-empty query that filters nothing. Mirrors the view's
/// `file_browser_search_is_active`.
pub(super) fn file_browser_query_filters(query: &str) -> bool {
    query.lines().any(|line| !line.trim().is_empty())
}

fn file_browser_is_filtered(repo_state: &RepoState) -> bool {
    file_browser_query_filters(&repo_state.file_browser.search_query)
}

pub(in crate::store::reducer) fn toggle_file_browser_dir(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        // A filtered tree renders every directory expanded and never reads
        // `expanded_dirs`, so a toggle here would move nothing on screen and
        // then silently reshape the tree the moment the search was cleared.
        if file_browser_is_filtered(repo_state) {
            return Vec::new();
        }
        let path = Arc::new(path);
        if repo_state.file_browser.expanded_dirs.contains(&path) {
            repo_state.file_browser.expanded_dirs.remove(&path);
        } else {
            repo_state.file_browser.expanded_dirs.insert(path);
        }
        repo_state.file_browser.bump_rev();
    }
    Vec::new()
}

/// Expand or collapse `path` and every directory under it.
///
/// The backend enumerates the whole tree in one pass, so every descendant is
/// already in `entries` and this needs no loading. `starts_with` on the flat
/// list also covers `path` itself, which is what makes "Expand all under here"
/// open the folder it was invoked on.
pub(in crate::store::reducer) fn set_file_browser_dir_expanded_recursive(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    expanded: bool,
) -> Vec<Effect> {
    // `Path::starts_with("")` is true of every path, so an empty path would
    // reach the whole tree and a collapse would wipe `expanded_dirs` outright.
    // The branch-group sibling guards this the same way.
    if path.as_os_str().is_empty() {
        return Vec::new();
    }
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // Frozen while a search filters the tree, for the same reason a single
    // toggle is.
    if file_browser_is_filtered(repo_state) {
        return Vec::new();
    }
    let Loadable::Ready(entries) = &repo_state.file_browser.entries else {
        return Vec::new();
    };

    // Cloning the Arc releases the borrow on `file_browser` so `expanded_dirs`
    // can be written while the entry list is walked.
    let entries = Arc::clone(entries);
    let mut changed = false;
    for entry in entries.iter() {
        if entry.kind != worktree_core::domain::FileEntryKind::Directory
            || !entry.path.starts_with(&path)
        {
            continue;
        }
        // Each entry already owns its path as an `Arc`, so expanding reuses it
        // rather than allocating a second copy per directory.
        changed |= if expanded {
            repo_state
                .file_browser
                .expanded_dirs
                .insert(Arc::clone(&entry.path))
        } else {
            repo_state.file_browser.expanded_dirs.remove(&entry.path)
        };
    }

    if changed {
        repo_state.file_browser.bump_rev();
    }
    Vec::new()
}

pub(in crate::store::reducer) fn set_file_browser_search(
    state: &mut AppState,
    repo_id: RepoId,
    query: String,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.file_browser.search_query != query
    {
        repo_state.file_browser.search_query = query;
        repo_state.file_browser.bump_rev();
    }
    Vec::new()
}

pub(in crate::store::reducer) fn request_file_browser_load(
    repo_state: &mut RepoState,
) -> Option<Effect> {
    repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::FILE_BROWSER)
        .then(|| Effect::LoadFileBrowser {
            repo_id: repo_state.id,
            source: repo_state.file_browser.source.clone(),
        })
}

pub(in crate::store::reducer) fn set_file_browser_source(
    state: &mut AppState,
    repo_id: RepoId,
    source: FileSource,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.file_browser.source != source
    {
        repo_state.file_browser.source = source;
        repo_state.file_browser.entries = Loadable::NotLoaded;
        repo_state.file_browser.expanded_dirs.clear();
        repo_state.file_browser.search_query.clear();
        repo_state.file_browser.stale = false;
        repo_state.file_browser.bump_rev();
        return request_file_browser_load(repo_state).into_iter().collect();
    }
    Vec::new()
}

pub(in crate::store::reducer) fn set_sidebar_mode(
    state: &mut AppState,
    mode: SidebarMode,
) -> Vec<Effect> {
    if state.sidebar_mode != mode {
        state.sidebar_mode = mode;

        if mode == SidebarMode::Files
            && let Some(repo_id) = state.active_repo
            && let Some(repo) = state.repos.iter_mut().find(|r| r.id == repo_id)
            && repo.file_browser.needs_load()
        {
            return request_file_browser_load(repo).into_iter().collect();
        }
    }
    Vec::new()
}

pub(in crate::store::reducer) fn browse_repository_at_commit(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
) -> Vec<Effect> {
    const BROWSE_HISTORY_CAP: usize = 32;
    // Capture the open file (if any) before re-targeting it to the new point.
    let reopen_path = browse_open_content_path(state, repo_id);
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && !repo_state.browse_history.contains(&commit_id)
    {
        repo_state.browse_history.push(commit_id.clone());
        if repo_state.browse_history.len() > BROWSE_HISTORY_CAP {
            repo_state.browse_history.remove(0);
        }
    }
    state.sidebar_mode = SidebarMode::Files;
    let mut effects =
        set_file_browser_source(state, repo_id, FileSource::Commit(commit_id.clone()));
    if let Some(path) = reopen_path
        && effects
            .iter()
            .any(|e| matches!(e, Effect::LoadFileBrowser { .. }))
    {
        effects.extend(super::diff_selection::open_file_content(
            state,
            repo_id,
            FileSource::Commit(commit_id),
            path,
        ));
    }
    effects
}

pub(in crate::store::reducer) fn reset_browse_to_live(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let reopen_path = browse_open_content_path(state, repo_id);
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.browse_history.clear();
    }
    let mut effects = set_file_browser_source(state, repo_id, FileSource::WorkingDirectory);
    if let Some(path) = reopen_path
        && effects
            .iter()
            .any(|e| matches!(e, Effect::LoadFileBrowser { .. }))
    {
        effects.extend(super::diff_selection::open_file_content(
            state,
            repo_id,
            FileSource::WorkingDirectory,
            path,
        ));
    }
    effects
}

/// Path of the file currently shown as full content (if any), so a browse-point
/// change can re-open the same file at the new point.
pub(super) fn browse_open_content_path(
    state: &AppState,
    repo_id: RepoId,
) -> Option<std::path::PathBuf> {
    let repo = state.repos.iter().find(|r| r.id == repo_id)?;
    if !repo.diff_state.content_preview {
        return None;
    }
    match &repo.diff_state.diff_target {
        Some(worktree_core::domain::DiffTarget::Commit { path: Some(p), .. }) => Some(p.clone()),
        Some(worktree_core::domain::DiffTarget::WorkingTree { path, .. }) => Some(path.clone()),
        _ => None,
    }
}

pub(in crate::store::reducer) fn file_browser_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    source: FileSource,
    result: std::result::Result<Vec<FileEntry>, worktree_core::error::Error>,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    // Release the lane before the stale-source guard: a reply for a source the
    // user has already navigated away from still ends the walk that was running,
    // and the request queued behind it is the one that matters now.
    let has_pending = repo_state
        .loads_in_flight
        .finish(RepoLoadsInFlight::FILE_BROWSER);

    if repo_state.file_browser.source == source {
        repo_state.file_browser.entries = match result {
            Ok(v) => Loadable::Ready(Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.file_browser.stale = false;
        repo_state.file_browser.bump_rev();
    }

    if has_pending {
        return vec![Effect::LoadFileBrowser {
            repo_id,
            source: repo_state.file_browser.source.clone(),
        }];
    }
    Vec::new()
}
