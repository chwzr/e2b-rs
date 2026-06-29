//! # e2b-rs
//!
//! Rust SDK for [E2B](https://e2b.dev) — cloud sandboxes for AI agents. A 1:1
//! port of the official JavaScript SDK with an idiomatic async API.
//!
//! This crate is built in milestones. This release provides the **foundation
//! layer**: configuration, errors, logging, pagination state, and URL
//! signatures. Sandbox creation, command execution, and the filesystem API
//! arrive in later milestones.

mod utils;

pub mod errors;

pub use errors::{Error, Result};

pub mod logs;

pub use logs::{Logger, NoopLogger};
