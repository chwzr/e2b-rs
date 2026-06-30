//! Sandbox Git API — public types and internal utilities.
//!
//! The [`crate::sandbox::git`] module exposes the result types produced by git
//! operations. The [`crate::sandbox::git::util`] sub-module contains the pure
//! (no-I/O) helpers that build commands and parse output; the `Git` struct that
//! drives actual sandbox commands arrives in a later task.

pub mod types;
pub(crate) mod util;

pub use types::{GitBranches, GitConfigScope, GitFileStatus, GitStatus, GitStatusLabel};
