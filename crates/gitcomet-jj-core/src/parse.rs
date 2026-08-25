//! Strict parsers for the CLI's machine-readable `-T` template output.
//!
//! The templates in [`crate::cli`] join fields with the unit separator
//! (`\x1f`) and terminate records with the record separator (`\x1e`) — both
//! validated against jj 0.44, which supports `\x1f`/`\x1e` escapes but not
//! `\u{..}`. Parsing is deliberately strict: every record must carry exactly
//! the expected field count and valid flag tokens, and a violation is an
//! error rather than a silently dropped or mis-shapen row. A description can
//! legitimately contain newlines (kept verbatim); it cannot contain the
//! separator bytes without tripping the field count — the failure mode for
//! hostile input is a surfaced error, never a mis-rendered log.
//!
//! Record framing: jj prints one template evaluation per line, so each
//! record is `\x1e`-terminated and followed by a newline. Chunks are
//! newline-trimmed at the edges only; newlines interior to a field (a
//! multi-line description) survive untouched.

use gitcomet_core::error::{Error, ErrorKind};
use gitcomet_core::services::Result;

use crate::domain::{ChangeId, JjBookmark, JjChange, JjCommitId, JjConflict, JjOp};

const FIELD_SEP: char = '\x1f';
const RECORD_SEP: char = '\x1e';
const LOG_FIELD_COUNT: usize = 10;
const BOOKMARK_FIELD_COUNT: usize = 4;
const OP_FIELD_COUNT: usize = 4;

/// Split template output into trimmed, non-empty record chunks.
fn records(output: &str) -> impl Iterator<Item = &str> {
    output
        .split(RECORD_SEP)
        .map(|record| record.trim_matches('\n'))
        .filter(|record| !record.is_empty())
}

/// Split one record into exactly `expected` fields.
fn fields<'a>(record: &'a str, expected: usize, what: &str) -> Result<Vec<&'a str>> {
    let fields: Vec<&str> = record.split(FIELD_SEP).collect();
    if fields.len() != expected {
        return Err(Error::new(ErrorKind::Backend(format!(
            "jj {what}: record has {} fields, expected {expected}",
            fields.len()
        ))));
    }
    Ok(fields)
}

fn flag(fields: &[&str], ix: usize, on: &str, what: &str) -> Result<bool> {
    match fields[ix] {
        value if value == on => Ok(true),
        "N" => Ok(false),
        other => Err(Error::new(ErrorKind::Backend(format!(
            "jj {what}: unexpected flag {other:?}"
        )))),
    }
}

fn non_empty<'a>(fields: &[&'a str], ix: usize, what: &str) -> Result<&'a str> {
    let value = fields[ix];
    if value.is_empty() {
        return Err(Error::new(ErrorKind::Backend(format!(
            "jj {what}: empty field {ix}"
        ))));
    }
    Ok(value)
}

fn unix_seconds(fields: &[&str], ix: usize, what: &str) -> Result<i64> {
    fields[ix].parse::<i64>().map_err(|_| {
        Error::new(ErrorKind::Backend(format!(
            "jj {what}: invalid timestamp {:?}",
            fields[ix]
        )))
    })
}

/// Parse `jj log` output in the crate's template format.
pub(crate) fn parse_log_records(output: &str) -> Result<Vec<JjChange>> {
    let mut changes = Vec::new();
    for record in records(output) {
        let what = "log";
        let fields = fields(record, LOG_FIELD_COUNT, what)?;
        let bookmarks = if fields[3].is_empty() {
            Vec::new()
        } else {
            fields[3].split(',').map(str::to_string).collect()
        };
        changes.push(JjChange {
            change_id: ChangeId(non_empty(&fields, 0, what)?.to_string()),
            commit_id: JjCommitId(non_empty(&fields, 1, what)?.to_string()),
            divergent: flag(&fields, 2, "D", what)?,
            bookmarks,
            is_working_copy: match fields[4] {
                "at" => true,
                "no" => false,
                other => {
                    return Err(Error::new(ErrorKind::Backend(format!(
                        "jj log: unexpected working-copy flag {other:?}"
                    ))));
                }
            },
            author_name: fields[5].to_string(),
            author_email: fields[6].to_string(),
            committed_at_unix: unix_seconds(&fields, 7, what)?,
            conflicted: flag(&fields, 8, "C", what)?,
            description: fields[9].to_string(),
        });
    }
    Ok(changes)
}

/// Parse `jj bookmark list --all` output in the crate's template format.
pub(crate) fn parse_bookmark_records(output: &str) -> Result<Vec<JjBookmark>> {
    let mut bookmarks = Vec::new();
    for record in records(output) {
        let what = "bookmark list";
        let fields = fields(record, BOOKMARK_FIELD_COUNT, what)?;
        bookmarks.push(JjBookmark {
            name: non_empty(&fields, 0, what)?.to_string(),
            remote: (!fields[1].is_empty()).then(|| fields[1].to_string()),
            target_commit_id: JjCommitId(non_empty(&fields, 2, what)?.to_string()),
            conflicted: flag(&fields, 3, "C", what)?,
        });
    }
    Ok(bookmarks)
}

/// Parse `jj op log` output in the crate's template format.
pub(crate) fn parse_op_records(output: &str) -> Result<Vec<JjOp>> {
    let mut ops = Vec::new();
    for record in records(output) {
        let what = "op log";
        let fields = fields(record, OP_FIELD_COUNT, what)?;
        ops.push(JjOp {
            op_id: non_empty(&fields, 0, what)?.to_string(),
            description: fields[1].to_string(),
            user: fields[2].to_string(),
            started_at_unix: unix_seconds(&fields, 3, what)?,
        });
    }
    Ok(ops)
}

/// Parse `jj resolve --list` output: one path per line.
pub(crate) fn parse_conflict_paths(output: &str) -> Vec<JjConflict> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|path| JjConflict {
            path: path.to_string(),
        })
        .collect()
}

/// `jj resolve --list` exits non-zero with this message when the working
/// copy has no conflicts; callers map it to an empty conflict list.
pub(crate) fn is_no_conflicts_message(detail: &str) -> bool {
    detail.contains("No conflicts found")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: char = '\x1f';
    const RS: char = '\x1e';

    #[allow(clippy::too_many_arguments)] // mirrors the ten log fields
    fn log_record(
        change: &str,
        commit: &str,
        divergent: bool,
        bookmarks: &str,
        at: bool,
        name: &str,
        email: &str,
        ts: &str,
        conflict: bool,
        desc: &str,
    ) -> String {
        format!(
            "{change}{FS}{commit}{FS}{}{FS}{bookmarks}{FS}{}{FS}{name}{FS}{email}{FS}{ts}{FS}{}{FS}{desc}{RS}\n",
            if divergent { "D" } else { "N" },
            if at { "at" } else { "no" },
            if conflict { "C" } else { "N" },
        )
    }

    #[test]
    fn parses_a_full_log_record() {
        let output = log_record(
            "wqnwyzpk",
            "019aa1e2",
            false,
            "main,main@origin",
            true,
            "Ada",
            "ada@example.com",
            "1719000000",
            false,
            "describe the change",
        );
        let changes = parse_log_records(&output).expect("record parses");
        assert_eq!(changes.len(), 1);
        let change = &changes[0];
        assert_eq!(change.change_id.0, "wqnwyzpk");
        assert_eq!(change.commit_id.0, "019aa1e2");
        assert!(!change.divergent);
        assert!(!change.conflicted);
        assert!(change.is_working_copy);
        assert_eq!(change.bookmarks, vec!["main", "main@origin"]);
        assert_eq!(change.author_name, "Ada");
        assert_eq!(change.author_email, "ada@example.com");
        assert_eq!(change.committed_at_unix, 1_719_000_000);
        assert_eq!(change.description, "describe the change");
    }

    #[test]
    fn keeps_multiline_descriptions_verbatim() {
        let output = log_record(
            "abc",
            "def",
            false,
            "",
            false,
            "Ada",
            "ada@example.com",
            "0",
            false,
            "subject\n\nbody line",
        );
        let changes = parse_log_records(&output).expect("record parses");
        assert_eq!(changes[0].description, "subject\n\nbody line");
        assert!(changes[0].bookmarks.is_empty());
    }

    #[test]
    fn parses_multiple_records_including_flags() {
        let output = format!(
            "{}{}",
            log_record(
                "a1",
                "c1",
                true,
                "feat",
                false,
                "n",
                "e",
                "1",
                true,
                "divergent+conflict"
            ),
            log_record("a2", "c2", false, "", true, "n", "e", "2", false, "")
        );
        let changes = parse_log_records(&output).expect("records parse");
        assert_eq!(changes.len(), 2);
        assert!(changes[0].divergent && changes[0].conflicted && !changes[0].is_working_copy);
        assert!(!changes[1].divergent && !changes[1].conflicted && changes[1].is_working_copy);
        assert_eq!(changes[1].description, "");
    }

    #[test]
    fn wrong_field_count_is_an_error() {
        let short = format!("only{FS}two{RS}\n");
        assert!(parse_log_records(&short).is_err());

        // An embedded separator inside a description shifts the count too.
        let injected = log_record(
            "a",
            "b",
            false,
            "",
            false,
            "n",
            "e",
            "0",
            false,
            &format!("line{FS}line2"),
        );
        assert!(parse_log_records(&injected).is_err());
    }

    #[test]
    fn bad_flags_and_timestamps_are_errors() {
        let bad_flag = format!("a{FS}b{FS}X{FS}{FS}no{FS}n{FS}e{FS}0{FS}N{FS}d{RS}\n");
        assert!(parse_log_records(&bad_flag).is_err());

        let bad_wc = format!("a{FS}b{FS}N{FS}{FS}maybe{FS}n{FS}e{FS}0{FS}N{FS}d{RS}\n");
        assert!(parse_log_records(&bad_wc).is_err());

        let bad_ts = log_record(
            "a",
            "b",
            false,
            "",
            false,
            "n",
            "e",
            "not-a-number",
            false,
            "d",
        );
        assert!(parse_log_records(&bad_ts).is_err());

        let empty_id = log_record("", "b", false, "", false, "n", "e", "0", false, "d");
        assert!(parse_log_records(&empty_id).is_err());
    }

    #[test]
    fn empty_log_output_yields_no_changes() {
        assert!(parse_log_records("").unwrap().is_empty());
        assert!(parse_log_records("\n").unwrap().is_empty());
    }

    #[test]
    fn parses_local_and_remote_bookmarks() {
        let output = format!(
            "main{FS}{FS}c0{FS}N{RS}\nmain{FS}origin{FS}c0{FS}N{RS}\nfeature{FS}{FS}c1{FS}C{RS}\n"
        );
        let bookmarks = parse_bookmark_records(&output).expect("records parse");
        assert_eq!(bookmarks.len(), 3);
        assert_eq!(bookmarks[0].name, "main");
        assert!(bookmarks[0].is_local());
        assert!(!bookmarks[0].conflicted);
        assert_eq!(bookmarks[1].remote.as_deref(), Some("origin"));
        assert!(!bookmarks[1].is_local());
        assert_eq!(bookmarks[2].target_commit_id.0, "c1");
        assert!(bookmarks[2].conflicted);
    }

    #[test]
    fn bookmark_field_count_and_flags_are_strict() {
        assert!(parse_bookmark_records(&format!("main{FS}origin{FS}c0{RS}\n")).is_err());
        assert!(parse_bookmark_records(&format!("main{FS}{FS}c0{FS}X{RS}\n")).is_err());
        assert!(parse_bookmark_records(&format!("{FS}{FS}c0{FS}N{RS}\n")).is_err());
    }

    #[test]
    fn parses_op_records() {
        let output =
            format!("0pf0ab{FS}check out git commit{FS}Ada <ada@example.com>{FS}1719000100{RS}\n");
        let ops = parse_op_records(&output).expect("records parse");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].op_id, "0pf0ab");
        assert_eq!(ops[0].description, "check out git commit");
        assert_eq!(ops[0].user, "Ada <ada@example.com>");
        assert_eq!(ops[0].started_at_unix, 1_719_000_100);
    }

    #[test]
    fn op_record_strictness() {
        assert!(parse_op_records(&format!("0pf{FS}d{FS}u{FS}x{RS}\n")).is_err());
        assert!(parse_op_records(&format!("0pf{FS}d{FS}u{RS}\n")).is_err());
    }

    #[test]
    fn parses_conflict_paths() {
        let conflicts = parse_conflict_paths("file.txt\n\ndir/nested.rs\n");
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].path, "file.txt");
        assert_eq!(conflicts[1].path, "dir/nested.rs");
    }

    #[test]
    fn recognizes_the_no_conflicts_message() {
        assert!(is_no_conflicts_message(
            "Error: No conflicts found at this revision"
        ));
        assert!(!is_no_conflicts_message("some other failure"));
    }
}
