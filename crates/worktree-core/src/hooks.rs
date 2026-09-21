//! Native management of a repository's `.git/hooks` directory.
//!
//! These are plain `std::fs` helpers — no `gix` dependency, no `GitRepository*`
//! trait methods — so the state layer can call them from a `spawn_with_repo`
//! closure that only holds `Arc<dyn GitRepository>` (it exposes `RepoSpec.workdir`).
//! This keeps feature 4 free of any trait-signature change.

use crate::domain::{RepoHook, RepoHookList, RepoHookName};
use crate::path_utils::git_dir_for_workdir;
use std::path::{Path, PathBuf};

/// Resolve the on-disk `hooks/` directory for a repository workdir.
///
/// `git_dir_for_workdir` returns the directory gix opens: the workdir for a
/// normal clone, or `<workdir>/.git` for a `.git`-named repo. The real git
/// directory (where `hooks/` lives) is that result's `.git` subdir when it
/// exists, otherwise the result itself (bare / `.git`-named repo). Linked
/// worktrees (whose `.git` is a `gitdir:` pointer) resolve to the workdir's
/// `.git` and may surface the wrong hooks dir — an accepted v1 limitation.
fn hooks_dir_for_workdir(workdir: &Path) -> PathBuf {
    let base = git_dir_for_workdir(workdir);
    let git_dir = if base.join(".git").is_dir() {
        base.join(".git")
    } else {
        base
    };
    git_dir.join("hooks")
}

/// Whether `path` is runnable as a git hook: on unix that means the executable
/// bit is set; on Windows we approximate it with "not read-only" (git-for-windows
/// hook execution differs from the POSIX exec-bit model, see plan §6 D-H3).
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| !m.permissions().readonly())
        .unwrap_or(false)
}

/// Set or clear the executable bit / read-only flag for `path`.
#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    let mode = perms.mode();
    let new_mode = if executable {
        mode | 0o111
    } else {
        mode & !0o111
    };
    perms.set_mode(new_mode);
    std::fs::set_permissions(path, perms)
}

#[cfg(windows)]
fn set_executable(path: &Path, executable: bool) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_readonly(!executable);
    std::fs::set_permissions(path, perms)
}

/// Inspect a single hook file on disk: defined / enabled / has-sample.
fn inspect(hooks_dir: &Path, name: &str) -> (bool, bool, bool) {
    let path = hooks_dir.join(name);
    let defined = path.exists();
    let enabled = defined && is_executable(&path);
    let has_sample = hooks_dir.join(format!("{name}.sample")).exists();
    (defined, enabled, has_sample)
}

/// List every hook git would consider for `workdir`:
///
/// - the curated [`RepoHookName::STANDARD_NAMES`] set (always shown, even when
///   undefined), then
/// - any extra non-`.sample` file found in `.git/hooks` (user-defined hooks).
pub fn list_hooks(workdir: &Path) -> Result<RepoHookList, String> {
    let hooks_dir = hooks_dir_for_workdir(workdir);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut hooks = Vec::new();

    for name in RepoHookName::STANDARD_NAMES {
        let name = RepoHookName(name.to_string());
        let (defined, enabled, has_sample) = inspect(&hooks_dir, &name.0);
        hooks.push(RepoHook {
            name: name.clone(),
            defined,
            enabled,
            has_sample,
        });
        seen.insert(name.0.clone());
    }

    if let Ok(entries) = std::fs::read_dir(&hooks_dir) {
        for entry in entries.flatten() {
            let fname = match entry.file_name().to_str() {
                Some(s) => s.to_string(),
                None => continue,
            };
            if fname.ends_with(".sample") || seen.contains(&fname) {
                continue;
            }
            let (defined, enabled, has_sample) = inspect(&hooks_dir, &fname);
            hooks.push(RepoHook {
                name: RepoHookName(fname.clone()),
                defined,
                enabled,
                has_sample,
            });
            seen.insert(fname);
        }
    }

    Ok(RepoHookList(hooks))
}

/// Enable (`true`) or disable (`false`) a defined hook by toggling its
/// executable bit / read-only flag. Errors if the hook file does not exist —
/// create it first via [`create_hook`].
pub fn set_hook_enabled(workdir: &Path, name: &RepoHookName, enabled: bool) -> Result<(), String> {
    let path = hooks_dir_for_workdir(workdir).join(&name.0);
    if !path.exists() {
        return Err(format!(
            "hook '{}' is not defined; create it before enabling",
            name.0
        ));
    }
    set_executable(&path, enabled)
        .map_err(|e| format!("failed to set hook '{}' executable: {e}", name.0))
}

/// Create a hook file. When `from_sample` is true and `<name>.sample` exists,
/// the new hook copies the sample's contents; otherwise a `#!/bin/sh` skeleton is
/// written. The new file is made executable so git will run it.
pub fn create_hook(
    workdir: &Path,
    name: &RepoHookName,
    from_sample: bool,
) -> Result<PathBuf, String> {
    let hooks_dir = hooks_dir_for_workdir(workdir);
    std::fs::create_dir_all(&hooks_dir).map_err(|e| format!("failed to create hooks dir: {e}"))?;
    let path = hooks_dir.join(&name.0);
    if path.exists() {
        return Err(format!("hook '{}' already exists", name.0));
    }
    let content = if from_sample {
        let sample = hooks_dir.join(format!("{}.sample", name.0));
        if !sample.exists() {
            return Err(format!("no sample template for '{}'", name.0));
        }
        std::fs::read_to_string(&sample).map_err(|e| format!("failed to read sample: {e}"))?
    } else {
        "#!/bin/sh\n# Add your hook commands here.\n".to_string()
    };
    std::fs::write(&path, content).map_err(|e| format!("failed to write hook: {e}"))?;
    set_executable(&path, true).map_err(|e| format!("failed to make hook executable: {e}"))?;
    Ok(path)
}

/// Absolute path of a hook file, whether or not it currently exists.
///
/// The UI needs this to hand a hook script to the external editor
/// (`Msg::OpenFileEditor`) and should never re-derive the hooks directory
/// itself — `hooks_dir_for_workdir` carries the workdir/`.git` and bare-repo
/// resolution rules.
pub fn hook_path(workdir: &Path, name: &RepoHookName) -> PathBuf {
    hooks_dir_for_workdir(workdir).join(&name.0)
}

/// Delete a hook file. Errors if it does not exist.
pub fn delete_hook(workdir: &Path, name: &RepoHookName) -> Result<(), String> {
    let path = hooks_dir_for_workdir(workdir).join(&name.0);
    if !path.exists() {
        return Err(format!("hook '{}' does not exist", name.0));
    }
    std::fs::remove_file(&path).map_err(|e| format!("failed to delete hook: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Build a fake repo layout `<dir>/.git/hooks` so `hooks_dir_for_workdir`
    /// resolves correctly via the normal-clone path.
    fn fake_repo_hooks_dir(dir: &Path) -> PathBuf {
        let hooks = dir.join(".git").join("hooks");
        fs::create_dir_all(&hooks).expect("create .git/hooks");
        hooks
    }

    #[test]
    fn empty_repo_lists_only_standard_names_all_undefined() {
        let dir = tempdir().expect("temp dir");
        let list = list_hooks(dir.path()).expect("list_hooks");
        assert!(!list.0.is_empty());
        assert!(list.0.iter().all(|h| !h.defined));
        // Every curated standard name is present exactly once.
        for std in RepoHookName::STANDARD_NAMES {
            let count = list.0.iter().filter(|h| h.name.0 == *std).count();
            assert_eq!(count, 1, "standard hook {std} should appear once");
        }
    }

    #[test]
    fn defined_hook_with_exec_bit_is_enabled() {
        let dir = tempdir().expect("temp dir");
        let hooks = fake_repo_hooks_dir(dir.path());
        let path = hooks.join("pre-commit");
        fs::write(&path, "#!/bin/sh\n").expect("write hook");
        set_executable(&path, true).expect("chmod +x");

        let list = list_hooks(dir.path()).expect("list_hooks");
        let pre = list
            .0
            .iter()
            .find(|h| h.name.0 == "pre-commit")
            .expect("pre-commit");
        assert!(pre.defined);
        assert!(pre.enabled, "hook with exec bit must be enabled");
    }

    #[test]
    fn clearing_exec_bit_flips_enabled() {
        let dir = tempdir().expect("temp dir");
        let hooks = fake_repo_hooks_dir(dir.path());
        let path = hooks.join("pre-push");
        fs::write(&path, "#!/bin/sh\n").expect("write hook");
        set_executable(&path, true).expect("chmod +x");

        assert!(
            list_hooks(dir.path())
                .unwrap()
                .0
                .iter()
                .find(|h| h.name.0 == "pre-push")
                .unwrap()
                .enabled
        );

        set_hook_enabled(dir.path(), &RepoHookName::from("pre-push"), false).expect("disable");
        assert!(
            !list_hooks(dir.path())
                .unwrap()
                .0
                .iter()
                .find(|h| h.name.0 == "pre-push")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn set_hook_enabled_errors_when_undefined() {
        let dir = tempdir().expect("temp dir");
        let err = set_hook_enabled(dir.path(), &RepoHookName::from("pre-commit"), true);
        assert!(err.is_err());
    }

    #[test]
    fn create_from_sample_copies_contents() {
        let dir = tempdir().expect("temp dir");
        let hooks = fake_repo_hooks_dir(dir.path());
        fs::write(hooks.join("pre-commit.sample"), "SAMPLE BODY\n").expect("write sample");

        let created = create_hook(dir.path(), &RepoHookName::from("pre-commit"), true)
            .expect("create from sample");
        assert_eq!(fs::read_to_string(&created).unwrap(), "SAMPLE BODY\n");
        assert!(is_executable(&created));

        // Now list_hooks should report it defined + enabled, and has_sample
        // stays true because the original `<name>.sample` template is still there.
        let list = list_hooks(dir.path()).unwrap();
        let pre = list.0.iter().find(|h| h.name.0 == "pre-commit").unwrap();
        assert!(pre.defined && pre.enabled && pre.has_sample);
    }

    #[test]
    fn create_blank_writes_skeleton_and_is_enabled() {
        let dir = tempdir().expect("temp dir");
        let created = create_hook(dir.path(), &RepoHookName::from("post-commit"), false)
            .expect("create blank");
        let body = fs::read_to_string(&created).unwrap();
        assert!(body.starts_with("#!/bin/sh"));
        assert!(is_executable(&created));
    }

    #[test]
    fn delete_removes_defined_hook() {
        let dir = tempdir().expect("temp dir");
        let created =
            create_hook(dir.path(), &RepoHookName::from("commit-msg"), false).expect("create");
        assert!(created.exists());
        delete_hook(dir.path(), &RepoHookName::from("commit-msg")).expect("delete");
        assert!(!created.exists());
    }

    #[test]
    fn extra_non_sample_file_is_listed_as_user_defined_hook() {
        let dir = tempdir().expect("temp dir");
        let hooks = fake_repo_hooks_dir(dir.path());
        fs::write(hooks.join("my-custom-hook"), "#!/bin/sh\n").expect("write custom");
        set_executable(&hooks.join("my-custom-hook"), true).expect("chmod");

        let list = list_hooks(dir.path()).expect("list_hooks");
        let custom = list.0.iter().find(|h| h.name.0 == "my-custom-hook");
        assert!(custom.is_some(), "user-defined hook should appear");
        let custom = custom.unwrap();
        assert!(custom.defined && custom.enabled && !custom.has_sample);
    }

    #[test]
    fn hook_path_points_inside_the_hooks_directory() {
        let dir = tempdir().expect("temp dir");
        let hooks = fake_repo_hooks_dir(dir.path());
        let name = RepoHookName::from("pre-commit");
        let path = hook_path(dir.path(), &name);
        assert_eq!(path, hooks.join("pre-commit"));
        // Resolves even when the file does not exist yet — the UI asks for the
        // path before creating the hook.
        assert!(!path.exists());
    }
}
