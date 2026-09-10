use super::GixRepo;
use crate::util::{
    git_command_timeout, run_git_raw_output, run_git_with_output, run_git_with_stdin_capture,
};
use std::path::Path;
use worktree_core::domain::{DiffTarget, LfsPointer, LfsPointerChange};
use worktree_core::services::{CommandOutput, Result};

impl GixRepo {
    /// Whether the repository has LFS wiring installed. `git lfs install`
    /// writes the pre-push hook, so its presence with the marker command is
    /// the cheapest reliable repo-level signal. Hooks resolve from the
    /// common dir so linked worktrees see the main repository's hooks, which
    /// is also how git itself resolves them.
    pub(super) fn lfs_enabled(&self) -> Result<bool> {
        let common_dir = self._repo.to_thread_local().common_dir().to_path_buf();
        let hook = common_dir.join("hooks").join("pre-push");
        let Ok(text) = std::fs::read_to_string(hook) else {
            return Ok(false);
        };
        Ok(text.contains("git lfs pre-push"))
    }

    /// Whether `path` carries a `filter=lfs` attribute from `.gitattributes`.
    /// `git check-attr -z filter -- <path>` prints one
    /// `<path>\0<attr>\0<value>\0` triplet; the attribute is in effect only
    /// when the value is exactly `lfs` (not `unspecified`/`set`/`unset`).
    pub(super) fn lfs_is_filtered(&self, path: &Path) -> Result<bool> {
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("check-attr")
            .arg("-z")
            .arg("filter")
            .arg("--")
            .arg(path);
        let output = run_git_raw_output(cmd, "git check-attr")?;
        Ok(check_attr_output_has_lfs_filter(&output.stdout))
    }

    /// Old/new LFS pointers for the diff `target`. The unified diff of an
    /// LFS-tracked path shows pointer-file hunks (git cleans the worktree
    /// side through the filter before comparing), so the same diff command
    /// the text view runs is reused here and only the parsing differs.
    pub(super) fn lfs_pointer_change(
        &self,
        target: &DiffTarget,
    ) -> Result<Option<LfsPointerChange>> {
        let text = self.run_unified_diff(self.build_unified_diff_command(target))?;
        Ok(parse_pointer_change(&text))
    }

    /// Turn LFS pointer bytes into the actual content through
    /// `git lfs smudge`. The smudge filter reads a pointer on stdin and
    /// writes the content (or a pass-through copy for non-pointer input) to
    /// stdout.
    pub(super) fn lfs_smudge_bytes(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("lfs").arg("smudge");
        run_git_with_stdin_capture(
            cmd,
            input.to_vec(),
            "git lfs smudge",
            git_command_timeout(),
            None,
        )
    }

    /// `git gc`, then `git lfs prune` when LFS is enabled — the combined
    /// cleanup the C# version's "Cleanup (GC)" action runs. A prune failure
    /// is folded into the output instead of failing the command: pruning
    /// wants the LFS storage reachable, and an offline machine must not lose
    /// the successful gc along with it.
    pub(super) fn cleanup_with_output(&self) -> Result<CommandOutput> {
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("gc");
        let mut output = run_git_with_output(cmd, "git gc")?;

        if self.lfs_enabled().unwrap_or(false) {
            let mut cmd = self.git_workdir_cmd();
            cmd.arg("lfs").arg("prune");
            match run_git_with_output(cmd, "git lfs prune") {
                Ok(prune) => {
                    output.stdout.push_str(&prune.stdout);
                    output.stderr.push_str(&prune.stderr);
                }
                Err(err) => {
                    output.stderr.push_str(&format!("git lfs prune: {err}\n"));
                }
            }
        }

        Ok(output)
    }
}

/// Parse `git check-attr -z` output, answering whether the `filter`
/// attribute is set to `lfs` for the queried path. Output is a sequence of
/// NUL-terminated `<path>`, `<attr>`, `<value>` triplets; a trailing
/// incomplete triplet is tolerated (it cannot contain a full `filter=lfs`
/// match anyway).
fn check_attr_output_has_lfs_filter(stdout: &[u8]) -> bool {
    let mut parts = stdout.split(|byte| *byte == 0).map(|part| {
        // check-attr prints the path it was given, quoted per
        // core.quotepath; the attribute name and value are never quoted.
        String::from_utf8_lossy(part)
    });
    while let (Some(_path), Some(attr), Some(value)) = (parts.next(), parts.next(), parts.next()) {
        if attr == "filter" && value == "lfs" {
            return true;
        }
    }
    false
}

/// Parse old/new LFS pointers out of a unified diff of pointer files.
///
/// Pointer headers change as `-oid sha256:…` / `+oid sha256:…` and
/// `-size <n>` / `+size <n>` lines; an unchanged ` size <n>` context line
/// carries the size both sides share when only the oid moved. Returns
/// `None` when no pointer header changed — either the path is not
/// LFS-filtered, or the pointer did not move.
fn parse_pointer_change(diff_text: &str) -> Option<LfsPointerChange> {
    let mut old = LfsPointer::default();
    let mut new = LfsPointer::default();
    let mut saw_old = false;
    let mut saw_new = false;

    for line in diff_text.lines() {
        let (sign, text) = match line.as_bytes().first() {
            Some(b'-') => ('-', &line[1..]),
            Some(b'+') => ('+', &line[1..]),
            Some(b' ') => (' ', &line[1..]),
            _ => continue,
        };
        if let Some(oid) = text.strip_prefix("oid sha256:") {
            match sign {
                '-' => {
                    old.oid = Some(oid.to_string());
                    saw_old = true;
                }
                '+' => {
                    new.oid = Some(oid.to_string());
                    saw_new = true;
                }
                ' ' => {}
                _ => unreachable!("sign is one of the three matched bytes"),
            }
        } else if let Some(size) = text.strip_prefix("size ") {
            let Ok(size) = size.trim().parse::<u64>() else {
                continue;
            };
            match sign {
                '-' => {
                    old.size = Some(size);
                    saw_old = true;
                }
                '+' => {
                    new.size = Some(size);
                    saw_new = true;
                }
                // An unchanged size line is the only place the untouched
                // side's size appears in the hunks.
                ' ' => {
                    old.size = Some(size);
                    new.size = Some(size);
                }
                _ => unreachable!("sign is one of the three matched bytes"),
            }
        }
    }

    (saw_old || saw_new).then(|| LfsPointerChange {
        old: saw_old.then_some(old),
        new: saw_new.then_some(new),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_attr_output_detects_lfs_filter_value() {
        let triplet = |path: &str, attr: &str, value: &str| format!("{path}\0{attr}\0{value}\0");

        assert!(check_attr_output_has_lfs_filter(
            triplet("art.bin", "filter", "lfs").as_bytes()
        ));
        // Multiple attributes per call, lfs among them.
        assert!(check_attr_output_has_lfs_filter(
            format!(
                "{}{}",
                triplet("art.bin", "diff", "lfs"),
                triplet("art.bin", "filter", "lfs"),
            )
            .as_bytes()
        ));
        assert!(!check_attr_output_has_lfs_filter(
            triplet("art.bin", "filter", "unspecified").as_bytes()
        ));
        assert!(!check_attr_output_has_lfs_filter(
            triplet("art.bin", "filter", "set").as_bytes()
        ));
        assert!(!check_attr_output_has_lfs_filter(
            triplet("art.bin", "text", "auto").as_bytes()
        ));
        assert!(!check_attr_output_has_lfs_filter(b""));
        // Truncated output cannot contain a full triplet.
        assert!(!check_attr_output_has_lfs_filter(b"art.bin\0filter\0"));
    }

    #[test]
    fn pointer_change_parses_a_typical_content_update() {
        let diff = "\
diff --git a/art.bin b/art.bin
index aaa..bbb 100644
--- a/art.bin
+++ b/art.bin
@@ -1,3 +1,3 @@
 version https://git-lfs.github.com/spec/v1
-oid sha256:1111111111111111111111111111111111111111111111111111111111111111
-size 4096
+oid sha256:2222222222222222222222222222222222222222222222222222222222222222
+size 8192
";
        let change = parse_pointer_change(diff).expect("pointer change");
        assert_eq!(
            change.old.as_ref().and_then(|p| p.oid.as_deref()),
            Some("1111111111111111111111111111111111111111111111111111111111111111")
        );
        assert_eq!(change.old.as_ref().and_then(|p| p.size), Some(4096));
        assert_eq!(
            change.new.as_ref().and_then(|p| p.oid.as_deref()),
            Some("2222222222222222222222222222222222222222222222222222222222222222")
        );
        assert_eq!(change.new.as_ref().and_then(|p| p.size), Some(8192));
    }

    #[test]
    fn pointer_change_parses_added_file_with_only_new_side() {
        let diff = "\
diff --git a/art.bin b/art.bin
new file mode 100644
--- /dev/null
+++ b/art.bin
@@ -0,0 +1,3 @@
+version https://git-lfs.github.com/spec/v1
+oid sha256:abcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabcabca
+size 123
";
        let change = parse_pointer_change(diff).expect("pointer change");
        assert!(change.old.is_none());
        assert_eq!(change.new.as_ref().and_then(|p| p.size), Some(123));
    }

    #[test]
    fn pointer_change_parses_deleted_file_with_only_old_side() {
        let diff = "\
diff --git a/art.bin b/art.bin
deleted file mode 100644
--- a/art.bin
+++ /dev/null
@@ -1,3 +0,0 @@
-version https://git-lfs.github.com/spec/v1
-oid sha256:defdefdefdefdefdefdefdefdefdefdefdefdefdefdefdefdefdefdefdefdefd
-size 99
";
        let change = parse_pointer_change(diff).expect("pointer change");
        assert!(change.new.is_none());
        assert_eq!(change.old.as_ref().and_then(|p| p.size), Some(99));
    }

    #[test]
    fn pointer_change_ignores_ordinary_text_diffs() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!(\"old\");
+    println!(\"new\");
 }
";
        assert!(parse_pointer_change(diff).is_none());
        assert!(parse_pointer_change("").is_none());
    }

    #[test]
    fn pointer_change_reads_shared_size_from_context_line() {
        // Same-size content update: both `size` lines appear as one context
        // line, so the shared value fills both sides.
        let diff = "\
diff --git a/art.bin b/art.bin
--- a/art.bin
+++ b/art.bin
@@ -1,3 +1,3 @@
 version https://git-lfs.github.com/spec/v1
-oid sha256:1111111111111111111111111111111111111111111111111111111111111111
+oid sha256:2222222222222222222222222222222222222222222222222222222222222222
 size 2048
";
        let change = parse_pointer_change(diff).expect("pointer change");
        assert_eq!(change.old.as_ref().and_then(|p| p.size), Some(2048));
        assert_eq!(change.new.as_ref().and_then(|p| p.size), Some(2048));
    }
}
