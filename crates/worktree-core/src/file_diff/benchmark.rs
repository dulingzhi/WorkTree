//! Benchmark-only replacement-distance backends (`--features benchmarks`).

use super::align::{
    build_side_by_side_plan_with_pair_cost, replacement_pair_cost,
    replacement_pair_cost_with_distance, replacement_pair_cost_with_shared_boundary,
    shared_boundary_bytes, split_lines,
};
use super::levenshtein::LevenshteinScratch;
use super::plan::{FileDiffPlan, PreparedReplacementLine};

#[cfg(feature = "benchmarks")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BenchmarkReplacementDistanceBackend {
    Scratch,
    Strsim,
}

#[cfg(feature = "benchmarks")]
pub fn benchmark_side_by_side_plan_with_replacement_backend(
    old: &str,
    new: &str,
    backend: BenchmarkReplacementDistanceBackend,
) -> FileDiffPlan {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    match backend {
        BenchmarkReplacementDistanceBackend::Scratch => build_side_by_side_plan_with_pair_cost(
            old,
            new,
            old_lines.as_slice(),
            new_lines.as_slice(),
            replacement_pair_cost_with_scratch,
        ),
        BenchmarkReplacementDistanceBackend::Strsim => build_side_by_side_plan_with_pair_cost(
            old,
            new,
            old_lines.as_slice(),
            new_lines.as_slice(),
            replacement_pair_cost_with_strsim,
        ),
    }
}

#[cfg(feature = "benchmarks")]
pub(super) struct CharSlice<'a>(pub(super) &'a [char]);

#[cfg(feature = "benchmarks")]
impl<'b> IntoIterator for &CharSlice<'b> {
    type Item = char;
    type IntoIter = std::iter::Copied<std::slice::Iter<'b, char>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().copied()
    }
}

#[cfg(all(feature = "benchmarks", test))]
pub(super) struct ByteSlice<'a>(pub(super) &'a [u8]);

#[cfg(all(feature = "benchmarks", test))]
impl<'b> IntoIterator for &ByteSlice<'b> {
    type Item = u8;
    type IntoIter = std::iter::Copied<std::slice::Iter<'b, u8>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().copied()
    }
}

#[cfg(feature = "benchmarks")]
fn replacement_pair_cost_with_scratch(
    old: &PreparedReplacementLine<'_>,
    new: &PreparedReplacementLine<'_>,
    scratch: &mut LevenshteinScratch,
) -> u32 {
    replacement_pair_cost(old, new, scratch)
}

#[cfg(feature = "benchmarks")]
fn replacement_pair_cost_with_strsim(
    old: &PreparedReplacementLine<'_>,
    new: &PreparedReplacementLine<'_>,
    scratch: &mut LevenshteinScratch,
) -> u32 {
    if old.text == new.text {
        return 0;
    }

    if let (Some(old_bytes), Some(new_bytes)) = (old.ascii_bytes(), new.ascii_bytes()) {
        let (shared_prefix, shared_suffix) = shared_boundary_bytes(old_bytes, new_bytes);
        return replacement_pair_cost_with_shared_boundary(
            old_bytes,
            new_bytes,
            shared_prefix,
            shared_suffix,
            |old_trimmed, new_trimmed| {
                u32::try_from(scratch.distance_bytes(old_trimmed, new_trimmed)).unwrap_or(u32::MAX)
            },
        );
    }

    replacement_pair_cost_with_distance(old.chars(), new.chars(), |old_trimmed, new_trimmed| {
        let old_trimmed_wrapper = CharSlice(old_trimmed);
        let new_trimmed_wrapper = CharSlice(new_trimmed);
        u32::try_from(strsim::generic_levenshtein(
            &old_trimmed_wrapper,
            &new_trimmed_wrapper,
        ))
        .unwrap_or(u32::MAX)
    })
}
