//! Daily application log file, for diagnosing behavior a crash log can't
//! cover — long-running state, network calls, anything where "what happened
//! just before" matters but nothing panicked.
//!
//! One file per calendar day, named `YYYYMMDD.txt` from the startup date's
//! local time zone, under the same state root as the crash logs:
//! `%LOCALAPPDATA%/worktree/log` on Windows. The name is fixed at startup,
//! so a session that crosses midnight keeps appending to the file it opened;
//! per-line timestamps keep such a file readable. Files whose date is more
//! than [`RETENTION_DAYS`] old are deleted at startup.
//!
//! Records land from any thread. Logging must never take the application
//! down: a log directory that cannot be created or opened turns every
//! subsequent call into a no-op, and write failures drop the record.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// How many days of log files to keep. The startup pass deletes files whose
/// date is strictly older than this many days before today.
const RETENTION_DAYS: i32 = 7;

/// The process-wide sink. `None` (or a `Mutex` holding `None`) until
/// [`init`] runs, and permanently if the log file could not be opened —
/// either way, log calls are no-ops rather than errors.
static SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Record severity. Coarse on purpose — the file exists to answer "what did
/// the app do", not to carry a full tracing taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_label(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// Resolve the log directory and open today's file. The first call wins;
/// later calls (a second window, a test) leave the original sink in place.
/// Old files are pruned whether or not today's file could be opened, so a
/// locked log file still cannot grow the directory forever.
pub fn init() {
    let Some(dir) = log_dir() else {
        return;
    };
    let today = jiff::Zoned::now().date();
    prune_old_logs_in_dir(&dir, today);
    let file = open_log_file(&dir, today);
    let _ = SINK.set(Mutex::new(file.ok()));
}

/// Append one record. A no-op until [`init`] succeeds in opening a file.
pub fn log(level: Level, target: &str, message: std::fmt::Arguments<'_>) {
    let Some(sink) = SINK.get() else {
        return;
    };
    let mut file = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(file) = file.as_mut() else {
        return;
    };
    let timestamp = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S%.3f");
    // A message with embedded newlines would blur where one record ends, so
    // each run of whitespace collapses to single spaces.
    let message = message.to_string();
    let message = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let _ = writeln!(
        file,
        "{timestamp} [{}] {target}: {message}",
        level.as_label()
    );
    let _ = file.flush();
}

/// `…/worktree/log` under the platform's per-user state location — the same
/// root [`crate`]'s crash logs use, so all diagnostics live in one tree.
fn log_dir() -> Option<PathBuf> {
    log_dir_from_base(state_dir_base()?)
}

fn log_dir_from_base(base: PathBuf) -> Option<PathBuf> {
    Some(base.join("worktree").join("log"))
}

#[cfg(target_os = "windows")]
fn state_dir_base() -> Option<PathBuf> {
    state_dir_base_windows(
        std::env::var("LOCALAPPDATA").ok().as_deref(),
        std::env::var("APPDATA").ok().as_deref(),
    )
}

/// LOCALAPPDATA is where the crash logs already live; APPDATA is the
/// fallback for the odd session where it is unset.
#[cfg(target_os = "windows")]
fn state_dir_base_windows(local_app_data: Option<&str>, app_data: Option<&str>) -> Option<PathBuf> {
    non_empty_path(local_app_data).or_else(|| non_empty_path(app_data))
}

#[cfg(target_os = "macos")]
fn state_dir_base() -> Option<PathBuf> {
    state_dir_base_macos(std::env::var("HOME").ok().as_deref())
}

#[cfg(target_os = "macos")]
fn state_dir_base_macos(home: Option<&str>) -> Option<PathBuf> {
    non_empty_path(home).map(|home| home.join("Library").join("Logs"))
}

#[cfg(target_os = "linux")]
fn state_dir_base() -> Option<PathBuf> {
    state_dir_base_linux(
        std::env::var("XDG_STATE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

#[cfg(target_os = "linux")]
fn state_dir_base_linux(xdg_state_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    non_empty_path(xdg_state_home)
        .or_else(|| non_empty_path(home).map(|home| home.join(".local").join("state")))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn state_dir_base() -> Option<PathBuf> {
    state_dir_base_other(std::env::var("HOME").ok().as_deref())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn state_dir_base_other(home: Option<&str>) -> Option<PathBuf> {
    non_empty_path(home)
}

fn non_empty_path(value: Option<&str>) -> Option<PathBuf> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

/// `YYYYMMDD.txt`, zero-padded, from the local calendar date.
fn log_file_name(date: jiff::civil::Date) -> String {
    date.strftime("%Y%m%d").to_string() + ".txt"
}

fn open_log_file(dir: &Path, today: jiff::civil::Date) -> std::io::Result<File> {
    std::fs::create_dir_all(dir)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(log_file_name(today)))
}

/// Delete `YYYYMMDD.txt` files older than [`RETENTION_DAYS`] days. Names
/// that don't parse as a date are left alone — this directory belongs to the
/// app, but an unparseable file is more likely a user's than garbage.
fn prune_old_logs_in_dir(dir: &Path, today: jiff::civil::Date) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(date) = log_file_date(name) else {
            continue;
        };
        // Negative age (a file dated in the future) reads as fresh, matching
        // how "older than" behaves with a skewed clock.
        let age_days = today.since(date).map(|span| span.get_days()).unwrap_or(0);
        if age_days > RETENTION_DAYS {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// The date encoded in a `YYYYMMDD.txt` file name, if it is exactly that.
fn log_file_date(name: &str) -> Option<jiff::civil::Date> {
    let stem = name.strip_suffix(".txt")?;
    if stem.len() != 8 || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    jiff::civil::Date::strptime("%Y%m%d", stem).ok()
}

#[macro_export]
macro_rules! applog_info {
    ($($arg:tt)*) => {
        $crate::applog::log($crate::applog::Level::Info, module_path!(), format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! applog_warn {
    ($($arg:tt)*) => {
        $crate::applog::log($crate::applog::Level::Warn, module_path!(), format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! applog_error {
    ($($arg:tt)*) => {
        $crate::applog::log($crate::applog::Level::Error, module_path!(), format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;
    use std::fs;

    fn log_dir_with_files(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in names {
            fs::write(dir.path().join(name), "x").expect("write log file");
        }
        dir
    }

    #[test]
    fn log_file_name_is_zero_padded_local_date() {
        assert_eq!(log_file_name(date(2026, 8, 9)), "20260809.txt");
        assert_eq!(log_file_name(date(2026, 12, 31)), "20261231.txt");
    }

    #[test]
    fn log_file_date_round_trips_the_file_name() {
        assert_eq!(log_file_date("20260809.txt"), Some(date(2026, 8, 9)));
        assert_eq!(log_file_date("20260809"), None, "missing .txt suffix");
        assert_eq!(log_file_date("2026089.txt"), None, "not 8 digits");
        assert_eq!(log_file_date("2026-08-09.txt"), None, "dashes");
        assert_eq!(log_file_date("not-a-date.txt"), None);
        assert_eq!(
            log_file_date("20261340.txt"),
            None,
            "digits but not a calendar date"
        );
    }

    #[test]
    fn prune_deletes_only_files_older_than_retention() {
        let today = date(2026, 8, 29);
        let dir = log_dir_with_files(&[
            "20260829.txt", // today — kept
            "20260822.txt", // 7 days old — kept (retention is > 7)
            "20260821.txt", // 8 days old — deleted
            "20260101.txt", // far past — deleted
            "notes.txt",    // not a log name — left alone
            "20260830.txt", // dated tomorrow — kept
        ]);

        prune_old_logs_in_dir(dir.path(), today);

        let mut remaining: Vec<String> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| Some(entry.expect("entry").file_name().into_string().ok()?))
            .collect();
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                "20260822.txt".to_string(),
                "20260829.txt".to_string(),
                "20260830.txt".to_string(),
                "notes.txt".to_string(),
            ]
        );
    }

    #[test]
    fn prune_tolerates_a_missing_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("never-created");
        prune_old_logs_in_dir(&missing, date(2026, 8, 29));
        assert!(!missing.exists());
    }

    #[test]
    fn open_log_file_creates_the_directory_and_appends() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir.path().join("log");

        {
            let mut file = open_log_file(&nested, date(2026, 8, 29)).expect("open log file");
            writeln!(file, "first").expect("write first line");
        }
        {
            let mut file = open_log_file(&nested, date(2026, 8, 29)).expect("reopen log file");
            writeln!(file, "second").expect("write second line");
        }

        let contents = fs::read_to_string(nested.join("20260829.txt")).expect("read log file");
        assert_eq!(contents, "first\nsecond\n");
    }

    // The expectation is a Windows path literal, and `PathBuf::join` on unix
    // would append "worktree/log" with forward slashes to the verbatim
    // `C:\State` component. The join shape is what is under test, not the
    // separator, so keep it where the literal is meaningful.
    #[cfg(windows)]
    #[test]
    fn log_dir_joins_worktree_log_under_the_state_base() {
        assert_eq!(
            log_dir_from_base(PathBuf::from(r"C:\State")),
            Some(PathBuf::from(r"C:\State\worktree\log"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn state_dir_base_prefers_local_app_data_then_app_data() {
        assert_eq!(
            state_dir_base_windows(Some(r"C:\Local"), Some(r"C:\Roaming")),
            Some(PathBuf::from(r"C:\Local"))
        );
        assert_eq!(
            state_dir_base_windows(Some("   "), Some(r"C:\Roaming")),
            Some(PathBuf::from(r"C:\Roaming"))
        );
        assert_eq!(state_dir_base_windows(None, None), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn state_dir_base_macos_uses_home_logs_dir() {
        assert_eq!(
            state_dir_base_macos(Some("/Users/alice")),
            Some(PathBuf::from("/Users/alice/Library/Logs"))
        );
        assert_eq!(state_dir_base_macos(Some("   ")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn state_dir_base_linux_prefers_xdg_state_home() {
        assert_eq!(
            state_dir_base_linux(Some("/state"), Some("/home/alice")),
            Some(PathBuf::from("/state"))
        );
        assert_eq!(
            state_dir_base_linux(None, Some("/home/alice")),
            Some(PathBuf::from("/home/alice/.local/state"))
        );
        assert_eq!(state_dir_base_linux(None, Some("  ")), None);
    }
}
