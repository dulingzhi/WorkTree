//! The resolved-output (merge result) editor: live syntax, provenance outline
//! recomputation and the streamed/materialized output projection.
//!
//! Every item is re-exported below, so `resolved_output_syntax::X` still names the same thing it always did.

mod entry;
mod impl_caches;
mod impl_outline;
mod impl_projection;
mod impl_syntax;
mod outline;

use super::*;
pub(in crate::view::panes::main::core_impl) use entry::{
    new_conflict_resolver_input, resolved_output_measure_row,
};
