//! Template builder foundation — ready-check command generators.
//!
//! This module provides [`ReadyCmd`] and the five free functions that produce
//! POSIX shell command strings used to signal that a sandbox is ready to serve
//! traffic. It is the first piece of the template builder; HTTP calls and
//! state management arrive in later milestones.

pub mod readycmd;

pub use readycmd::{
    ReadyCmd, wait_for_file, wait_for_port, wait_for_process, wait_for_timeout, wait_for_url,
};
