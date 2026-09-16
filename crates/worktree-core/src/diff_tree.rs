//! Directory-level aggregation of per-file changes.
//!
//! Turns a flat list of [`CommitFileChange`] (as produced by the diff backend)
//! into a nested [`DirectoryNode`] tree rooted at a chosen directory prefix, so
//! the UI can show a folder-comparison view (SmartGit-style) with per-directory
//! rolled-up line counts. Pure and backend-agnostic: it depends only on
//! [`crate::domain`] types and the standard library, which lets it be unit-tested
//! and shipped before the `worktree-state` / UI layers are unfrozen.
//!
//! The request descriptor that ties this to a repo identity and a base/target
//! ref ([`crate::domain`] has no `RepoId`/`Refish`) lives in `worktree-state`
//! and is built in task T-B; this module owns only the aggregation model.

use crate::domain::{CommitFileChange, FileStatusKind};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Whether a [`DirectoryNode`] represents a file or a directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryNodeKind {
    File,
    Directory,
}

/// A node in a directory-comparison tree.
///
/// File nodes carry their own line counts and `file_count == 1`. Directory
/// nodes have `file_count`/`additions`/`deletions` equal to the sum of their
/// (recursive) children, computed bottom-up by [`aggregate_to_tree`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryNode {
    /// Last path component (the file or directory name).
    pub name: String,
    /// Full repo-relative path of this node (file path, or directory path).
    pub path: PathBuf,
    pub kind: DirectoryNodeKind,
    /// Change kind for file nodes; `None` for directory nodes.
    pub change_kind: Option<FileStatusKind>,
    /// Rolled-up added line count. `None` stats in descendant files count as 0.
    pub additions: u64,
    /// Rolled-up removed line count. `None` stats count as 0.
    pub deletions: u64,
    /// Number of files in this subtree (0 for a lone file node is `1`).
    pub file_count: u64,
    pub children: Vec<DirectoryNode>,
}

/// The result of aggregating a diff to a directory tree, rooted at `root`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectoryDiffResult {
    pub root: DirectoryNode,
}

impl DirectoryDiffResult {
    /// Build the result directly from a flat change list and the comparison root.
    pub fn new(changes: &[CommitFileChange], root: &Path) -> Self {
        Self {
            root: aggregate_to_tree(changes, root),
        }
    }
}

/// Build a [`DirectoryNode`] tree rooted at `root` from a flat list of file
/// changes. Changes whose path does not lie under `root` are skipped.
///
/// `additions`/`deletions` may be `None` (binary files, submodules, or commits
/// too large to stat per file); those contribute 0 to the rolled-up totals.
/// Children are sorted directories-first then alphabetically for stable UI.
pub fn aggregate_to_tree(changes: &[CommitFileChange], root: &Path) -> DirectoryNode {
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ".".to_string());

    let mut root_node = DirectoryNode {
        name: root_name,
        path: root.to_path_buf(),
        kind: DirectoryNodeKind::Directory,
        change_kind: None,
        additions: 0,
        deletions: 0,
        file_count: 0,
        children: Vec::new(),
    };

    for change in changes {
        let Ok(rel) = change.path.strip_prefix(root) else {
            continue;
        };
        let comps: Vec<&OsStr> = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s),
                _ => None,
            })
            .collect();
        // A file whose path equals `root` itself cannot occur (root is a dir);
        // skip it defensively.
        if comps.is_empty() {
            continue;
        }
        insert_into(&mut root_node, &comps, change);
    }

    finalize(&mut root_node);
    root_node
}

fn insert_into(parent: &mut DirectoryNode, comps: &[&OsStr], change: &CommitFileChange) {
    if comps.is_empty() {
        return;
    }
    if comps.len() == 1 {
        let name = comps[0].to_string_lossy().into_owned();
        parent.children.push(DirectoryNode {
            name,
            path: change.path.clone(),
            kind: DirectoryNodeKind::File,
            change_kind: Some(change.kind),
            additions: change.additions.unwrap_or(0) as u64,
            deletions: change.deletions.unwrap_or(0) as u64,
            file_count: 1,
            children: Vec::new(),
        });
        return;
    }

    let name = comps[0].to_string_lossy().into_owned();
    let dir_path = parent.path.join(&name);
    let child = find_or_create_dir(parent, &name, dir_path);
    insert_into(child, &comps[1..], change);
}

fn find_or_create_dir<'a>(
    parent: &'a mut DirectoryNode,
    name: &str,
    dir_path: PathBuf,
) -> &'a mut DirectoryNode {
    if let Some(pos) = parent
        .children
        .iter()
        .position(|c| c.kind == DirectoryNodeKind::Directory && c.name == name)
    {
        &mut parent.children[pos]
    } else {
        parent.children.push(DirectoryNode {
            name: name.to_string(),
            path: dir_path,
            kind: DirectoryNodeKind::Directory,
            change_kind: None,
            additions: 0,
            deletions: 0,
            file_count: 0,
            children: Vec::new(),
        });
        parent.children.last_mut().expect("just pushed")
    }
}

/// Sums children into their parent and sorts children (dirs first, then alpha).
fn finalize(node: &mut DirectoryNode) -> (u64, u64, u64) {
    if node.kind == DirectoryNodeKind::File {
        return (node.additions, node.deletions, node.file_count);
    }

    let mut add = 0u64;
    let mut del = 0u64;
    let mut files = 0u64;
    for child in &mut node.children {
        let (ca, cd, cf) = finalize(child);
        add += ca;
        del += cd;
        files += cf;
    }
    node.additions = add;
    node.deletions = del;
    node.file_count = files;

    node.children.sort_by(|a, b| match (a.kind, b.kind) {
        (DirectoryNodeKind::Directory, DirectoryNodeKind::Directory)
        | (DirectoryNodeKind::File, DirectoryNodeKind::File) => a.name.cmp(&b.name),
        (DirectoryNodeKind::Directory, DirectoryNodeKind::File) => std::cmp::Ordering::Less,
        (DirectoryNodeKind::File, DirectoryNodeKind::Directory) => std::cmp::Ordering::Greater,
    });

    (add, del, files)
}

/// Returns the subtree rooted at `prefix`, pruning every sibling subtree.
///
/// * `prefix` empty or equal to `root.path` → the whole tree (re-rooted at
///   `root.path`).
/// * `prefix` is a descendant → that subtree, re-rooted so its `path`/`name`
///   reflect `prefix` (what a drill-down view wants as its new display root).
/// * `prefix` matches nothing → an empty directory node at `prefix`.
pub fn filter_by_prefix(root: &DirectoryNode, prefix: &Path) -> DirectoryNode {
    if prefix.as_os_str().is_empty() || prefix == root.path {
        return root.clone();
    }
    let Ok(rel) = prefix.strip_prefix(&root.path) else {
        return empty_dir(prefix);
    };
    let comps: Vec<&OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    if comps.is_empty() {
        return root.clone();
    }
    match descend(root, &comps) {
        Some(sub) => {
            let mut sub = sub.clone();
            sub.path = prefix.to_path_buf();
            sub.name = prefix
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ".".to_string());
            sub
        }
        None => empty_dir(prefix),
    }
}

fn descend<'a>(node: &'a DirectoryNode, comps: &[&OsStr]) -> Option<&'a DirectoryNode> {
    if comps.is_empty() {
        return Some(node);
    }
    let head = comps[0].to_string_lossy();
    let child = node
        .children
        .iter()
        .find(|c| c.kind == DirectoryNodeKind::Directory && c.name == head)?;
    descend(child, &comps[1..])
}

fn empty_dir(prefix: &Path) -> DirectoryNode {
    DirectoryNode {
        name: prefix
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ".".to_string()),
        path: prefix.to_path_buf(),
        kind: DirectoryNodeKind::Directory,
        change_kind: None,
        additions: 0,
        deletions: 0,
        file_count: 0,
        children: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fc(
        path: &str,
        kind: FileStatusKind,
        add: Option<u32>,
        del: Option<u32>,
    ) -> CommitFileChange {
        CommitFileChange {
            path: PathBuf::from(path),
            kind,
            is_submodule: false,
            additions: add,
            deletions: del,
        }
    }

    fn sample_changes() -> Vec<CommitFileChange> {
        vec![
            fc("src/a.rs", FileStatusKind::Added, Some(10), Some(0)),
            fc("src/b.rs", FileStatusKind::Modified, Some(5), Some(3)),
            fc("src/utils/x.rs", FileStatusKind::Added, Some(2), Some(0)),
            fc("docs/readme.md", FileStatusKind::Modified, Some(1), Some(1)),
            // Binary file: stats are None and must count as 0.
            fc("assets/logo.png", FileStatusKind::Added, None, None),
        ]
    }

    #[test]
    fn aggregates_nested_totals() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        assert_eq!(root.file_count, 5);
        assert_eq!(root.additions, 18); // 10+5+2+1+0
        assert_eq!(root.deletions, 4); // 0+3+0+1+0
        assert_eq!(root.kind, DirectoryNodeKind::Directory);
        assert_eq!(root.change_kind, None);
    }

    #[test]
    fn rolls_up_per_directory() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));

        let src = root
            .children
            .iter()
            .find(|c| c.name == "src")
            .expect("src dir present");
        assert_eq!(src.kind, DirectoryNodeKind::Directory);
        assert_eq!(src.file_count, 3); // a.rs, b.rs, utils/x.rs
        assert_eq!(src.additions, 17); // 10+5+2
        assert_eq!(src.deletions, 3); // 0+3+0

        let utils = src
            .children
            .iter()
            .find(|c| c.name == "utils")
            .expect("utils dir present");
        assert_eq!(utils.file_count, 1);
        assert_eq!(utils.additions, 2);

        let docs = root
            .children
            .iter()
            .find(|c| c.name == "docs")
            .expect("docs dir present");
        assert_eq!(docs.file_count, 1);
        assert_eq!(docs.additions, 1);
        assert_eq!(docs.deletions, 1);
    }

    #[test]
    fn file_nodes_carry_their_own_stats() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        let src = root.children.iter().find(|c| c.name == "src").unwrap();
        // children sorted dirs-first: "utils" then files "a.rs", "b.rs".
        let a = src
            .children
            .iter()
            .find(|c| c.name == "a.rs")
            .expect("a.rs present");
        assert_eq!(a.kind, DirectoryNodeKind::File);
        assert_eq!(a.change_kind, Some(FileStatusKind::Added));
        assert_eq!(a.additions, 10);
        assert_eq!(a.file_count, 1);
    }

    #[test]
    fn binary_file_counts_as_zero() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        let assets = root.children.iter().find(|c| c.name == "assets").unwrap();
        assert_eq!(assets.file_count, 1);
        assert_eq!(assets.additions, 0);
        assert_eq!(assets.deletions, 0);
    }

    #[test]
    fn skips_changes_outside_root() {
        let changes = vec![
            fc("src/a.rs", FileStatusKind::Added, Some(10), Some(0)),
            fc("docs/readme.md", FileStatusKind::Modified, Some(1), Some(1)),
        ];
        let root = aggregate_to_tree(&changes, Path::new("src"));
        // Only "src/a.rs" is under root "src"; "docs/readme.md" is skipped.
        assert_eq!(root.file_count, 1);
        assert_eq!(root.additions, 10);
        assert!(root.children.iter().any(|c| c.name == "a.rs"));
        assert!(!root.children.iter().any(|c| c.name == "docs"));
    }

    #[test]
    fn filter_returns_subtree_rerooted() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        let src = filter_by_prefix(&root, Path::new("src"));
        assert_eq!(src.path, Path::new("src"));
        assert_eq!(src.name, "src");
        assert_eq!(src.kind, DirectoryNodeKind::Directory);
        assert_eq!(src.file_count, 3);
        assert_eq!(src.additions, 17);
        assert_eq!(src.deletions, 3);
    }

    #[test]
    fn filter_descends_to_leaf_directory() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        let utils = filter_by_prefix(&root, Path::new("src/utils"));
        assert_eq!(utils.path, Path::new("src/utils"));
        assert_eq!(utils.file_count, 1);
        assert_eq!(utils.additions, 2);
        assert!(
            utils
                .children
                .iter()
                .any(|c| c.name == "x.rs" && c.kind == DirectoryNodeKind::File)
        );
    }

    #[test]
    fn filter_empty_prefix_returns_whole_tree() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        let same = filter_by_prefix(&root, Path::new(""));
        assert_eq!(same.file_count, 5);
    }

    #[test]
    fn filter_missing_prefix_returns_empty_dir() {
        let root = aggregate_to_tree(&sample_changes(), Path::new(""));
        let missing = filter_by_prefix(&root, Path::new("does/not/exist"));
        assert_eq!(missing.path, Path::new("does/not/exist"));
        assert_eq!(missing.file_count, 0);
        assert!(missing.children.is_empty());
    }

    #[test]
    fn build_result_wraps_root() {
        let result = DirectoryDiffResult::new(&sample_changes(), Path::new(""));
        assert_eq!(result.root.file_count, 5);
    }

    #[test]
    fn aggregates_many_files_flat_under_one_directory() {
        // A directory far larger than COMMIT_STATS_MAX_FILES (400, in
        // worktree-git-gix) — proves the O(n) aggregation stays correct and
        // cheap at scale. The perf *tier* (pathspect `root/` + T6 virtualization)
        // is built in T-B/T6; this guards the pure aggregation itself.
        const N: usize = 2_000;
        let mut changes = Vec::with_capacity(N);
        let mut total_add = 0u64;
        for i in 0..N {
            let add = (i % 7) as u32; // varying, some 0
            total_add += add as u64;
            changes.push(fc(
                &format!("src/gen_{i}.rs"),
                FileStatusKind::Modified,
                Some(add),
                Some((i % 3) as u32),
            ));
        }

        let start = std::time::Instant::now();
        let root = aggregate_to_tree(&changes, Path::new(""));
        let elapsed = start.elapsed();
        eprintln!("aggregate_to_tree over {N} files took {elapsed:?}");

        assert_eq!(root.file_count, N as u64);
        assert_eq!(root.additions, total_add);
        assert_eq!(root.deletions, (0..N).map(|i| (i % 3) as u64).sum::<u64>());

        let src = root.children.iter().find(|c| c.name == "src").unwrap();
        assert_eq!(src.file_count, N as u64);
        assert_eq!(src.additions, total_add);
        // Flat: exactly N file children, no sub-directories.
        assert_eq!(src.children.len(), N);
        assert!(
            src.children
                .iter()
                .all(|c| c.kind == DirectoryNodeKind::File)
        );
    }

    #[test]
    fn aggregates_deeply_nested_single_file() {
        // 100-level deep path: must not overflow the stack and must count as 1 file.
        let mut deep = String::new();
        for _ in 0..99 {
            deep.push_str("a/");
        }
        deep.push_str("leaf.rs");
        let changes = vec![fc(&deep, FileStatusKind::Added, Some(42), Some(7))];
        let root = aggregate_to_tree(&changes, Path::new(""));

        // Walk down to confirm the structure exists at every level.
        // 99 directory "a" nodes + leaf.rs = 100 components, so descend 100 times.
        let mut node = &root;
        for depth in 0..100 {
            assert_eq!(node.file_count, 1, "depth {depth}");
            if depth < 100 {
                node = node.children.first().expect("directory level present");
            }
        }
        assert_eq!(node.name, "leaf.rs");
        assert_eq!(node.kind, DirectoryNodeKind::File);
        assert_eq!(node.additions, 42);

        // Drill into the middle via filter_by_prefix.
        let mut mid = String::new();
        for _ in 0..50 {
            mid.push_str("a/");
        }
        let mid = mid.trim_end_matches('/');
        let filtered = filter_by_prefix(&root, Path::new(mid));
        assert_eq!(filtered.file_count, 1);
        assert_eq!(filtered.additions, 42);
    }

    #[test]
    fn filter_prunes_sibling_subtrees_on_large_tree() {
        let mut changes = Vec::new();
        for i in 0..500 {
            changes.push(fc(
                &format!("keep/a_{i}.rs"),
                FileStatusKind::Added,
                Some(1),
                Some(0),
            ));
            changes.push(fc(
                &format!("drop/b_{i}.rs"),
                FileStatusKind::Modified,
                Some(2),
                Some(1),
            ));
        }
        let root = aggregate_to_tree(&changes, Path::new(""));
        assert_eq!(root.file_count, 1000);

        let kept = filter_by_prefix(&root, Path::new("keep"));
        assert_eq!(kept.path, Path::new("keep"));
        assert_eq!(kept.file_count, 500);
        assert_eq!(kept.additions, 500);
        // The dropped sibling must be gone entirely.
        assert!(!kept.children.iter().any(|c| c.name == "drop"));
        assert!(kept.children.iter().all(|c| c.name.starts_with('a')));
    }

    #[test]
    fn handles_paths_with_spaces() {
        let changes = vec![
            fc(
                "my notes/file a.rs",
                FileStatusKind::Added,
                Some(3),
                Some(0),
            ),
            fc(
                "my notes/file b.rs",
                FileStatusKind::Modified,
                Some(1),
                Some(2),
            ),
            fc("src/a b.rs", FileStatusKind::Added, Some(5), Some(0)),
        ];
        let root = aggregate_to_tree(&changes, Path::new(""));
        // Spaces are part of component names, never separators.
        let notes = root.children.iter().find(|c| c.name == "my notes").unwrap();
        assert_eq!(notes.kind, DirectoryNodeKind::Directory);
        assert_eq!(notes.file_count, 2);
        assert_eq!(notes.additions, 4);
        let src = root.children.iter().find(|c| c.name == "src").unwrap();
        assert_eq!(src.file_count, 1);
        assert_eq!(src.additions, 5);
    }

    #[test]
    fn handles_non_ascii_filenames() {
        let changes = vec![
            fc("源/文件.rs", FileStatusKind::Added, Some(7), Some(0)),
            fc("src/中文.md", FileStatusKind::Modified, Some(2), Some(1)),
        ];
        let root = aggregate_to_tree(&changes, Path::new(""));
        let src = root.children.iter().find(|c| c.name == "src").unwrap();
        assert_eq!(src.file_count, 1);
        assert_eq!(src.additions, 2);
        assert!(root.children.iter().any(|c| c.name == "源"));
    }

    #[test]
    fn handles_platform_path_separator() {
        // Use the host's main separator so the test is meaningful on both
        // Windows (backslash) and Unix (slash).
        let sep = std::path::MAIN_SEPARATOR;
        let changes = vec![
            fc(
                &format!("src{sep}a.rs"),
                FileStatusKind::Added,
                Some(4),
                Some(0),
            ),
            fc(
                &format!("src{sep}sub{sep}b.rs"),
                FileStatusKind::Modified,
                Some(1),
                Some(1),
            ),
        ];
        let root = aggregate_to_tree(&changes, Path::new(""));
        let src = root.children.iter().find(|c| c.name == "src").unwrap();
        assert_eq!(src.file_count, 2);
        assert_eq!(src.additions, 5);
    }

    #[test]
    fn treats_case_distinct_paths_as_separate_files() {
        // Git paths are byte-distinct; a case-only difference is two files.
        let changes = vec![
            fc("src/A.rs", FileStatusKind::Added, Some(3), Some(0)),
            fc("src/a.rs", FileStatusKind::Modified, Some(1), Some(1)),
        ];
        let root = aggregate_to_tree(&changes, Path::new(""));
        let src = root.children.iter().find(|c| c.name == "src").unwrap();
        assert_eq!(src.file_count, 2);
        assert_eq!(src.additions, 4);
        assert_eq!(src.children.len(), 2);
    }
}
