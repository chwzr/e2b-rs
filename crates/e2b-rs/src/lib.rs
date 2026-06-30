//! # e2b-rs
//!
//! Rust SDK for [E2B](https://e2b.dev) — cloud sandboxes for AI agents. A 1:1
//! port of the official JavaScript SDK with an idiomatic async API.
//!
//! This crate is built in milestones. This release provides the **foundation
//! layer**: configuration, errors, logging, pagination state, and URL
//! signatures. Sandbox creation, command execution, and the filesystem API
//! arrive in later milestones.
//!
//! ## Creating a sandbox
//!
//! ```no_run
//! # async fn run() -> e2b_rs::Result<()> {
//! use e2b_rs::Sandbox;
//!
//! let sandbox = Sandbox::create().template("base").await?;
//! let info = sandbox.get_info().await?;
//! assert_eq!(info.state, e2b_rs::SandboxState::Running);
//! sandbox.kill().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Pausing, metrics, and snapshots
//!
//! ```no_run
//! # async fn run() -> e2b_rs::Result<()> {
//! use e2b_rs::Sandbox;
//! let sandbox = Sandbox::create().template("base").await?;
//! let metrics = sandbox.get_metrics().await?;
//! println!("{} samples", metrics.len());
//! let snap = sandbox.create_snapshot(Some("nightly".to_string())).await?;
//! println!("snapshot {}", snap.snapshot_id);
//! sandbox.pause().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Resolving configuration
//!
//! ```
//! use e2b_rs::{ConnectionConfig, ConnectionConfigOpts};
//!
//! let config = ConnectionConfig::new(ConnectionConfigOpts {
//!     domain: Some("e2b.app".to_string()),
//!     ..Default::default()
//! });
//! assert_eq!(config.domain, "e2b.app");
//! ```

pub(crate) mod connect;

pub(crate) mod http;

pub(crate) mod envd;

pub(crate) mod volume;

pub(crate) mod api;

mod utils;

pub mod errors;

pub use errors::{Error, Result};

pub mod logs;

pub use logs::{Logger, NoopLogger};

pub mod connection_config;

pub use connection_config::{
    ConnectionConfig, ConnectionConfigOpts, DEFAULT_SANDBOX_TIMEOUT_MS, REQUEST_TIMEOUT_MS,
};

pub mod sandbox;

pub use sandbox::signature::{Signature, SignatureOperation, get_signature, get_signature_now};
pub use sandbox::{
    EntryInfo, FileType, Filesystem, FilesystemEvent, FilesystemEventType, FsWriteOpts,
    NetworkRule, Sandbox, SandboxConnectBuilder, SandboxConnectOpts, SandboxCreateBuilder,
    SandboxCreateOpts, SandboxInfo, SandboxListOpts, SandboxMetrics, SandboxNetworkUpdate,
    SandboxPaginator, SandboxState, SandboxUrlOpts, SnapshotInfo, SnapshotListOpts,
    SnapshotPaginator, WriteEntry, WriteInfo,
};

pub mod paginator;

pub use paginator::PaginationState;
