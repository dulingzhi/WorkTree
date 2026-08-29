use super::GixRepo;
use crate::util::{run_git_raw_output, run_git_simple};
use std::path::{Path, PathBuf};
use worktree_core::services::Result;

impl GixRepo {
    /// Paths marked assume-unchanged in the index. `git ls-files -v` prefixes
    /// every entry with a tag letter; git lowercases the tag exactly when the
    /// assume-unchanged bit is set, so `h <path>` marks an unchanged-assumed
    /// regular file. (`S` — skip-worktree — is a different mechanism and is
    /// deliberately not mixed in.)
    pub(super) fn assume_unchanged_list_impl(&self) -> Result<Vec<PathBuf>> {
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("ls-files").arg("-v");
        let output = run_git_raw_output(cmd, "git ls-files")?;
        Ok(parse_ls_files_tagged(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    /// Mark or unmark `path` assume-unchanged. The flag lives in the index
    /// only, so the worktree file itself is never touched.
    pub(super) fn set_assume_unchanged_impl(&self, path: &Path, enable: bool) -> Result<()> {
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("update-index");
        if enable {
            cmd.arg("--assume-unchanged");
        } else {
            cmd.arg("--no-assume-unchanged");
        }
        cmd.arg("--").arg(path);
        run_git_simple(cmd, "git update-index")
    }
}

/// Collect `<path>` for every entry whose `-v` tag is lowercase — the
/// assume-unchanged marker. Anything unparseable (empty lines, missing tag)
/// is skipped: it cannot represent a marked file.
fn parse_ls_files_tagged(stdout: &str) -> Vec<PathBuf> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix('h')?.strip_prefix(' ')?;
            let path = rest.strip_suffix("\r").unwrap_or(rest);
            (!path.is_empty()).then(|| PathBuf::from(path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_h_tag_entries_are_collected() {
        let stdout = "\
H normal.txt
h assumed.txt
M modified.txt
h another.bin
";
        let paths = parse_ls_files_tagged(stdout);
        assert_eq!(
            paths,
            vec![PathBuf::from("assumed.txt"), PathBuf::from("another.bin")]
        );
    }

    #[test]
    fn malformed_lines_are_skipped() {
        assert!(parse_ls_files_tagged("").is_empty());
        assert!(parse_ls_files_tagged("h\n").is_empty());
        assert!(parse_ls_files_tagged("no-tag\n").is_empty());
        // CRLF output must not leak the carriage return into the path.
        assert_eq!(
            parse_ls_files_tagged("h win.txt\r\n"),
            vec![PathBuf::from("win.txt")]
        );
    }
}
