//! Sandbox APIs. The full sandbox lifecycle and I/O modules arrive in later
//! milestones; this milestone provides URL signatures.

pub(crate) mod api;
pub(crate) mod mcp_gen;
pub mod opts;
pub(crate) mod paginator;
#[allow(clippy::module_inception)] // `sandbox.rs` inside the `sandbox` module is intentional
pub(crate) mod sandbox;
pub mod signature;
pub mod types;

pub use opts::{SandboxConnectOpts, SandboxCreateOpts, SandboxListOpts};
pub use paginator::SandboxPaginator;
pub use sandbox::{Sandbox, SandboxConnectBuilder, SandboxCreateBuilder};
pub use types::{SandboxInfo, SandboxState};
