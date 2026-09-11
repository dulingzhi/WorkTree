//! Resolver bootstrap and session sync: the mergetool trace context, source
//! fingerprints, the bootstrap model, and the `MainPaneView` methods that drive them.
//!
//! Every item is re-exported below, so `bootstrap::X` still names the same thing it always did.

mod fingerprint;
mod impl_bootstrap;
mod impl_session;
mod impl_view;
mod model;
mod trace;

#[cfg(test)]
mod tests;

use super::*;

#[cfg(test)]
use fingerprint::conflict_file_source_fingerprint;
#[cfg(test)]
use model::conflict_session_plan_projection;
