//! Line-coverage reports for the diff view's overlay.
//!
//! One format covers both producers the roadmap names: lcov tracefiles and
//! `llvm-cov`'s lcov output (`llvm-cov show --format=lcov`, which is also
//! what `cargo llvm-cov --lcov` emits). The parser is deliberately lenient
//! — unknown sections (functions, branches) are skipped, malformed records
//! dropped — so an export from any generator flavor still yields the line
//! table the overlay needs.

use std::collections::BTreeMap;

/// One file's line table: 1-based line number to execution count. A count
/// of zero is a missed line; absence means no data for that line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileCoverage {
    pub lines: BTreeMap<u32, u64>,
}

impl FileCoverage {
    fn record(&mut self, line: u32, hits: u64) {
        self.lines.insert(line, hits);
    }
}

/// The verdict the overlay paints for one line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageLineStatus {
    Covered,
    Missed,
}

/// A parsed coverage report, keyed by repo-relative path with `/`
/// separators (the shape lcov's `SF:` records use, and the shape diff
/// targets already carry).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverageReport {
    pub files: BTreeMap<String, FileCoverage>,
}

/// Normalize one side of a path lookup: backslashes to forward slashes and
/// a leading `./` stripped, so a generator's `SF:./src/lib.rs` matches the
/// diff target's `src/lib.rs`.
pub fn normalize_coverage_path(path: &str) -> String {
    let trimmed = path.trim();
    let slashed = trimmed.replace('\\', "/");
    slashed
        .strip_prefix("./")
        .unwrap_or(&slashed)
        .trim_start_matches('/')
        .to_string()
}

impl CoverageReport {
    /// Parse lcov tracefile text. Records without any `DA:` line are
    /// dropped (nothing to annotate from); a report with no records at all
    /// is an error, because the usual cause is picking a non-lcov file.
    pub fn parse_lcov(text: &str) -> Result<Self, String> {
        let mut report = Self::default();
        let mut current: Option<(String, FileCoverage)> = None;
        for line in text.lines() {
            let line = line.trim();
            if let Some(path) = line.strip_prefix("SF:") {
                if current.is_some() {
                    // An unterminated record: lcov writers close them, but a
                    // truncated export should not poison the next one.
                    current = None;
                }
                current = Some((normalize_coverage_path(path), FileCoverage::default()));
            } else if let Some(data) = line.strip_prefix("DA:") {
                let Some((path, file)) = current.as_mut() else {
                    continue;
                };
                let mut parts = data.split(',');
                let (Some(line_no), Some(hits)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let (Ok(line_no), Ok(hits)) = (line_no.trim().parse::<u32>(), hits.trim().parse::<i64>())
                else {
                    continue;
                };
                // Some generators emit -1 for "not instrumented"; the only
                // signal the overlay has is zero-vs-nonzero.
                file.record(line_no, hits.max(0) as u64);
                let _ = path;
            } else if line == "end_of_record" {
                if let Some((path, file)) = current.take() {
                    if !file.lines.is_empty() {
                        report.files.insert(path, file);
                    }
                }
            }
        }
        if let Some((path, file)) = current {
            // A missing final end_of_record still counts if the writer was
            // cut off after the last DA line.
            if !file.lines.is_empty() {
                report.files.insert(path, file);
            }
        }
        if report.files.is_empty() {
            return Err("no coverage records found — is this an lcov/llvm-cov export?".to_string());
        }
        Ok(report)
    }

    /// The overlay verdict for one line of one file, if the report knows it.
    pub fn line_status(&self, path: &str, line: u32) -> Option<CoverageLineStatus> {
        let file = self.files.get(&normalize_coverage_path(path))?;
        match file.lines.get(&line)? {
            0 => Some(CoverageLineStatus::Missed),
            _ => Some(CoverageLineStatus::Covered),
        }
    }

    /// What the import toast reports back: how much data landed.
    pub fn summarize(&self) -> CoverageSummary {
        let mut summary = CoverageSummary::default();
        summary.files = self.files.len();
        for file in self.files.values() {
            for &hits in file.lines.values() {
                summary.lines += 1;
                if hits > 0 {
                    summary.covered += 1;
                }
            }
        }
        summary
    }
}

/// Aggregate counts for feedback, not for drawing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoverageSummary {
    pub files: usize,
    pub lines: usize,
    pub covered: usize,
}

impl CoverageSummary {
    /// Covered lines as a percentage, rounded down; `None` when the report
    /// carries no line data at all.
    pub fn covered_percent(&self) -> Option<u32> {
        (self.lines > 0)
            .then(|| u32::try_from(self.covered.saturating_mul(100) / self.lines).unwrap_or(100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
TN:
SF:src/lib.rs
FN:4,(name=run/1)
DA:1,2
DA:2,0
DA:3,5,checksum
end_of_record
SF:./src/ui/widget.rs
DA:10,1
end_of_record
SF:docs/empty.rs
FN:1,(name=unused)
end_of_record
";

    #[test]
    fn parse_lcov_reads_line_data_and_skips_empty_records() {
        let report = CoverageReport::parse_lcov(SAMPLE).unwrap();
        // The empty record (no DA lines) is dropped, not kept as noise.
        assert_eq!(report.files.len(), 2);
        let lib = report.files.get("src/lib.rs").unwrap();
        assert_eq!(
            lib.lines,
            BTreeMap::from([(1, 2), (2, 0), (3, 5)]),
            "checksum third fields are ignored"
        );
        // The ./ prefix normalizes onto the diff target's shape.
        assert!(report.files.contains_key("src/ui/widget.rs"));
    }

    #[test]
    fn line_status_reports_zero_as_missed() {
        let report = CoverageReport::parse_lcov(SAMPLE).unwrap();
        assert_eq!(report.line_status("src/lib.rs", 1), Some(CoverageLineStatus::Covered));
        assert_eq!(report.line_status("src/lib.rs", 2), Some(CoverageLineStatus::Missed));
        // Unknown line and unknown file both read as "no data".
        assert_eq!(report.line_status("src/lib.rs", 99), None);
        assert_eq!(report.line_status("other.rs", 1), None);
        // Path separators and ./ agree with the normalized key.
        assert_eq!(
            report.line_status(".\\src\\ui\\widget.rs", 10),
            Some(CoverageLineStatus::Covered)
        );
    }

    #[test]
    fn parse_lcov_survives_truncation_and_negative_hits() {
        let truncated = "SF:src/a.rs\nDA:1,3\n"; // no end_of_record
        let report = CoverageReport::parse_lcov(truncated).unwrap();
        assert_eq!(report.line_status("src/a.rs", 1), Some(CoverageLineStatus::Covered));

        let negative = "SF:src/b.rs\nDA:7,-1\nend_of_record\n";
        let report = CoverageReport::parse_lcov(negative).unwrap();
        assert_eq!(
            report.line_status("src/b.rs", 7),
            Some(CoverageLineStatus::Missed),
            "negative counts clamp to missed, matching zero-vs-nonzero"
        );
    }

    #[test]
    fn parse_lcov_rejects_files_without_any_records() {
        let error = CoverageReport::parse_lcov("hello world\n").unwrap_err();
        assert!(error.contains("lcov"), "the error should point at the format: {error}");
    }

    #[test]
    fn summarize_counts_files_lines_and_coverage() {
        let report = CoverageReport::parse_lcov(SAMPLE).unwrap();
        let summary = report.summarize();
        assert_eq!(summary.files, 2);
        assert_eq!(summary.lines, 4);
        assert_eq!(summary.covered, 3);
        assert_eq!(summary.covered_percent(), Some(75));
        assert_eq!(CoverageSummary::default().covered_percent(), None);
    }
}
