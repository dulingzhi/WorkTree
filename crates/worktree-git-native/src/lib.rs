//! `worktree-git-native` — Stage-0 spike of a cache-free, biturbo-style native
//! Git object store for GitComet.
//!
//! The end goal of the refactor this crate supports is to *remove* GitComet's
//! on-disk history cache and its stamp-based in-memory caches, and instead
//! re-walk the commit graph through a fast native object reader. This crate is
//! the object database half of that reader, ported faithfully from
//! `D:\Projects\GitFork\src\biturbo` (Fork's `biturbo.dll` reimplementation).
//!
//! It is intentionally NOT in the workspace `default-members`, so the default
//! `cargo build` does not compile it and cannot regress the shipped build. It
//! is built only via `cargo build -p worktree-git-native`, its tests, or when
//! `worktree-git-gix` opts into the (off by default) `native-object` feature.
//!
//! Public surface (kept small on purpose): fetch a single object's body by OID
//! and type, exactly as `git cat-file <type> <oid>` would. Stage-1 will add the
//! commit-graph walk and wire it into the hot paths behind the feature flag.

mod object_store;

pub use object_store::{ObjKind, fetch_object_bytes, read_object as read_object_any};

/// Fetch a git object's raw content by OID and expected type, identical to
/// `git cat-file <type> <hex>` stdout. `git_dir` is the repository's `.git`
/// directory. Returns `None` only when the object truly does not exist (or the
/// type does not match). The native reader resolves pack/loose objects; a
/// `git` CLI fallback covers exotic object stores (thin / promisor packs).
pub fn read_object(git_dir: &std::path::Path, hex: &str, obj_type: &str) -> Option<Vec<u8>> {
    fetch_object_bytes(git_dir, hex, obj_type)
}
