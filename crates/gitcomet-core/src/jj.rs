//! Optional Jujutsu (`jj`) CLI runtime detection.
//!
//! Unlike Git, the `jj` executable is only needed for colocated Jujutsu
//! repositories, so probing must never block the UI thread or repo opening.
//! Detection follows the same stale-while-revalidate shape as the external
//! editor detection in `gitcomet-ui-gpui`: a probe runs on a background
//! thread, the result is cached process-wide with a TTL, and readers are
//! served the stale value while a revalidation is in flight.
//!
//! Set `GITCOMET_LOG_JJ=1` to log probe timings to stderr.

use std::ffi::OsString;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::process::background_command;

/// How long a completed `jj --version` probe stays fresh before the next read
/// triggers a background revalidation.
pub const JJ_PROBE_TTL: Duration = Duration::from_secs(300);

/// The `jj` program probed on the system PATH. A custom executable setting
/// can replace this later without changing the state machine.
const JJ_PROGRAM: &str = "jj";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JjRuntimeAvailability {
    Available {
        /// The program label that was probed (currently always `"jj"`).
        program: String,
        /// First line of `jj --version`, e.g. `"jj 0.25.0"`.
        version_output: String,
    },
    Unavailable {
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjRuntimeState {
    pub availability: JjRuntimeAvailability,
    probed_at: Instant,
}

impl JjRuntimeState {
    fn probed_now(availability: JjRuntimeAvailability) -> Self {
        Self {
            availability,
            probed_at: Instant::now(),
        }
    }

    /// A state with an artificially aged probe timestamp, for TTL tests.
    #[cfg(test)]
    fn probed_ago(availability: JjRuntimeAvailability, age: Duration) -> Self {
        Self {
            availability,
            probed_at: Instant::now()
                .checked_sub(age)
                .expect("test age must fit in an Instant"),
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self.availability, JjRuntimeAvailability::Available { .. })
    }

    pub fn version_output(&self) -> Option<&str> {
        match &self.availability {
            JjRuntimeAvailability::Available { version_output, .. } => {
                Some(version_output.as_str())
            }
            JjRuntimeAvailability::Unavailable { .. } => None,
        }
    }

    pub fn unavailable_detail(&self) -> Option<&str> {
        match &self.availability {
            JjRuntimeAvailability::Available { .. } => None,
            JjRuntimeAvailability::Unavailable { detail } => Some(detail.as_str()),
        }
    }

    fn is_fresh(&self) -> bool {
        self.probed_at.elapsed() < JJ_PROBE_TTL
    }
}

/// Parse the version tuple out of a `jj --version` line like `jj 0.25.0`.
/// Returns `None` unless the line starts with `jj ` and carries at least a
/// `major.minor` pair, so callers can whitelist known-good versions.
pub fn parse_jj_version(version_output: &str) -> Option<(u64, u64, u64)> {
    let rest = version_output.trim().strip_prefix("jj ")?;
    let mut numbers = rest.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next()?.parse().ok()?;
    let patch = numbers.next().unwrap_or("0");
    // Tolerate a leading patch number followed by build metadata (`0.25.0+abc`).
    let patch = patch.split(['+', '-']).next().unwrap_or("0");
    let patch = patch.trim().parse().unwrap_or(0);
    Some((major, minor, patch))
}

struct JjRuntimeSlot {
    state: Option<JjRuntimeState>,
    probe_running: bool,
}

fn jj_runtime_slot() -> &'static (Mutex<JjRuntimeSlot>, Condvar) {
    static SLOT: OnceLock<(Mutex<JjRuntimeSlot>, Condvar)> = OnceLock::new();
    SLOT.get_or_init(|| {
        (
            Mutex::new(JjRuntimeSlot {
                state: None,
                probe_running: false,
            }),
            Condvar::new(),
        )
    })
}

#[cfg(test)]
fn jj_runtime_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// Program override for deterministic tests; `None` probes the real `jj`.
#[cfg(test)]
fn jj_test_program() -> &'static Mutex<Option<OsString>> {
    static PROGRAM: OnceLock<Mutex<Option<OsString>>> = OnceLock::new();
    PROGRAM.get_or_init(|| Mutex::new(None))
}

fn jj_probe_log_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("GITCOMET_LOG_JJ").is_some())
}

fn jj_program() -> OsString {
    #[cfg(test)]
    if let Some(program) = jj_test_program()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
    {
        return program;
    }
    OsString::from(JJ_PROGRAM)
}

/// The cached probe result, if one has completed. Serve-stale: this may be
/// older than [`JJ_PROBE_TTL`] while a revalidation is in flight.
pub fn current_jj_runtime() -> Option<JjRuntimeState> {
    jj_runtime_slot()
        .0
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .state
        .clone()
}

/// Start a background probe unless a fresh result is cached or a probe is
/// already in flight. Safe to call from anywhere, including the UI thread.
pub fn warm_jj_runtime() {
    let should_probe = {
        let mut slot = jj_runtime_slot()
            .0
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let fresh = slot.state.as_ref().is_some_and(JjRuntimeState::is_fresh);
        if fresh || slot.probe_running {
            false
        } else {
            slot.probe_running = true;
            true
        }
    };
    if !should_probe {
        return;
    }

    std::thread::spawn(|| {
        let state = probe_jj_runtime();
        let mut slot = jj_runtime_slot()
            .0
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        slot.state = Some(state);
        slot.probe_running = false;
        jj_runtime_slot().1.notify_all();
    });
}

/// Wait for a usable probe result: returns the cached state immediately when
/// fresh, otherwise kicks off [`warm_jj_runtime`] and waits up to `timeout`
/// for it to finish. Falls back to a stale result when the deadline passes,
/// and to `None` when nothing has ever been probed.
pub fn ensure_jj_runtime(timeout: Duration) -> Option<JjRuntimeState> {
    let (lock, ready) = jj_runtime_slot();
    let slot = lock.lock().unwrap_or_else(|err| err.into_inner());
    if slot.state.as_ref().is_some_and(JjRuntimeState::is_fresh) {
        return slot.state.clone();
    }
    drop(slot);
    warm_jj_runtime();
    let mut slot = lock.lock().unwrap_or_else(|err| err.into_inner());
    let deadline = Instant::now() + timeout;
    while slot.probe_running && Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (guard, _timeout) = ready
            .wait_timeout(slot, remaining)
            .unwrap_or_else(|err| err.into_inner());
        slot = guard;
    }
    slot.state.clone()
}

/// Probe synchronously and replace the cached state. For callers that are
/// already on a background thread and want a guaranteed-fresh result (e.g.
/// a settings "re-check" button or the first colocated-repo open).
pub fn refresh_jj_runtime() -> JjRuntimeState {
    let state = probe_jj_runtime();
    let mut slot = jj_runtime_slot()
        .0
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    slot.state = Some(state.clone());
    slot.probe_running = false;
    jj_runtime_slot().1.notify_all();
    state
}

fn probe_jj_runtime() -> JjRuntimeState {
    let program = jj_program();
    let program_label = program.to_string_lossy().to_string();
    let log = jj_probe_log_enabled();
    if log {
        eprintln!("[gitcomet-jj] probing `{program_label} --version` …");
    }
    let started = Instant::now();

    let mut command = background_command(&program);
    command.arg("--version");
    let availability = match command.output() {
        Ok(output) if output.status.success() => {
            let text = if output.stdout.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            } else {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            };
            if text.is_empty() {
                JjRuntimeAvailability::Unavailable {
                    detail: format!("`{program_label} --version` returned no version text."),
                }
            } else {
                JjRuntimeAvailability::Available {
                    program: program_label.clone(),
                    version_output: text,
                }
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                format!("`{program_label} --version` exited with {}.", output.status)
            } else {
                format!("`{program_label} --version` failed: {stderr}")
            };
            JjRuntimeAvailability::Unavailable { detail }
        }
        Err(err) => JjRuntimeAvailability::Unavailable {
            detail: format!("`{program_label}` is unavailable: {err}"),
        },
    };

    let state = JjRuntimeState::probed_now(availability);
    if log {
        eprintln!(
            "[gitcomet-jj] probe finished in {:?}: {:?}",
            started.elapsed(),
            state.availability
        );
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Replace the slot contents directly, bypassing any probing.
    fn install_slot_state(state: Option<JjRuntimeState>) {
        let mut slot = jj_runtime_slot()
            .0
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        slot.state = state;
        slot.probe_running = false;
        jj_runtime_slot().1.notify_all();
    }

    /// Serialize tests that touch the process-wide slot and reset it around
    /// each test so cached state never leaks between them.
    struct JjRuntimeReset;

    impl JjRuntimeReset {
        fn install(state: Option<JjRuntimeState>) -> Self {
            install_slot_state(state);
            Self
        }
    }

    impl Drop for JjRuntimeReset {
        fn drop(&mut self) {
            install_slot_state(None);
            *jj_test_program()
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = None;
        }
    }

    /// Replace the probed program for the current test. Probing a bundled
    /// script keeps these tests deterministic on machines without `jj`.
    fn override_program(program: Option<OsString>) {
        *jj_test_program()
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = program;
    }

    fn available_state() -> JjRuntimeState {
        JjRuntimeState::probed_now(JjRuntimeAvailability::Available {
            program: "jj".to_string(),
            version_output: "jj 9.9.9-test".to_string(),
        })
    }

    #[test]
    fn jj_runtime_state_accessors_mirror_availability() {
        let available = available_state();
        assert!(available.is_available());
        assert_eq!(available.version_output(), Some("jj 9.9.9-test"));
        assert_eq!(available.unavailable_detail(), None);

        let unavailable = JjRuntimeState::probed_now(JjRuntimeAvailability::Unavailable {
            detail: "not found".to_string(),
        });
        assert!(!unavailable.is_available());
        assert_eq!(unavailable.version_output(), None);
        assert_eq!(unavailable.unavailable_detail(), Some("not found"));
    }

    #[test]
    fn jj_runtime_freshness_expires_after_ttl() {
        let fresh = available_state();
        assert!(fresh.is_fresh());
        let stale = JjRuntimeState::probed_ago(
            JjRuntimeAvailability::Unavailable {
                detail: String::new(),
            },
            JJ_PROBE_TTL + Duration::from_secs(1),
        );
        assert!(!stale.is_fresh());
    }

    #[test]
    fn parse_jj_version_accepts_plain_and_suffixed_versions() {
        assert_eq!(parse_jj_version("jj 0.25.0"), Some((0, 25, 0)));
        assert_eq!(parse_jj_version("jj 0.26.1\n"), Some((0, 26, 1)));
        assert_eq!(parse_jj_version("jj 1.0"), Some((1, 0, 0)));
        assert_eq!(parse_jj_version("jj 0.25.0+abc123"), Some((0, 25, 0)));
        assert_eq!(parse_jj_version("git version 2.47.0"), None);
        assert_eq!(parse_jj_version(""), None);
    }

    #[test]
    fn current_jj_runtime_serves_cached_state_without_probing() {
        let _lock = jj_runtime_test_lock();
        let installed = available_state();
        let _reset = JjRuntimeReset::install(Some(installed.clone()));
        override_program(Some(OsString::from("gitcomet-jj-test-missing-binary")));

        assert_eq!(current_jj_runtime(), Some(installed));
    }

    #[test]
    fn ensure_jj_runtime_returns_fresh_cache_without_spawning() {
        let _lock = jj_runtime_test_lock();
        let _reset = JjRuntimeReset::install(Some(available_state()));
        override_program(Some(OsString::from("gitcomet-jj-test-missing-binary")));

        let ensured = ensure_jj_runtime(Duration::from_millis(500)).expect("fresh state");
        assert_eq!(ensured.version_output(), Some("jj 9.9.9-test"));
    }

    #[test]
    fn warm_jj_runtime_probes_and_caches_unavailable_result() {
        let _lock = jj_runtime_test_lock();
        let _reset = JjRuntimeReset::install(None);
        override_program(Some(OsString::from("gitcomet-jj-test-missing-binary")));

        let state = ensure_jj_runtime(Duration::from_secs(5)).expect("probe completes");
        assert!(!state.is_available(), "missing binary must degrade");
        let detail = state.unavailable_detail().expect("unavailable detail");
        assert!(
            detail.contains("gitcomet-jj-test-missing-binary"),
            "detail should name the probed program: {detail}"
        );

        // The degraded result is cached and served without a second probe:
        // the in-flight guard must clear and the state stay stable.
        assert_eq!(current_jj_runtime(), Some(state));
    }

    #[test]
    fn ensure_jj_runtime_falls_back_to_stale_state_when_probe_hangs() {
        let _lock = jj_runtime_test_lock();
        let stale = JjRuntimeState::probed_ago(
            JjRuntimeAvailability::Unavailable {
                detail: "old probe".to_string(),
            },
            JJ_PROBE_TTL + Duration::from_secs(1),
        );
        let _reset = JjRuntimeReset::install(Some(stale.clone()));

        // `/bin/sleep`-style hang is awkward to port; instead simulate it by
        // marking a probe as already in flight: ensure must time out and
        // serve the stale cached value rather than blocking forever.
        {
            let mut slot = jj_runtime_slot()
                .0
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            slot.probe_running = true;
        }

        let started = Instant::now();
        let served = ensure_jj_runtime(Duration::from_millis(200));
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert_eq!(served, Some(stale), "stale value must be served on timeout");
    }

    #[test]
    fn refresh_jj_runtime_replaces_stale_state_synchronously() {
        let _lock = jj_runtime_test_lock();
        let stale = JjRuntimeState::probed_ago(
            JjRuntimeAvailability::Unavailable {
                detail: "old probe".to_string(),
            },
            JJ_PROBE_TTL + Duration::from_secs(1),
        );
        let _reset = JjRuntimeReset::install(Some(stale.clone()));

        // No program override: probes the real `jj`. Both a real success and
        // a real "not installed" outcome are acceptable — the point is that
        // the state is replaced synchronously with a freshly stamped probe.
        let refreshed = refresh_jj_runtime();
        assert!(refreshed.is_fresh());
        assert_eq!(current_jj_runtime(), Some(refreshed.clone()));
        assert_ne!(refreshed, stale);
    }
}
