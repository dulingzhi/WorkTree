//! Pure aggregation model behind the statistics popover.
//!
//! The backend delivers raw `(author, time)` pairs from a 400-day window
//! (see `contributor_commits_since`); grouping them into calendar buckets
//! needs the user's *display* timezone, which only the UI layer knows. All
//! bucketing here is integer epoch-day math (Howard Hinnant's civil-date
//! algorithms) resolved through [`Timezone::offset_seconds_at`], so
//! `SystemLocal` stays DST-correct without duplicating any calendar rules.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use worktree_core::domain::ContributorCommit;

use super::date_time::Timezone;

/// Bars and contributor rankings for one period. Every bar covers exactly one
/// bucket of the period (weekday / day-of-month / calendar month) and
/// `total_commits` is the sum of the bars, so commits outside the current
/// window are excluded from both the chart and the ranking.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct PeriodModel {
    pub(in crate::view) total_commits: u64,
    pub(in crate::view) bars: Vec<StatisticsBar>,
    pub(in crate::view) contributors: Vec<StatisticsContributor>,
}

/// One chart column. `label_index` is a weekday (0 = Sunday), day of month
/// (1..=31), or calendar month (1..=12) depending on the period; the label
/// text is resolved against the locale at render time, keeping this model
/// free of i18n state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct StatisticsBar {
    pub(in crate::view) label_index: u32,
    pub(in crate::view) count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct StatisticsContributor {
    pub(in crate::view) author: Arc<str>,
    pub(in crate::view) count: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct StatisticsModel {
    /// Current calendar week (Sunday-first), grouped by weekday.
    pub(in crate::view) week: PeriodModel,
    /// Current calendar month, grouped by day of month.
    pub(in crate::view) month: PeriodModel,
    /// Current calendar year, grouped by month.
    pub(in crate::view) year: PeriodModel,
}

pub(in crate::view) fn build_statistics_model(
    commits: &[ContributorCommit],
    timezone: Timezone,
    now: SystemTime,
) -> StatisticsModel {
    let today = epoch_day_in_zone(unix_seconds(now), timezone);
    let (year, month, _day) = civil_from_days(today);

    let week_start = today - i64::from(weekday_sunday_first(today));
    let month_start = days_from_civil(year, month, 1);
    let month_len = i64::from(days_in_month(year, month));
    let year_start = days_from_civil(year, 1, 1);
    let year_len = days_from_civil(year + 1, 1, 1) - year_start;

    let mut week = WindowAccumulator::new(7);
    let mut month_acc = WindowAccumulator::new(month_len as u32);
    let mut year_acc = WindowAccumulator::new(12);

    for commit in commits {
        let day = epoch_day_in_zone(unix_seconds(commit.time), timezone);

        // Week: bar index is the weekday itself.
        week.add_if_in_window(
            day,
            week_start,
            7,
            u32::from(weekday_sunday_first(day)),
            &commit.author,
        );

        // Month: bar index counts days from the month's first day.
        month_acc.add_if_in_window(
            day,
            month_start,
            month_len,
            (day - month_start) as u32,
            &commit.author,
        );

        // Year: bar index is the calendar month; month lengths vary, so the
        // index comes from the civil date rather than the day offset.
        let (_y, m, _d) = civil_from_days(day);
        year_acc.add_if_in_window(day, year_start, year_len, m - 1, &commit.author);
    }

    StatisticsModel {
        week: week.finish(0),
        month: month_acc.finish(1),
        year: year_acc.finish(1),
    }
}

/// Collects bars and per-author counts for one window. `bar_count` buckets
/// exist regardless of data so empty periods still render their full axis.
struct WindowAccumulator {
    counts: Vec<u64>,
    authors: HashMap<Arc<str>, u64>,
    total: u64,
}

impl WindowAccumulator {
    fn new(bar_count: u32) -> Self {
        Self {
            counts: vec![0; bar_count as usize],
            authors: HashMap::new(),
            total: 0,
        }
    }

    /// Count `author` into bar `bar_index` (0-based bucket index, mapped to a
    /// label by `finish`) iff `day` falls in `[start, start + window_days)`;
    /// commits dated outside the window (older periods, or future clock-skew
    /// past the window end) are skipped so they distort neither the chart nor
    /// the ranking.
    fn add_if_in_window(
        &mut self,
        day: i64,
        start: i64,
        window_days: i64,
        bar_index: u32,
        author: &Arc<str>,
    ) {
        if day < start || day >= start + window_days {
            return;
        }
        let Some(count) = self.counts.get_mut(bar_index as usize) else {
            return;
        };
        *count += 1;
        *self.authors.entry(Arc::clone(author)).or_insert(0) += 1;
        self.total += 1;
    }

    /// Freeze into a `PeriodModel`; `first_label_index` is the label of
    /// bucket 0 (weekday 0 for weeks, day 1 / month 1 for the others).
    fn finish(self, first_label_index: u32) -> PeriodModel {
        let mut contributors: Vec<StatisticsContributor> = self
            .authors
            .into_iter()
            .map(|(author, count)| StatisticsContributor { author, count })
            .collect();
        // Count descending; ties fall back to name so the order is stable
        // across reloads regardless of HashMap iteration order.
        contributors.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.author.cmp(&b.author)));

        PeriodModel {
            total_commits: self.total,
            bars: self
                .counts
                .into_iter()
                .enumerate()
                .map(|(ix, count)| StatisticsBar {
                    label_index: first_label_index + ix as u32,
                    count,
                })
                .collect(),
            contributors,
        }
    }
}

fn unix_seconds(t: SystemTime) -> i64 {
    use std::time::UNIX_EPOCH;
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// Local calendar day (days since 1970-01-01) of a unix timestamp.
fn epoch_day_in_zone(unix: i64, timezone: Timezone) -> i64 {
    floor_div(unix.saturating_add(timezone.offset_seconds_at(unix)), 86_400)
}

fn floor_div(a: i64, b: i64) -> i64 {
    let mut q = a / b;
    let r = a % b;
    if (r != 0) && ((r < 0) != (b < 0)) {
        q -= 1;
    }
    q
}

/// Day of week with Sunday = 0. Epoch day 0 (1970-01-01) was a Thursday.
fn weekday_sunday_first(epoch_day: i64) -> u32 {
    ((epoch_day + 4).rem_euclid(7)) as u32
}

/// Howard Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch.saturating_add(719_468);
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = mp + if mp < 10 { 3 } else { -9 }; // [1, 12]
    let y = y + i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}

/// Howard Hinnant's `days_from_civil` (inverse of `civil_from_days`).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(if m <= 2 { y - 1 } else { y });
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 }); // [0, 11]
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
            u32::from(leap) + 28
        }
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn at_utc(y: i32, m: u32, d: u32, hour: u32) -> SystemTime {
        let secs = days_from_civil(y, m, d) * 86_400 + i64::from(hour) * 3_600;
        UNIX_EPOCH + Duration::from_secs(secs as u64)
    }

    fn commit(author: &str, time: SystemTime) -> ContributorCommit {
        ContributorCommit {
            author: Arc::from(author),
            time,
        }
    }

    #[test]
    fn calendar_helpers_round_trip_known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-01-01 is epoch day 10957; leap February puts March 1 at 11017.
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        // Century rule: 1900 is not leap, 2000 is.
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);

        for days in [0i64, 11_017, 20_662] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn weekdays_are_sunday_first() {
        // 1970-01-01 was a Thursday; 2026-08-27 also is.
        assert_eq!(weekday_sunday_first(0), 4);
        assert_eq!(weekday_sunday_first(days_from_civil(2026, 8, 27)), 4);
        assert_eq!(weekday_sunday_first(days_from_civil(2026, 8, 23)), 0);
        assert_eq!(weekday_sunday_first(days_from_civil(2026, 8, 29)), 6);
    }

    #[test]
    fn week_buckets_by_weekday_and_excludes_other_weeks() {
        // 2026-08-27 is a Thursday; the current week is Sun Aug 23 .. Sat 29.
        let now = at_utc(2026, 8, 27, 12);
        let commits = vec![
            commit("Alice", at_utc(2026, 8, 27, 9)),
            commit("Alice", at_utc(2026, 8, 27, 15)),
            commit("Bob", at_utc(2026, 8, 23, 1)),
            // Saturday of the previous week and next week's Sunday stay out.
            commit("Carol", at_utc(2026, 8, 22, 12)),
            commit("Carol", at_utc(2026, 8, 30, 12)),
        ];

        let model = build_statistics_model(&commits, Timezone::Utc, now);

        // Two Thursday commits plus Sunday's; the prior/next-week commits
        // on Aug 22 and Aug 30 stay out.
        assert_eq!(model.week.total_commits, 3);
        assert_eq!(model.week.bars.len(), 7);
        assert_eq!(model.week.bars[0], StatisticsBar { label_index: 0, count: 1 });
        assert_eq!(model.week.bars[4], StatisticsBar { label_index: 4, count: 2 });
        assert_eq!(model.week.bars[6], StatisticsBar { label_index: 6, count: 0 });
        assert_eq!(
            model.week.contributors,
            vec![
                StatisticsContributor { author: Arc::from("Alice"), count: 2 },
                StatisticsContributor { author: Arc::from("Bob"), count: 1 },
            ]
        );
    }

    #[test]
    fn timezone_shift_moves_a_commit_across_the_day_boundary() {
        // 23:30 UTC on Thursday Aug 27 is Friday Aug 28 at UTC+8.
        let now = at_utc(2026, 8, 27, 12);
        let time = at_utc(2026, 8, 27, 23) + Duration::from_secs(30 * 60);
        let commits = vec![commit("Alice", time)];

        let utc = build_statistics_model(&commits, Timezone::Utc, now);
        assert_eq!(utc.week.bars[4].count, 1);
        assert_eq!(utc.week.bars[5].count, 0);

        let plus8 = build_statistics_model(&commits, Timezone::Fixed(8 * 3600), now);
        assert_eq!(plus8.week.bars[4].count, 0);
        assert_eq!(plus8.week.bars[5].count, 1);
    }

    #[test]
    fn month_buckets_by_day_of_month() {
        let now = at_utc(2026, 8, 27, 12);
        let commits = vec![
            commit("Alice", at_utc(2026, 8, 1, 6)),
            commit("Bob", at_utc(2026, 8, 15, 6)),
            commit("Bob", at_utc(2026, 8, 15, 20)),
            commit("Alice", at_utc(2026, 8, 31, 23)),
            // July's last day belongs to July's chart.
            commit("Carol", at_utc(2026, 7, 31, 12)),
        ];

        let model = build_statistics_model(&commits, Timezone::Utc, now);

        assert_eq!(model.month.total_commits, 4);
        assert_eq!(model.month.bars.len(), 31);
        assert_eq!(model.month.bars[0].label_index, 1);
        assert_eq!(model.month.bars[0].count, 1);
        assert_eq!(model.month.bars[14].count, 2);
        assert_eq!(model.month.bars[30].label_index, 31);
        assert_eq!(model.month.bars[30].count, 1);
    }

    #[test]
    fn year_buckets_by_calendar_month() {
        let now = at_utc(2026, 8, 27, 12);
        let commits = vec![
            commit("Alice", at_utc(2026, 1, 15, 6)),
            commit("Bob", at_utc(2026, 2, 1, 6)),
            commit("Bob", at_utc(2026, 2, 28, 23)),
            // Last day of the previous year stays out of 2026.
            commit("Carol", at_utc(2025, 12, 31, 23)),
        ];

        let model = build_statistics_model(&commits, Timezone::Utc, now);

        assert_eq!(model.year.total_commits, 3);
        assert_eq!(model.year.bars.len(), 12);
        assert_eq!(model.year.bars[0].label_index, 1);
        assert_eq!(model.year.bars[0].count, 1);
        assert_eq!(model.year.bars[1].count, 2);
        assert_eq!(model.year.bars[11].label_index, 12);
        assert_eq!(model.year.bars[11].count, 0);
    }

    #[test]
    fn contributors_sort_count_desc_then_name_asc() {
        let now = at_utc(2026, 8, 27, 12);
        let commits = vec![
            commit("Bob", at_utc(2026, 8, 26, 6)),
            commit("Bob", at_utc(2026, 8, 25, 6)),
            commit("Alice", at_utc(2026, 8, 24, 6)),
            commit("Alice", at_utc(2026, 8, 23, 6)),
            commit("Carol", at_utc(2026, 8, 27, 6)),
        ];

        let model = build_statistics_model(&commits, Timezone::Utc, now);
        let names: Vec<&str> = model
            .week
            .contributors
            .iter()
            .map(|c| &*c.author)
            .collect();
        assert_eq!(names, vec!["Alice", "Bob", "Carol"]);
        assert!(model.week.contributors[0].count >= model.week.contributors[1].count);
    }

    #[test]
    fn an_empty_window_renders_a_full_axis_of_zero_bars() {
        let model = build_statistics_model(&[], Timezone::Utc, at_utc(2026, 8, 27, 12));

        for period in [&model.week, &model.month, &model.year] {
            assert_eq!(period.total_commits, 0);
            assert!(period.contributors.is_empty());
            assert!(period.bars.iter().all(|bar| bar.count == 0));
        }
        assert_eq!(model.week.bars.len(), 7);
        assert_eq!(model.month.bars.len(), 31);
        assert_eq!(model.year.bars.len(), 12);
    }
}
