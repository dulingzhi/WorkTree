//! The jj version whitelist.
//!
//! The CLI implementation's `-T` templates use keywords whose availability
//! and semantics move with jj releases (`normal_target`, `remote`,
//! `current_working_copy`, `bookmarks.join`), so the set of versions this
//! crate will drive is bounded: a hard floor at the oldest release the
//! templates are known to parse against, and a tested ceiling above which
//! the crate still runs but says so through the trace channel. A jj older
//! than the floor is refused at open time rather than mis-parsed.

use gitcomet_core::jj;

/// Oldest jj release the templates and command surface are validated
/// against. Below this, [`crate::JjCliRepository::open`] refuses the repo.
pub const MIN_SUPPORTED_JJ_VERSION: (u64, u64, u64) = (0, 25, 0);

/// Newest jj release exercised by the test suite. Newer versions are
/// allowed through — jj's template language has been append-only for the
/// keywords used here — but they are reported as untested via
/// [`gitcomet_core::jj::trace`].
pub const MAX_TESTED_JJ_VERSION: (u64, u64, u64) = (0, 50, 0);

/// Outcome of checking a jj version against the whitelist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JjVersionSupport {
    /// Within [min, max) — validated.
    Supported,
    /// At or above the tested ceiling — allowed, traced as untested.
    Untested,
    /// Below the floor — refused.
    Unsupported,
}

/// Classify a `(major, minor, patch)` tuple against the whitelist.
pub fn classify_jj_version(version: (u64, u64, u64)) -> JjVersionSupport {
    if version < MIN_SUPPORTED_JJ_VERSION {
        JjVersionSupport::Unsupported
    } else if version >= MAX_TESTED_JJ_VERSION {
        JjVersionSupport::Untested
    } else {
        JjVersionSupport::Supported
    }
}

/// A probed jj version together with its whitelist classification.
pub type ProbedJjVersion = (JjVersionSupport, (u64, u64, u64));

/// Run `jj --version` and classify the result. `Ok(None)` means jj is not
/// installed or its version line was unrecognizable; `Ok(Some(..))` carries
/// the classification for the parsed tuple.
pub fn probe_jj_version_support() -> Result<Option<ProbedJjVersion>, String> {
    let output = gitcomet_core::process::background_command("jj")
        .arg("--version")
        .output()
        .map_err(|err| format!("spawn `jj --version`: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "`jj --version` failed: {}",
            String::from_utf8_lossy(if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            })
            .trim()
        ));
    }
    let version_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let Some(version) = jj::parse_jj_version(&version_output) else {
        // Not a hard error: the caller decides how to treat an
        // unrecognizable version line.
        return Ok(None);
    };
    Ok(Some((classify_jj_version(version), version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_below_the_floor_are_unsupported() {
        assert_eq!(
            classify_jj_version((0, 24, 9)),
            JjVersionSupport::Unsupported
        );
        assert_eq!(
            classify_jj_version((0, 24, 0)),
            JjVersionSupport::Unsupported
        );
    }

    #[test]
    fn the_floor_itself_is_supported() {
        assert_eq!(classify_jj_version((0, 25, 0)), JjVersionSupport::Supported);
    }

    #[test]
    fn the_locally_validated_version_is_supported() {
        // jj 0.44.0 is the release the templates were validated against.
        assert_eq!(classify_jj_version((0, 44, 0)), JjVersionSupport::Supported);
    }

    #[test]
    fn versions_at_or_above_the_ceiling_are_untested_but_allowed() {
        assert_eq!(classify_jj_version((0, 50, 0)), JjVersionSupport::Untested);
        assert_eq!(classify_jj_version((1, 0, 0)), JjVersionSupport::Untested);
    }
}
