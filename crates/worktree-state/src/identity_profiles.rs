//! Named commit-identity profiles: a small, app-level preset store that lets
//! a user switch between, say, a work identity and an open-source identity
//! across repositories without retyping `user.name`/`user.email`/`commit.gpgsign`.
//!
//! Persisted as `identity_profiles.json` in the same app state dir as
//! `session.json`, written atomically under the session-file persist lock so a
//! profile save and a session save can never interleave into a torn file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A named commit-identity preset. The fields mirror exactly what the
/// per-repository settings popover already edits in a repo's local git config,
/// so applying a profile is just copying these values into that popover's
/// draft and reusing its existing Apply path.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdentityProfile {
    /// The user's label for the preset (may contain spaces/punctuation). This
    /// is the join key within the set, not a git config key.
    pub name: String,
    pub user_name: String,
    pub user_email: String,
    /// Tri-state signing parallel to `RepoSettingsDraft::sign_commits`: `None`
    /// leaves the target repository inheriting its global `commit.gpgsign`.
    #[serde(default)]
    pub sign_commits: Option<bool>,
}

/// The full set of saved identity profiles. A `Vec` (not a map) keeps ordering
/// stable and user-controlled; `name` is the unique key.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IdentityProfiles {
    #[serde(default)]
    pub profiles: Vec<IdentityProfile>,
}

impl IdentityProfiles {
    /// Insert `profile`, replacing any existing entry with the same `name`.
    /// Returns `true` when an entry was replaced rather than appended.
    pub fn upsert(&mut self, profile: IdentityProfile) -> bool {
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile;
            true
        } else {
            self.profiles.push(profile);
            false
        }
    }
}

/// Where the profiles file lives, next to `session.json`. Returns `None` when
/// the app has no state dir — tests and persistence-disabled runs — which the
/// public loaders treat as "no profiles" and the persister treats as a no-op.
fn identity_profiles_file_path() -> Option<PathBuf> {
    if cfg!(test) || crate::session::running_under_test_harness() {
        return None;
    }
    crate::session::app_state_dir().map(|dir| dir.join("identity_profiles.json"))
}

/// Load the saved profiles, or an empty set when the file is absent or
/// unreadable. A corrupt file degrades to no profiles rather than erroring,
/// because losing presets is recoverable while a panic on startup is not.
pub fn load_identity_profiles() -> IdentityProfiles {
    match identity_profiles_file_path() {
        Some(path) => load_identity_profiles_from_path(&path),
        None => IdentityProfiles::default(),
    }
}

/// Load from an explicit path (used by tests and any future importer).
pub fn load_identity_profiles_from_path(path: &Path) -> IdentityProfiles {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => IdentityProfiles::default(),
        Err(_) => IdentityProfiles::default(),
    }
}

/// Persist the profiles, or succeed no-op when there is no state dir.
pub fn persist_identity_profiles(profiles: &IdentityProfiles) -> std::io::Result<()> {
    match identity_profiles_file_path() {
        Some(path) => persist_identity_profiles_to_path(&path, profiles),
        None => Ok(()),
    }
}

/// Persist to an explicit path, atomic and lock-guarded like the session file.
pub fn persist_identity_profiles_to_path(
    path: &Path,
    profiles: &IdentityProfiles,
) -> std::io::Result<()> {
    let _guard = crate::session::session_file_persist_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    crate::session::persist_to_path(path, profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_replaces_by_name_and_appends_otherwise() {
        let mut set = IdentityProfiles::default();
        assert!(!set.upsert(IdentityProfile {
            name: "work".into(),
            user_name: "A".into(),
            user_email: "a@w".into(),
            sign_commits: Some(true),
        }));
        assert_eq!(set.profiles.len(), 1);
        // Same name replaces in place.
        assert!(set.upsert(IdentityProfile {
            name: "work".into(),
            user_name: "A2".into(),
            user_email: "a2@w".into(),
            sign_commits: None,
        }));
        assert_eq!(set.profiles.len(), 1);
        assert_eq!(set.profiles[0].user_name, "A2");
        // Different name appends.
        assert!(!set.upsert(IdentityProfile {
            name: "oss".into(),
            user_name: "B".into(),
            user_email: "b@o".into(),
            sign_commits: Some(false),
        }));
        assert_eq!(set.profiles.len(), 2);
    }

    #[test]
    fn roundtrip_persist_and_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("wt-iprofiles-{}", std::process::id()));
        let path = dir.join("identity_profiles.json");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(&path);

        assert!(load_identity_profiles_from_path(&path).profiles.is_empty());

        let mut set = IdentityProfiles::default();
        set.upsert(IdentityProfile {
            name: "work".into(),
            user_name: "A".into(),
            user_email: "a@w".into(),
            sign_commits: Some(true),
        });
        persist_identity_profiles_to_path(&path, &set).expect("persist");

        let loaded = load_identity_profiles_from_path(&path);
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].sign_commits, Some(true));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_file_degrades_to_empty() {
        let dir = std::env::temp_dir().join(format!("wt-iprofiles-corrupt-{}", std::process::id()));
        let path = dir.join("identity_profiles.json");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(&path, "not json{").expect("write");
        assert!(load_identity_profiles_from_path(&path).profiles.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
