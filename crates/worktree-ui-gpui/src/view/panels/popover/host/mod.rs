//! The popover host: the popover/picker kinds, the per-picker state each one owns,
//! and the `PopoverHost` methods that drive them.
//!
//! Every item is re-exported below, so `popover_host::X` still names the same thing it always did.

mod impl_new;
mod impl_settings;
mod impl_state;
mod impl_sync;
mod impl_tests_api;
mod kinds;
mod popover_host;
mod state;

pub(in crate::view) use kinds::{
    AutosquashMode, BranchPickerPurpose, PopoverKind, RemotePickerPurpose, RemotePopoverKind,
    RepoPopoverKind, StashPickerPurpose, SubmodulePopoverKind, WorktreePopoverKind,
};
pub(in crate::view) use popover_host::PopoverHost;
#[cfg(test)]
pub(in crate::view) use popover_host::RemoteRow;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use popover_host::{benchmark_branch_checkout_rows, benchmark_workspace_rows};
