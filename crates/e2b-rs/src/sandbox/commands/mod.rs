//! Sandbox commands and PTY APIs (Process service).
//!
//! This module provides the reusable handle infrastructure. The `Commands`
//! struct and its high-level `run`/`list`/`connect` methods arrive in later
//! tasks.

pub(crate) mod handle;
pub mod types;

pub use handle::CommandHandle;
pub use types::{CommandOutput, CommandResult, ProcessInfo};

#[allow(unused_imports)] // used by Task 2+ callers (run, connect, pty)
pub(crate) use handle::{open_handle, pid_selector};
