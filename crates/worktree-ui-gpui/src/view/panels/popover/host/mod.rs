//! The popover host: the popover/picker kinds, the per-picker state each one owns,
//! and the `PopoverHost` methods that drive them.
//!
//! Every item is re-exported below, so `host::X` still names the same thing it always did.

mod host;
mod impl_new;
mod impl_settings;
mod impl_state;
mod impl_sync;
mod impl_tests_api;
mod kinds;
mod state;

pub(in crate::view) use host::PopoverHost;
#[cfg(test)]
pub(in crate::view) use host::RemoteRow;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use host::{benchmark_branch_checkout_rows, benchmark_workspace_rows};
pub(in crate::view) use kinds::{
    AutosquashMode, BranchPickerPurpose, PopoverKind, RemotePickerPurpose, RemotePopoverKind,
    RepoPopoverKind, StashPickerPurpose, SubmodulePopoverKind, WorktreePopoverKind,
};
