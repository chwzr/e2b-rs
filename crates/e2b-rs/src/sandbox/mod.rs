//! Sandbox APIs. The full sandbox lifecycle and I/O modules arrive in later
//! milestones; this milestone provides URL signatures.

pub(crate) mod api;
pub(crate) mod filesystem;
pub(crate) mod mcp_gen;
pub(crate) mod network;
pub mod opts;
pub(crate) mod paginator;
#[allow(clippy::module_inception)] // `sandbox.rs` inside the `sandbox` module is intentional
pub(crate) mod sandbox;
pub mod signature;
pub(crate) mod snapshot_paginator;
pub mod types;

pub use filesystem::{
    EntryInfo, FileType, Filesystem, FilesystemEvent, FilesystemEventType, FsWriteOpts,
    WatchHandle, WatchOpts, WriteEntry, WriteInfo,
};
pub use network::{NetworkRule, SandboxNetworkUpdate};
pub use opts::{
    SandboxConnectOpts, SandboxCreateOpts, SandboxListOpts, SandboxUrlOpts, SnapshotListOpts,
};
pub use paginator::SandboxPaginator;
pub use sandbox::{Sandbox, SandboxConnectBuilder, SandboxCreateBuilder};
pub use snapshot_paginator::SnapshotPaginator;
pub use types::{SandboxInfo, SandboxMetrics, SandboxState, SnapshotInfo};
