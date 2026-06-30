//! Template builder foundation — ready-check command generators, build-log
//! types, and build-status/tag public wrappers.
//!
//! This module provides:
//! - [`ReadyCmd`] and the five free functions that produce POSIX shell command
//!   strings used to signal that a sandbox is ready to serve traffic.
//! - [`LogEntry`] / [`LogEntryLevel`] — structured log entries emitted during
//!   a template build (ANSI-stripped).
//! - [`BuildStatus`], [`BuildStatusReason`], [`TemplateTag`], and
//!   [`TemplateBuildStatusResponse`] — status/tag wrapper types that map the
//!   generated API schema types to an ergonomic public API.
//!
//! HTTP calls and full build orchestration arrive in later milestones.

pub mod log;
pub mod readycmd;
pub mod types;

pub use log::{LogEntry, LogEntryLevel};
pub use readycmd::{
    ReadyCmd, wait_for_file, wait_for_port, wait_for_process, wait_for_timeout, wait_for_url,
};
pub use types::{BuildStatus, BuildStatusReason, TemplateBuildStatusResponse, TemplateTag};
