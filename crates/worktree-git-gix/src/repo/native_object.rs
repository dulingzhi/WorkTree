//! Cache-free object access for the hot paths (feature `native-object` only).
//!
//! This module is the single integration point for the "remove caching, be
//! faster" refactor. It is compiled *only* when the `native-object` feature is
//! enabled; under the default build it contributes nothing, so the default
//! build's hot paths stay exactly as they were (pure gix).
//!
//! When the feature is on, every call site that used to read a Git object
//! through gix's high-level handle types now goes through the helpers here.
//! The decoded output is **byte-for-byte identical** to what gix produces:
//!
//!   * We read the object *body* natively (a single pack-index hash lookup, a
//!     loose-object open, or a `git cat-file` fallback) and feed those exact
//!     bytes into gix's own pure parser (`ObjectRef::from_bytes`). The parser
//!     is gix's, so the owned value it yields is the same gix would yield.
//!   * On any native miss/error we fall back to gix in this module, so exotic
//!     object stores (thin/promisor/partial clones, unindexed alternates)
//!     behave exactly like the default build.
//!
//! Peeling is *not* done by the `find_*` helpers — `find_commit`/`find_tree`
//! return the object as-is, exactly like gix. Call sites that need a
//! commit/tree use the explicit [`peel_to_commit`]/[`peel_to_tree_owned`]
//! helpers below, which replicate gix's peel semantics
//! (commit→tree, tag→target, tree→self, blob→err), bounded to 10 hops.

use std::path::PathBuf;
use std::sync::Arc;

use gix::objs::{tree::EntryMode, Kind, Object, Tree};
use gix::ObjectId;

use worktree_core::domain::{FileEntry, FileEntryKind};
use worktree_core::error::{Error, ErrorKind};
use worktree_core::services::Result;

// ---------------------------------------------------------------------------
// Unified owned-object read
// ---------------------------------------------------------------------------

/// Read the object identified by `id` and return it as owned `gix::objs::Object`.
///
/// Does **not** peel. A missing object returns `None`; a decode error returns
/// `None`. On the native path, any native miss/error falls back to gix so the
/// decoded value is identical to the default build.
#[cfg(feature = "native-object")]
fn read_owned(repo: &gix::Repository, id: &ObjectId) -> Option<Object> {
    let git_dir = repo.git_dir();
    let hash = repo.object_hash();
    let hex = id.to_string();
    // `read_object` returns the detected kind and the object *body* (no
    // `type SP len \0` header), exactly what `ObjectRef::from_bytes` consumes.
    if let Some((obj_kind, body)) = worktree_git_native::read_object_any(git_dir, &hex) {
        let kind = match obj_kind {
            worktree_git_native::ObjKind::Commit => Kind::Commit,
            worktree_git_native::ObjKind::Tree => Kind::Tree,
            worktree_git_native::ObjKind::Blob => Kind::Blob,
            worktree_git_native::ObjKind::Tag => Kind::Tag,
        };
        if let Ok(parsed) = gix::objs::ObjectRef::from_bytes(&body, kind, hash)
            .ok()?
            .into_owned()
        {
            return Some(parsed);
        }
    }
    // Fallback: any native miss (thin pack base, promisor, alternates) must
    // behave like gix, so re-read through gix here.
    let detached = repo.find_object(*id).ok()?.detach();
    gix::objs::ObjectRef::from_bytes(&detached.data, detached.kind, hash)
        .ok()?
        .into_owned()
        .ok()
}

// ---------------------------------------------------------------------------
// Typed accessors (non-peeling)
// ---------------------------------------------------------------------------

/// Return the object `id` as an owned commit, or `None` if it is not a commit.
#[cfg(feature = "native-object")]
pub(crate) fn find_commit_owned(repo: &gix::Repository, id: &ObjectId) -> Option<gix::objs::Commit> {
    match read_owned(repo, id)? {
        Object::Commit(c) => Some(c),
        _ => None,
    }
}

/// Return the tree reachable from `id` as an owned tree, peeling commits and
/// tags to their tree exactly like gix's `object.peel_to_tree()`. A bare tree id
/// is returned unchanged, so callers that already hold a tree id (e.g. the file
/// browser walking `commit.tree`) see no behavior change.
#[cfg(feature = "native-object")]
pub(crate) fn find_tree_owned(repo: &gix::Repository, id: &ObjectId) -> Option<Tree> {
    peel_to_tree_owned(repo, id)
}

/// Return the blob content at `id`, or `None` if it is not a blob.
///
/// This is the native equivalent of the original
/// `repo.find_object(id)?.detach().into_blob()?.data` hot-path read.
#[cfg(feature = "native-object")]
pub(crate) fn blob_bytes(repo: &gix::Repository, id: &ObjectId) -> Option<Vec<u8>> {
    match read_owned(repo, id)? {
        Object::Blob(b) => Some(b.data),
        _ => None,
    }
}

/// Return the kind of the object at `id`, or `None` if it cannot be read.
#[cfg(feature = "native-object")]
pub(crate) fn peek_object_kind(repo: &gix::Repository, id: &ObjectId) -> Option<Kind> {
    read_owned(repo, id).map(|o| o.kind())
}

// ---------------------------------------------------------------------------
// Peeling (replicates gix's peel semantics)
// ---------------------------------------------------------------------------

/// Follow tag/commit until a commit is reached, returning the owned commit and
/// its (peeled) id.
#[cfg(feature = "native-object")]
pub(crate) fn peel_to_commit(
    repo: &gix::Repository,
    id: &ObjectId,
) -> Option<(ObjectId, gix::objs::Commit)> {
    let mut cur = *id;
    for _ in 0..10 {
        match read_owned(repo, &cur)? {
            Object::Commit(c) => return Some((cur, c)),
            Object::Tag(t) => cur = t.target,
            _ => return None,
        }
    }
    None
}

/// Convenience: the peeled commit's id only.
#[cfg(feature = "native-object")]
pub(crate) fn peel_to_commit_id(repo: &gix::Repository, id: &ObjectId) -> Option<ObjectId> {
    peel_to_commit(repo, id).map(|(cid, _)| cid)
}

/// Follow tag/commit/tree until a tree is reached, returning the owned tree.
#[cfg(feature = "native-object")]
pub(crate) fn peel_to_tree_owned(repo: &gix::Repository, id: &ObjectId) -> Option<Tree> {
    let mut cur = *id;
    for _ in 0..10 {
        match read_owned(repo, &cur)? {
            Object::Tree(t) => return Some(t),
            Object::Commit(c) => cur = c.tree,
            Object::Tag(t) => cur = t.target,
            Object::Blob(_) => return None,
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tree walk (file browser)
// ---------------------------------------------------------------------------

/// Recursively collect the entries of `tree` into `out`, mirroring the
/// high-level `collect_tree_entries` in `file_browser.rs` exactly:
///
///   * directories come first, then files, each group sorted by name;
///   * a tree entry pushes a [`FileEntryKind::Directory`] row and recurses;
///   * a blob or symlink entry pushes a [`FileEntryKind::File`] row
///     (executable blobs are emitted as `File`, matching the original).
///
/// Unlike the high-level version this operates on an *owned* `gix::objs::Tree`
/// and performs lookups through [`peel_to_tree_owned`], so it works under the
/// native-object feature without a `gix::Tree` handle.
#[cfg(feature = "native-object")]
pub(crate) fn collect_tree_entries(
    repo: &gix::Repository,
    tree: &Tree,
    parent_path: String,
    out: &mut Vec<FileEntry>,
    depth: usize,
) -> Result<()> {
    let mut child_entries: Vec<(String, EntryMode, ObjectId)> =
        Vec::with_capacity(tree.entries.len());

    for entry in &tree.entries {
        let name = entry.filename.to_string();
        let mode = entry.mode;
        let oid = entry.oid;
        child_entries.push((name, mode, oid));
    }

    child_entries.sort_by(|(a_name, a_mode, _), (b_name, b_mode, _)| {
        let a_is_dir = a_mode.is_tree();
        let b_is_dir = b_mode.is_tree();
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a_name.cmp(b_name),
        }
    });

    for (name, mode, oid) in child_entries {
        let path = if parent_path.is_empty() {
            name.clone()
        } else {
            format!("{parent_path}/{name}")
        };

        if mode.is_tree() {
            out.push(FileEntry {
                name,
                path: Arc::new(PathBuf::from(&path)),
                kind: FileEntryKind::Directory,
                depth,
            });

            let child_tree = peel_to_tree_owned(repo, &oid).ok_or_else(|| {
                Error::new(ErrorKind::Backend(format!("gix peel_to_tree: {oid}")))
            })?;
            collect_tree_entries(repo, &child_tree, path, out, depth + 1)?;
        } else if mode.is_blob() || mode.is_link() {
            out.push(FileEntry {
                name,
                path: Arc::new(PathBuf::from(&path)),
                kind: FileEntryKind::File,
                depth,
            });
        }
    }

    Ok(())
}

/// Resolve the entry at relative `path` inside `tree`, returning its mode and
/// object id — mirroring `gix::Tree::lookup_entry_by_path` exactly. Returns
/// `None` when the path does not exist or is not a single (non-tree-collapsing)
/// entry.
#[cfg(feature = "native-object")]
pub(crate) fn tree_lookup_entry(
    repo: &gix::Repository,
    tree: &Tree,
    path: &std::path::Path,
) -> Option<(EntryMode, ObjectId)> {
    let mut cur: Tree = tree.clone();
    let mut components = path.components().peekable();
    while let Some(comp) = components.next() {
        let name = match comp {
            std::path::Component::Normal(s) => s.to_string_lossy().into_owned(),
            _ => return None,
        };
        let next = cur.entries.iter().find(|e| e.filename.to_string() == name)?;
        if components.peek().is_some() {
            cur = peel_to_tree_owned(repo, &next.oid)?;
        } else {
            return Some((next.mode, next.oid));
        }
    }
    None
}

/// Resolve the blob bytes at `path` within the tree named by `tree_id`, or
/// `None` when the path is absent or not a blob/symlink. Mirrors the
/// `find_object -> peel_to_tree -> lookup_entry_by_path -> find_object` chain
/// used by the high-level image-blob helpers.
#[cfg(feature = "native-object")]
pub(crate) fn blob_at_tree_path(
    repo: &gix::Repository,
    tree_id: &ObjectId,
    path: &std::path::Path,
) -> Option<Vec<u8>> {
    let tree = peel_to_tree_owned(repo, tree_id)?;
    let (mode, oid) = tree_lookup_entry(repo, &tree, path)?;
    if mode.is_blob() || mode.is_link() {
        blob_bytes(repo, &oid)
    } else {
        None
    }
}
