use super::GixRepo;
use crate::util::{run_git_with_output, validate_ref_like_arg};
use worktree_core::services::{CommandOutput, Result};
use std::path::Path;

impl GixRepo {
    pub(super) fn archive_zip_with_output_impl(
        &self,
        revision: &str,
        dest: &Path,
    ) -> Result<CommandOutput> {
        validate_ref_like_arg(revision, "revision")?;
        let output_arg = format!("--output={}", dest.display());
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("archive")
            .arg("--format=zip")
            .arg(&output_arg)
            .arg(revision);
        run_git_with_output(
            cmd,
            &format!("git archive --format=zip {output_arg} {revision}"),
        )
    }
}
