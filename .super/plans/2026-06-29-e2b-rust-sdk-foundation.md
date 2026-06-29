# E2B Rust SDK — Foundation (Plan 1 of 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the foundation layer of `e2b-rs` — workspace, lints, errors, connection config + env resolution, logging, pagination state, URL signatures, and utilities — all pure, fully unit-tested, no network.

**Architecture:** A Cargo workspace (`e2b-rs` repo root) with the published SDK crate at `crates/e2b-rs` (library name `e2b_rs`). This plan delivers only the dependency-free base modules that every later milestone builds on. Each module mirrors a file in the reference JS SDK (`../E2B/packages/js-sdk/src/`) so parity is auditable file-by-file. No HTTP, no codegen, no async runtime yet — those arrive in Plan 2.

**Tech Stack:** Rust 2024 edition, MSRV 1.95.0; `thiserror` (errors), `sha2` + `base64` (signatures). `tokio`/`reqwest`/`serde` are intentionally **not** added until Plan 2.

**Reference spec:** `.super/specs/2026-06-28-e2b-rust-sdk-design.md`
**Reference implementation:** `../E2B/packages/js-sdk/src/{errors,utils,logs,connectionConfig,paginator}.ts` and `sandbox/signature.ts`

## Milestone roadmap (context — this plan is #1)

| Plan | Spec phases | Deliverable |
|---|---|---|
| **1 — Foundation (this plan)** | 1 | errors, config+env, logs, pagination state, signature, utils — unit-tested, no network |
| 2 — Codegen & Transports | 2–3 | vendored types + ApiClient / EnvdApiClient / Connect client |
| 3 — Sandbox & envd I/O | 4–5 | create sandbox, run commands, files, pty, watch |
| 4 — Git & Volume | 6–7 | leaf subsystems |
| 5 — Template & Polish | 8–9 | build pipeline, examples, README, parity checklist, CI |

## Global Constraints

These apply to **every** task; each task's requirements implicitly include them.

- **Crate naming:** workspace/repo root is `e2b-rs`; published package `e2b-rs`, library `e2b_rs`. All crates live under `crates/`.
- **Toolchain:** `edition = "2024"`, `rust-version = "1.95.0"` (MSRV). Pinned via `rust-toolchain.toml`.
- **Lints (panic-free lib):** `clippy::unwrap_used = "deny"` and `clippy::expect_used = "deny"` and `missing_docs = "deny"`, set in `[workspace.lints]`. **No `.unwrap()`, `.expect()`, `panic!`, `unreachable!`, or panicking indexing in non-test library code.** Use `.unwrap_or(..)`, `?`, `match`, `.get(..)` instead. Tests may use `.unwrap()`/`.expect()` (enabled via `clippy.toml`).
- **Docs:** every public item carries a `///` doc comment. Public examples that touch no network are runnable doctests; network examples (later plans) use `no_run`.
- **TDD:** write the failing test first, watch it fail, implement minimally, watch it pass, commit. In Rust the "failing" state for a not-yet-defined symbol is a **compile error** — that counts.
- **Commits:** conventional-commit messages; end every commit with the trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- **Parity source of truth:** when porting behavior, match the cited JS file exactly (string formats, defaults, precedence).

---

### Task 1: Workspace, crate skeleton, lints & CI gates

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `clippy.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `crates/e2b-rs/Cargo.toml`
- Create: `crates/e2b-rs/src/lib.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: a compiling, lint-clean empty library crate `e2b_rs`; `[workspace.dependencies]` table for later tasks to reference; CI running fmt/clippy/test/doc.

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.95.0"
license = "MIT"
repository = "https://github.com/e2b-dev/e2b-rs"
authors = ["E2B"]

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"

[workspace.lints.rust]
missing_docs = "deny"

[workspace.lints.rustdoc]
broken_intra_doc_links = "deny"

[workspace.dependencies]
thiserror = "2"
sha2 = "0.10"
base64 = "0.22"
```

- [ ] **Step 2: Create lint, toolchain, and ignore files**

Create `clippy.toml`:

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.95.0"
components = ["clippy", "rustfmt"]
```

Create `.gitignore`:

```gitignore
/target
**/*.rs.bk
.DS_Store
```

- [ ] **Step 3: Create the SDK crate manifest**

Create `crates/e2b-rs/Cargo.toml`:

```toml
[package]
name = "e2b-rs"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Rust SDK for E2B cloud sandboxes (a 1:1 port of the official JavaScript SDK)"
keywords = ["e2b", "sandbox", "ai-agents", "code-interpreter"]
categories = ["api-bindings", "asynchronous"]

[lib]
name = "e2b_rs"

[lints]
workspace = true

[dependencies]
thiserror = { workspace = true }
sha2 = { workspace = true }
base64 = { workspace = true }
```

- [ ] **Step 4: Create the minimal crate root**

Create `crates/e2b-rs/src/lib.rs`:

```rust
//! # e2b-rs
//!
//! Rust SDK for [E2B](https://e2b.dev) — cloud sandboxes for AI agents. A 1:1
//! port of the official JavaScript SDK with an idiomatic async API.
//!
//! This crate is built in milestones. This release provides the **foundation
//! layer**: configuration, errors, logging, pagination state, and URL
//! signatures. Sandbox creation, command execution, and the filesystem API
//! arrive in later milestones.
```

- [ ] **Step 5: Create the CI workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.95.0
        with:
          components: clippy, rustfmt
      - name: Format
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --all-targets --all-features -- -D warnings
      - name: Test
        run: cargo test --all-features
      - name: Doctests
        run: cargo test --doc --all-features
      - name: Docs
        run: cargo doc --no-deps --all-features
```

- [ ] **Step 6: Verify the skeleton builds and is lint-clean**

Run: `cargo build`
Expected: compiles successfully (an empty library).

Run: `cargo fmt --all --check`
Expected: no output, exit 0.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings, exit 0.

Run: `cargo test`
Expected: `0 passed; 0 failed` (no tests yet), exit 0.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml clippy.toml rust-toolchain.toml .gitignore crates .github
git commit -m "chore: scaffold e2b-rs workspace, lints, and CI" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Error types (`errors.rs`)

Port `../E2B/packages/js-sdk/src/errors.ts`. JS uses class inheritance; Rust uses one `#[non_exhaustive]` enum plus predicate helpers (`is_not_found`, `is_authentication`, `is_build`) to recover the `instanceof`-based groupings, and a `from_status` mapper.

**Files:**
- Create: `crates/e2b-rs/src/errors.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (add module + re-exports)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum Error` with variants: `Sandbox(String)`, `Timeout(String)`, `InvalidArgument(String)`, `NotEnoughSpace(String)`, `NotFound(String)`, `FileNotFound(String)`, `SandboxNotFound(String)`, `Authentication(String)`, `GitAuth(String)`, `GitUpstream(String)`, `Template(String)`, `RateLimit(String)`, `Build(String)`, `FileUpload(String)`, `Volume(String)`, `CommandExit { exit_code: i32, stdout: String, stderr: String, error: Option<String> }`, `Internal(String)`. (`Transport(#[from] reqwest::Error)` is added in Plan 2.)
  - `pub type Result<T> = std::result::Result<T, Error>;`
  - `pub fn Error::from_status(status: u16, message: impl Into<String>) -> Error`
  - `pub fn format_sandbox_timeout_error(message: impl Into<String>) -> Error`
  - `pub fn Error::is_not_found(&self) -> bool`, `is_authentication(&self) -> bool`, `is_build(&self) -> bool`

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/errors.rs`:

```rust
//! Error types for the E2B SDK.
//!
//! Mirrors the error hierarchy of the JavaScript SDK (`src/errors.ts`). Rust
//! has no class inheritance, so the JS subclass relationships are modeled as
//! sibling variants plus the [`Error::is_not_found`], [`Error::is_authentication`],
//! and [`Error::is_build`] predicates.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_status_maps_known_codes() {
        assert!(matches!(Error::from_status(400, "x"), Error::InvalidArgument(_)));
        assert!(matches!(Error::from_status(401, "x"), Error::Authentication(_)));
        assert!(matches!(Error::from_status(404, "x"), Error::NotFound(_)));
        assert!(matches!(Error::from_status(429, "x"), Error::RateLimit(_)));
        assert!(matches!(Error::from_status(507, "x"), Error::NotEnoughSpace(_)));
        assert!(matches!(Error::from_status(500, "x"), Error::Sandbox(_)));
    }

    #[test]
    fn from_status_502_is_timeout_with_hint() {
        let err = Error::from_status(502, "boom");
        match err {
            Error::Timeout(msg) => {
                assert!(msg.contains("boom"));
                assert!(msg.contains("sandbox timeout"));
            }
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn not_found_predicate_groups_subtypes() {
        assert!(Error::NotFound("a".into()).is_not_found());
        assert!(Error::FileNotFound("a".into()).is_not_found());
        assert!(Error::SandboxNotFound("a".into()).is_not_found());
        assert!(!Error::Sandbox("a".into()).is_not_found());
    }

    #[test]
    fn auth_and_build_predicates_group_subtypes() {
        assert!(Error::Authentication("a".into()).is_authentication());
        assert!(Error::GitAuth("a".into()).is_authentication());
        assert!(!Error::Sandbox("a".into()).is_authentication());

        assert!(Error::Build("a".into()).is_build());
        assert!(Error::FileUpload("a".into()).is_build());
        assert!(!Error::Sandbox("a".into()).is_build());
    }

    #[test]
    fn display_renders_message_and_command_exit() {
        assert_eq!(Error::Sandbox("boom".into()).to_string(), "boom");
        let ce = Error::CommandExit {
            exit_code: 2,
            stdout: "out".into(),
            stderr: "err".into(),
            error: Some("bad".into()),
        };
        assert!(ce.to_string().contains("exited with code 2"));
    }
}
```

Add to `crates/e2b-rs/src/lib.rs` (after the crate docs):

```rust
pub mod errors;

pub use errors::{Error, Result};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p e2b-rs errors`
Expected: FAIL — compile error `cannot find type/function ... in this scope` (e.g. `Error`, `from_status`).

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `errors.rs`:

```rust
/// Convenient result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All errors returned by the E2B SDK.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// General sandbox error (JS `SandboxError`).
    #[error("{0}")]
    Sandbox(String),
    /// A timeout, often caused by the sandbox itself timing out (JS `TimeoutError`).
    #[error("{0}")]
    Timeout(String),
    /// An invalid argument was supplied (JS `InvalidArgumentError`).
    #[error("{0}")]
    InvalidArgument(String),
    /// The sandbox ran out of disk space (JS `NotEnoughSpaceError`).
    #[error("{0}")]
    NotEnoughSpace(String),
    /// A resource was not found (JS deprecated `NotFoundError`).
    #[error("{0}")]
    NotFound(String),
    /// A file or directory was not found inside the sandbox (JS `FileNotFoundError`).
    #[error("{0}")]
    FileNotFound(String),
    /// The sandbox was not found or is no longer running (JS `SandboxNotFoundError`).
    #[error("{0}")]
    SandboxNotFound(String),
    /// Authentication failed (JS `AuthenticationError`).
    #[error("{0}")]
    Authentication(String),
    /// Git authentication failed (JS `GitAuthError`).
    #[error("{0}")]
    GitAuth(String),
    /// Git upstream tracking is missing (JS `GitUpstreamError`).
    #[error("{0}")]
    GitUpstream(String),
    /// The template uses an incompatible envd version (JS `TemplateError`).
    #[error("{0}")]
    Template(String),
    /// The API rate limit was exceeded (JS `RateLimitError`).
    #[error("{0}")]
    RateLimit(String),
    /// A template build failed (JS `BuildError`).
    #[error("{0}")]
    Build(String),
    /// A build file upload failed (JS `FileUploadError`).
    #[error("{0}")]
    FileUpload(String),
    /// A volume operation failed (JS `VolumeError`).
    #[error("{0}")]
    Volume(String),
    /// A command exited with a non-zero status (JS `CommandExitError`).
    #[error("command exited with code {exit_code}")]
    CommandExit {
        /// Process exit code.
        exit_code: i32,
        /// Accumulated stdout.
        stdout: String,
        /// Accumulated stderr.
        stderr: String,
        /// Optional error string reported by envd.
        error: Option<String>,
    },
    /// Internal invariant violation. Used instead of panicking in "impossible"
    /// cases so the library never aborts the host process.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Build the sandbox-timeout error message used for 502/Unavailable responses,
/// matching `formatSandboxTimeoutError` in the JS SDK.
pub fn format_sandbox_timeout_error(message: impl Into<String>) -> Error {
    let message = message.into();
    Error::Timeout(format!(
        "{message}: This error is likely due to sandbox timeout. You can modify the \
         sandbox timeout by passing 'timeoutMs' when starting the sandbox or calling \
         '.setTimeout' on the sandbox with the desired timeout."
    ))
}

impl Error {
    /// Map an HTTP status code to a typed error. Mirrors the envd/control-plane
    /// default error maps in the JS SDK (the comprehensive superset).
    pub fn from_status(status: u16, message: impl Into<String>) -> Error {
        let message = message.into();
        match status {
            400 => Error::InvalidArgument(message),
            401 => Error::Authentication(message),
            404 => Error::NotFound(message),
            429 => Error::RateLimit(message),
            502 => format_sandbox_timeout_error(message),
            507 => Error::NotEnoughSpace(message),
            _ => Error::Sandbox(message),
        }
    }

    /// True for `NotFound` and its JS subtypes (`FileNotFound`, `SandboxNotFound`).
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Error::NotFound(_) | Error::FileNotFound(_) | Error::SandboxNotFound(_)
        )
    }

    /// True for `Authentication` and its JS subtype (`GitAuth`).
    pub fn is_authentication(&self) -> bool {
        matches!(self, Error::Authentication(_) | Error::GitAuth(_))
    }

    /// True for `Build` and its JS subtype (`FileUpload`).
    pub fn is_build(&self) -> bool {
        matches!(self, Error::Build(_) | Error::FileUpload(_))
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p e2b-rs errors`
Expected: PASS — 5 tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/e2b-rs/src/errors.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(errors): add Error enum, status mapping, and predicates" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Utilities (`utils.rs`)

Port the pure helpers from `../E2B/packages/js-sdk/src/utils.ts` that the foundation needs: `sha256` (base64), `timeoutToSeconds` (ceil), `shellQuote` (shlex.quote port), and a User-Agent builder. (`stripAnsi`/`toBlob`/`toUploadBody` are deferred to the milestones that consume them.) This module is **crate-internal** (`mod utils;`).

**Files:**
- Create: `crates/e2b-rs/src/utils.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (add `mod utils;`)

**Interfaces:**
- Consumes: nothing.
- Produces (all `pub(crate)`):
  - `fn sha256_base64(data: &str) -> String` — standard base64 (with `=` padding) of the SHA-256 digest.
  - `fn timeout_to_seconds(ms: u64) -> u64` — `ceil(ms / 1000)`.
  - `fn shell_quote(s: &str) -> String` — POSIX shell quoting matching Python `shlex.quote`.
  - `fn build_user_agent(integration: Option<&str>) -> String`.

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/utils.rs`:

```rust
//! Internal utility functions ported from the JS SDK's `utils.ts`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_base64_known_vectors() {
        // Standard test vectors for SHA-256, base64-encoded with padding.
        assert_eq!(sha256_base64(""), "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
        assert_eq!(sha256_base64("abc"), "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=");
    }

    #[test]
    fn timeout_to_seconds_rounds_up() {
        assert_eq!(timeout_to_seconds(0), 0);
        assert_eq!(timeout_to_seconds(1), 1);
        assert_eq!(timeout_to_seconds(1000), 1);
        assert_eq!(timeout_to_seconds(1001), 2);
        assert_eq!(timeout_to_seconds(300_000), 300);
    }

    #[test]
    fn shell_quote_matches_shlex() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("abc"), "abc");
        assert_eq!(shell_quote("a_b.c-d/e@f"), "a_b.c-d/e@f");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$x"), "'$x'");
        // Embedded single quote becomes '"'"'
        assert_eq!(shell_quote("it's"), "'it'\"'\"'s'");
    }

    #[test]
    fn user_agent_contains_version() {
        let ua = build_user_agent(None);
        assert!(ua.starts_with("e2b-rs/"));
        let ua2 = build_user_agent(Some("langchain"));
        assert!(ua2.contains("langchain"));
    }
}
```

Add to `crates/e2b-rs/src/lib.rs` (after the crate docs, before `pub mod errors;` is fine — order is cosmetic):

```rust
mod utils;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p e2b-rs utils`
Expected: FAIL — compile error, functions not found.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `utils.rs`:

```rust
use base64::prelude::{Engine as _, BASE64_STANDARD};
use sha2::{Digest, Sha256};

/// SHA-256 of `data`, encoded as standard base64 (with `=` padding), matching
/// the JS `sha256` helper.
pub(crate) fn sha256_base64(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    BASE64_STANDARD.encode(hasher.finalize())
}

/// Convert milliseconds to whole seconds, rounding up (JS `timeoutToSeconds`).
pub(crate) fn timeout_to_seconds(ms: u64) -> u64 {
    ms.div_ceil(1000)
}

/// True for characters Python's `shlex.quote` leaves unquoted: `[A-Za-z0-9_]`
/// plus `@%+=:,./-`.
fn is_safe_shell_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-')
}

/// Quote a string for safe interpolation into a POSIX shell command. Faithful
/// port of Python's `shlex.quote` (the JS `shellQuote`): empty becomes `''`,
/// all-safe strings are returned unchanged, otherwise single-quote-wrapped with
/// embedded single quotes escaped as `'"'"'`.
pub(crate) fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().all(is_safe_shell_char) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// Build the `User-Agent` header value, optionally tagging an integration.
pub(crate) fn build_user_agent(integration: Option<&str>) -> String {
    let base = concat!("e2b-rs/", env!("CARGO_PKG_VERSION"));
    match integration {
        Some(name) if !name.is_empty() => format!("{base} ({name})"),
        _ => base.to_string(),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p e2b-rs utils`
Expected: PASS — 4 tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/e2b-rs/src/utils.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(utils): add sha256, timeout conversion, shell quoting, user-agent" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Logger trait (`logs.rs`)

Port the `Logger` interface from `../E2B/packages/js-sdk/src/logs.ts`. The RPC/API logging middleware (`createRpcLogger`/`createApiLogger`) is transport-layer and belongs to Plan 2; this task defines only the user-facing trait and a no-op implementation.

**Files:**
- Create: `crates/e2b-rs/src/logs.rs`
- Modify: `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub trait Logger: Send + Sync` with `fn debug(&self, message: &str)`, `info`, `warn`, `error` — all defaulting to no-ops.
  - `pub struct NoopLogger;` implementing `Logger`.

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/logs.rs`:

```rust
//! Logging interface for SDK diagnostics.

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Capture {
        msgs: Mutex<Vec<String>>,
    }
    impl Logger for Capture {
        fn info(&self, message: &str) {
            self.msgs.lock().unwrap().push(format!("info:{message}"));
        }
        fn error(&self, message: &str) {
            self.msgs.lock().unwrap().push(format!("error:{message}"));
        }
    }

    #[test]
    fn only_overridden_levels_record() {
        let c = Capture::default();
        c.debug("ignored"); // default no-op
        c.warn("ignored"); // default no-op
        c.info("hello");
        c.error("boom");
        let msgs = c.msgs.lock().unwrap();
        assert_eq!(&*msgs, &["info:hello".to_string(), "error:boom".to_string()]);
    }

    #[test]
    fn noop_logger_is_silent() {
        let logger = NoopLogger;
        logger.debug("x");
        logger.info("x");
        logger.warn("x");
        logger.error("x");
        // No panic, nothing recorded — just verifies it compiles and runs.
    }

    #[test]
    fn logger_is_object_safe() {
        let logger: std::sync::Arc<dyn Logger> = std::sync::Arc::new(NoopLogger);
        logger.info("works behind a trait object");
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub mod logs;

pub use logs::{Logger, NoopLogger};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p e2b-rs logs`
Expected: FAIL — `Logger` / `NoopLogger` not found.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `logs.rs`:

```rust
/// Logging sink for SDK diagnostics, mirroring the JS `Logger` interface.
///
/// All methods default to no-ops, so implementors override only the levels
/// they care about. Pass an `Arc<dyn Logger>` through the connection options.
///
/// ```
/// use e2b_rs::Logger;
/// use std::sync::Mutex;
///
/// #[derive(Default)]
/// struct Collect(Mutex<Vec<String>>);
/// impl Logger for Collect {
///     fn info(&self, message: &str) {
///         if let Ok(mut v) = self.0.lock() {
///             v.push(message.to_string());
///         }
///     }
/// }
///
/// let logger = Collect::default();
/// logger.info("sandbox created");
/// ```
pub trait Logger: Send + Sync {
    /// Log a debug-level message.
    fn debug(&self, message: &str) {
        let _ = message;
    }
    /// Log an info-level message.
    fn info(&self, message: &str) {
        let _ = message;
    }
    /// Log a warning-level message.
    fn warn(&self, message: &str) {
        let _ = message;
    }
    /// Log an error-level message.
    fn error(&self, message: &str) {
        let _ = message;
    }
}

/// A [`Logger`] that discards every message.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopLogger;

impl Logger for NoopLogger {}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p e2b-rs logs`
Expected: PASS — 3 tests pass.

Run: `cargo test --doc -p e2b-rs`
Expected: the `Logger` doctest compiles and runs (PASS).

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/e2b-rs/src/logs.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(logs): add Logger trait and NoopLogger" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Connection config & env resolution (`connection_config.rs`)

Port `../E2B/packages/js-sdk/src/connectionConfig.ts`: constants, options, env-var resolution (with JS's `||`-vs-`??` precedence nuances), and the host/URL construction methods. Env reading is injected so resolution is unit-testable without touching (or racing on) the process environment.

**Files:**
- Create: `crates/e2b-rs/src/connection_config.rs`
- Modify: `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `crate::logs::Logger`, `crate::utils::build_user_agent`.
- Produces:
  - Constants: `pub const REQUEST_TIMEOUT_MS: u64 = 60_000;`, `DEFAULT_SANDBOX_TIMEOUT_MS: u64 = 300_000;`, `KEEPALIVE_PING_INTERVAL_SEC: u64 = 50;`, `KEEPALIVE_PING_HEADER: &str = "Keepalive-Ping-Interval";`, `DEFAULT_USERNAME: &str = "user";`, `ENVD_PORT: u16 = 49983;`
  - `pub struct ConnectionConfigOpts { api_key, validate_api_key, access_token, domain, api_url, sandbox_url, debug, request_timeout_ms, logger, headers, proxy, integration }` (all `Option`/`BTreeMap`, `#[derive(Default, Clone)]`).
  - `pub struct ConnectionConfig { debug, domain, api_url, sandbox_url, logger, request_timeout_ms, api_key, validate_api_key, access_token, integration, headers, proxy }` (`#[derive(Clone)]`).
  - `pub fn ConnectionConfig::new(opts: ConnectionConfigOpts) -> Self` (reads `std::env`).
  - `pub(crate) fn ConnectionConfig::from_env(opts, env: impl Fn(&str) -> Option<String>) -> Self`.
  - `pub fn get_host(&self, sandbox_id: &str, port: u16, sandbox_domain: Option<&str>) -> String`
  - `pub fn get_sandbox_url(&self, sandbox_id: &str, sandbox_domain: &str, envd_port: u16) -> String`
  - `pub fn get_sandbox_direct_url(&self, sandbox_id: &str, sandbox_domain: &str, envd_port: u16) -> String`

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/connection_config.rs`:

```rust
//! Connection configuration and environment-variable resolution.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(opts: ConnectionConfigOpts, env: &[(&str, &str)]) -> ConnectionConfig {
        let map: HashMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ConnectionConfig::from_env(opts, move |k| map.get(k).cloned())
    }

    #[test]
    fn defaults_with_empty_env() {
        let c = cfg(ConnectionConfigOpts::default(), &[]);
        assert_eq!(c.domain, "e2b.app");
        assert_eq!(c.api_url, "https://api.e2b.app");
        assert!(!c.debug);
        assert!(c.validate_api_key);
        assert_eq!(c.request_timeout_ms, REQUEST_TIMEOUT_MS);
        assert_eq!(c.api_key, None);
        assert_eq!(c.headers.get("User-Agent").map(String::as_str), Some("e2b-rs/0.1.0"));
    }

    #[test]
    fn env_domain_flows_into_api_url() {
        let c = cfg(ConnectionConfigOpts::default(), &[("E2B_DOMAIN", "example.com")]);
        assert_eq!(c.domain, "example.com");
        assert_eq!(c.api_url, "https://api.example.com");
    }

    #[test]
    fn opt_overrides_env() {
        let opts = ConnectionConfigOpts {
            domain: Some("opt.dev".to_string()),
            ..Default::default()
        };
        let c = cfg(opts, &[("E2B_DOMAIN", "env.dev")]);
        assert_eq!(c.domain, "opt.dev");
    }

    #[test]
    fn empty_string_opt_is_falsy_and_falls_through() {
        // JS uses `||` for domain: an empty-string opt is falsy, so env wins.
        let opts = ConnectionConfigOpts {
            domain: Some(String::new()),
            ..Default::default()
        };
        let c = cfg(opts, &[("E2B_DOMAIN", "env.dev")]);
        assert_eq!(c.domain, "env.dev");
    }

    #[test]
    fn debug_changes_api_url_and_parses_env() {
        let c = cfg(ConnectionConfigOpts::default(), &[("E2B_DEBUG", "true")]);
        assert!(c.debug);
        assert_eq!(c.api_url, "http://localhost:3000");
    }

    #[test]
    fn validate_api_key_env_false_disables() {
        let c = cfg(ConnectionConfigOpts::default(), &[("E2B_VALIDATE_API_KEY", "false")]);
        assert!(!c.validate_api_key);
    }

    #[test]
    fn get_host_production_and_debug() {
        let prod = cfg(ConnectionConfigOpts::default(), &[]);
        assert_eq!(prod.get_host("sb1", 49983, Some("e2b.app")), "49983-sb1.e2b.app");

        let dbg = cfg(ConnectionConfigOpts::default(), &[("E2B_DEBUG", "true")]);
        assert_eq!(dbg.get_host("sb1", 49983, Some("e2b.app")), "localhost:49983");
    }

    #[test]
    fn sandbox_url_stable_vs_direct() {
        let c = cfg(ConnectionConfigOpts::default(), &[]);
        // Supported domain → stable host.
        assert_eq!(c.get_sandbox_url("sb1", "e2b.app", 49983), "https://sandbox.e2b.app");
        // Unsupported domain → direct host.
        assert_eq!(c.get_sandbox_url("sb1", "custom.io", 49983), "https://49983-sb1.custom.io");
        // Direct URL never uses the stable host, even for supported domains.
        assert_eq!(c.get_sandbox_direct_url("sb1", "e2b.app", 49983), "https://49983-sb1.e2b.app");
    }

    #[test]
    fn sandbox_url_override_and_debug() {
        let opts = ConnectionConfigOpts {
            sandbox_url: Some("https://my.proxy".to_string()),
            ..Default::default()
        };
        let c = cfg(opts, &[]);
        assert_eq!(c.get_sandbox_url("sb1", "e2b.app", 49983), "https://my.proxy");

        let dbg = cfg(ConnectionConfigOpts::default(), &[("E2B_DEBUG", "true")]);
        assert_eq!(dbg.get_sandbox_url("sb1", "e2b.app", 49983), "http://localhost:49983");
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub mod connection_config;

pub use connection_config::{
    ConnectionConfig, ConnectionConfigOpts, DEFAULT_SANDBOX_TIMEOUT_MS, REQUEST_TIMEOUT_MS,
};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p e2b-rs connection_config`
Expected: FAIL — types/functions not found.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `connection_config.rs`:

```rust
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::logs::Logger;
use crate::utils::build_user_agent;

/// Default per-request timeout in milliseconds (60s).
pub const REQUEST_TIMEOUT_MS: u64 = 60_000;
/// Default sandbox lifetime in milliseconds (5 minutes).
pub const DEFAULT_SANDBOX_TIMEOUT_MS: u64 = 300_000;
/// Keepalive ping interval for streaming RPCs, in seconds.
pub const KEEPALIVE_PING_INTERVAL_SEC: u64 = 50;
/// Header carrying the keepalive ping interval.
pub const KEEPALIVE_PING_HEADER: &str = "Keepalive-Ping-Interval";
/// Default sandbox user.
pub const DEFAULT_USERNAME: &str = "user";
/// Port the envd daemon listens on inside a sandbox.
pub const ENVD_PORT: u16 = 49983;

/// Domains for which the stable `sandbox.<domain>` host is guaranteed.
const SUPPORTED_DOMAINS: [&str; 4] = ["e2b.app", "e2b.dev", "e2b.pro", "e2b-staging.dev"];

/// Options for constructing a [`ConnectionConfig`]. Unset (`None`/empty) fields
/// fall back to environment variables and then documented defaults.
#[derive(Default, Clone)]
pub struct ConnectionConfigOpts {
    /// API key; falls back to `E2B_API_KEY`.
    pub api_key: Option<String>,
    /// Whether to validate the API key format; falls back to `E2B_VALIDATE_API_KEY` (default `true`).
    pub validate_api_key: Option<bool>,
    /// Deprecated access token; falls back to `E2B_ACCESS_TOKEN`.
    pub access_token: Option<String>,
    /// Domain; falls back to `E2B_DOMAIN` (default `e2b.app`).
    pub domain: Option<String>,
    /// API base URL; falls back to `E2B_API_URL`.
    pub api_url: Option<String>,
    /// Sandbox base URL override; falls back to `E2B_SANDBOX_URL`.
    pub sandbox_url: Option<String>,
    /// Debug mode; falls back to `E2B_DEBUG` (default `false`).
    pub debug: Option<bool>,
    /// Per-request timeout in milliseconds (default [`REQUEST_TIMEOUT_MS`]).
    pub request_timeout_ms: Option<u64>,
    /// Optional logger.
    pub logger: Option<Arc<dyn Logger>>,
    /// Extra request headers.
    pub headers: BTreeMap<String, String>,
    /// Optional proxy URL.
    pub proxy: Option<String>,
    /// Integration name appended to the `User-Agent`.
    pub integration: Option<String>,
}

/// Resolved connection configuration.
#[derive(Clone)]
pub struct ConnectionConfig {
    /// Debug mode.
    pub debug: bool,
    /// Resolved domain.
    pub domain: String,
    /// Resolved API base URL.
    pub api_url: String,
    /// Optional sandbox base URL override.
    pub sandbox_url: Option<String>,
    /// Optional logger.
    pub logger: Option<Arc<dyn Logger>>,
    /// Per-request timeout in milliseconds.
    pub request_timeout_ms: u64,
    /// Resolved API key.
    pub api_key: Option<String>,
    /// Whether to validate the API key format.
    pub validate_api_key: bool,
    /// Deprecated access token.
    pub access_token: Option<String>,
    /// Integration name.
    pub integration: Option<String>,
    /// Request headers, including the computed `User-Agent`.
    pub headers: BTreeMap<String, String>,
    /// Optional proxy URL.
    pub proxy: Option<String>,
}

/// Return `primary` if it is a non-empty string, else the first non-empty
/// `fallback`. Mirrors JS truthiness for `a || b` where `""` is falsy.
fn first_non_empty(
    primary: Option<String>,
    fallback: impl FnOnce() -> Option<String>,
) -> Option<String> {
    match primary {
        Some(s) if !s.is_empty() => Some(s),
        _ => fallback().filter(|s| !s.is_empty()),
    }
}

impl ConnectionConfig {
    /// Build a configuration from `opts`, reading any unset values from the
    /// process environment.
    pub fn new(opts: ConnectionConfigOpts) -> Self {
        Self::from_env(opts, |key| std::env::var(key).ok())
    }

    /// Build a configuration using an injected environment lookup. Used by
    /// `new` (with `std::env`) and by tests (with a fixed map).
    pub(crate) fn from_env(
        opts: ConnectionConfigOpts,
        env: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let api_key = first_non_empty(opts.api_key, || env("E2B_API_KEY"));
        let access_token = first_non_empty(opts.access_token, || env("E2B_ACCESS_TOKEN"));
        let domain = first_non_empty(opts.domain, || env("E2B_DOMAIN"))
            .unwrap_or_else(|| "e2b.app".to_string());

        let debug = opts.debug.unwrap_or_else(|| {
            env("E2B_DEBUG")
                .map(|v| v.to_ascii_lowercase() == "true")
                .unwrap_or(false)
        });

        let validate_api_key = opts.validate_api_key.unwrap_or_else(|| {
            env("E2B_VALIDATE_API_KEY")
                .map(|v| v.to_ascii_lowercase() != "false")
                .unwrap_or(true)
        });

        let request_timeout_ms = opts.request_timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS);

        let api_url = first_non_empty(opts.api_url, || env("E2B_API_URL")).unwrap_or_else(|| {
            if debug {
                "http://localhost:3000".to_string()
            } else {
                format!("https://api.{domain}")
            }
        });

        let sandbox_url = first_non_empty(opts.sandbox_url, || env("E2B_SANDBOX_URL"));

        let mut headers = opts.headers;
        headers.insert(
            "User-Agent".to_string(),
            build_user_agent(opts.integration.as_deref()),
        );

        Self {
            debug,
            domain,
            api_url,
            sandbox_url,
            logger: opts.logger,
            request_timeout_ms,
            api_key,
            validate_api_key,
            access_token,
            integration: opts.integration,
            headers,
            proxy: opts.proxy,
        }
    }

    /// External host for a sandbox port, e.g. `49983-<id>.e2b.app`. In debug
    /// mode returns `localhost:<port>`.
    pub fn get_host(&self, sandbox_id: &str, port: u16, sandbox_domain: Option<&str>) -> String {
        if self.debug {
            return format!("localhost:{port}");
        }
        let domain = sandbox_domain.unwrap_or(&self.domain);
        format!("{port}-{sandbox_id}.{domain}")
    }

    /// Base URL for reaching a sandbox: the override if set, the stable
    /// `sandbox.<domain>` host for supported domains, otherwise the direct host.
    pub fn get_sandbox_url(&self, sandbox_id: &str, sandbox_domain: &str, envd_port: u16) -> String {
        if let Some(url) = &self.sandbox_url {
            return url.clone();
        }
        if self.debug {
            return format!(
                "http://{}",
                self.get_host(sandbox_id, envd_port, Some(sandbox_domain))
            );
        }
        if SUPPORTED_DOMAINS.contains(&sandbox_domain) {
            return format!("https://sandbox.{sandbox_domain}");
        }
        format!(
            "https://{}",
            self.get_host(sandbox_id, envd_port, Some(sandbox_domain))
        )
    }

    /// Direct sandbox host URL, never using the stable-domain fallback.
    pub fn get_sandbox_direct_url(
        &self,
        sandbox_id: &str,
        sandbox_domain: &str,
        envd_port: u16,
    ) -> String {
        if let Some(url) = &self.sandbox_url {
            return url.clone();
        }
        let scheme = if self.debug { "http" } else { "https" };
        format!(
            "{scheme}://{}",
            self.get_host(sandbox_id, envd_port, Some(sandbox_domain))
        )
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p e2b-rs connection_config`
Expected: PASS — 9 tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/e2b-rs/src/connection_config.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(config): add ConnectionConfig with env resolution and URL helpers" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: URL signatures (`sandbox/signature.rs`)

Port `../E2B/packages/js-sdk/src/sandbox/signature.ts`. Pure SHA-256 signature assembly with an injected clock for deterministic tests. Creates the `sandbox` module (further populated in Plan 3).

**Files:**
- Create: `crates/e2b-rs/src/sandbox/mod.rs`
- Create: `crates/e2b-rs/src/sandbox/signature.rs`
- Modify: `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `crate::utils::sha256_base64`, `crate::errors::{Error, Result}`.
- Produces:
  - `pub enum SignatureOperation { Read, Write }`
  - `pub struct Signature { signature: String, expiration: Option<i64> }`
  - `pub fn get_signature(path: &str, operation: SignatureOperation, user: Option<&str>, expiration_in_seconds: Option<i64>, envd_access_token: Option<&str>, now_unix: i64) -> Result<Signature>`
  - `pub fn get_signature_now(path: &str, operation: SignatureOperation, user: Option<&str>, expiration_in_seconds: Option<i64>, envd_access_token: Option<&str>) -> Result<Signature>`

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/sandbox/mod.rs`:

```rust
//! Sandbox APIs. The full sandbox lifecycle and I/O modules arrive in later
//! milestones; this milestone provides URL signatures.

pub mod signature;
```

Create `crates/e2b-rs/src/sandbox/signature.rs`:

```rust
//! Signed-URL signatures for sandbox file access.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::sha256_base64;

    #[test]
    fn missing_token_errors() {
        let err = get_signature("/f", SignatureOperation::Read, None, None, None, 0).unwrap_err();
        assert!(matches!(err, crate::errors::Error::Sandbox(_)));
    }

    #[test]
    fn unexpiring_signature_matches_assembly() {
        let sig = get_signature("/f", SignatureOperation::Read, None, None, Some("tok"), 0).unwrap();
        let expected_hash = sha256_base64("/f:read::tok");
        let expected = format!("v1_{}", expected_hash.trim_end_matches('='));
        assert_eq!(sig.signature, expected);
        assert_eq!(sig.expiration, None);
        assert!(sig.signature.starts_with("v1_"));
        assert!(!sig.signature.ends_with('='));
    }

    #[test]
    fn expiring_signature_adds_offset_to_now() {
        let sig = get_signature(
            "/f",
            SignatureOperation::Write,
            Some("alice"),
            Some(100),
            Some("tok"),
            1000,
        )
        .unwrap();
        assert_eq!(sig.expiration, Some(1100));
        let expected_hash = sha256_base64("/f:write:alice:tok:1100");
        assert_eq!(sig.signature, format!("v1_{}", expected_hash.trim_end_matches('=')));
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub mod sandbox;

pub use sandbox::signature::{get_signature, get_signature_now, Signature, SignatureOperation};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p e2b-rs signature`
Expected: FAIL — symbols not found.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `sandbox/signature.rs`:

```rust
use crate::errors::{Error, Result};
use crate::utils::sha256_base64;

/// File-system operation a signature authorizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureOperation {
    /// Read access.
    Read,
    /// Write access.
    Write,
}

impl SignatureOperation {
    fn as_str(self) -> &'static str {
        match self {
            SignatureOperation::Read => "read",
            SignatureOperation::Write => "write",
        }
    }
}

/// A computed URL signature and its absolute expiration (unix seconds, if any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// The `v1_`-prefixed signature value.
    pub signature: String,
    /// Absolute expiration as a unix timestamp in seconds, or `None` if it never expires.
    pub expiration: Option<i64>,
}

/// Compute a signature for accessing `path` with `operation` as `user`.
///
/// Mirrors the JS `getSignature`. When `expiration_in_seconds` is provided it is
/// added to `now_unix` to form an absolute expiration. `now_unix` is injected so
/// callers (and tests) control the clock; see [`get_signature_now`] for the
/// convenience wrapper that uses the system clock.
///
/// # Errors
/// Returns [`Error::Sandbox`] if `envd_access_token` is missing or empty.
pub fn get_signature(
    path: &str,
    operation: SignatureOperation,
    user: Option<&str>,
    expiration_in_seconds: Option<i64>,
    envd_access_token: Option<&str>,
    now_unix: i64,
) -> Result<Signature> {
    let token = match envd_access_token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Err(Error::Sandbox(
                "Access token is not set and signature cannot be generated!".to_string(),
            ));
        }
    };

    let user = user.unwrap_or("");
    let expiration = expiration_in_seconds.map(|secs| now_unix + secs);
    let op = operation.as_str();

    let raw = match expiration {
        None => format!("{path}:{op}:{user}:{token}"),
        Some(exp) => format!("{path}:{op}:{user}:{token}:{exp}"),
    };

    let hash = sha256_base64(&raw);
    let signature = format!("v1_{}", hash.trim_end_matches('='));

    Ok(Signature {
        signature,
        expiration,
    })
}

/// Like [`get_signature`] but reads the current system time for expiration.
///
/// # Errors
/// Returns [`Error::Sandbox`] if `envd_access_token` is missing or empty.
pub fn get_signature_now(
    path: &str,
    operation: SignatureOperation,
    user: Option<&str>,
    expiration_in_seconds: Option<i64>,
    envd_access_token: Option<&str>,
) -> Result<Signature> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    get_signature(
        path,
        operation,
        user,
        expiration_in_seconds,
        envd_access_token,
        now,
    )
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p e2b-rs signature`
Expected: PASS — 3 tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/e2b-rs/src/sandbox crates/e2b-rs/src/lib.rs
git commit -m "feat(signature): add signed-URL signature computation" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Pagination state (`paginator.rs`)

Port the cursor bookkeeping of `../E2B/packages/js-sdk/src/paginator.ts`. The JS abstract `Paginator` becomes a reusable `PaginationState` struct; concrete list paginators (which perform HTTP) compose it in Plan 3.

**Files:**
- Create: `crates/e2b-rs/src/paginator.rs`
- Modify: `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct PaginationState` with `pub fn new(limit: Option<u32>, next_token: Option<String>) -> Self`, `pub fn has_next(&self) -> bool`, `pub fn next_token(&self) -> Option<&str>`, `pub fn limit(&self) -> Option<u32>`, `pub fn update_from_token(&mut self, token: Option<String>)`.

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/paginator.rs`:

```rust
//! Cursor-based pagination state shared by list endpoints.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_has_next_true() {
        let s = PaginationState::new(None, None);
        assert!(s.has_next());
        assert_eq!(s.next_token(), None);
        assert_eq!(s.limit(), None);
    }

    #[test]
    fn initial_token_and_limit_are_stored() {
        let s = PaginationState::new(Some(50), Some("cursor".to_string()));
        assert!(s.has_next());
        assert_eq!(s.next_token(), Some("cursor"));
        assert_eq!(s.limit(), Some(50));
    }

    #[test]
    fn nonempty_token_continues_pagination() {
        let mut s = PaginationState::new(None, None);
        s.update_from_token(Some("next".to_string()));
        assert!(s.has_next());
        assert_eq!(s.next_token(), Some("next"));
    }

    #[test]
    fn empty_or_missing_token_ends_pagination() {
        let mut s = PaginationState::new(None, Some("start".to_string()));
        s.update_from_token(Some(String::new()));
        assert!(!s.has_next());
        assert_eq!(s.next_token(), None);

        let mut s2 = PaginationState::new(None, Some("start".to_string()));
        s2.update_from_token(None);
        assert!(!s2.has_next());
        assert_eq!(s2.next_token(), None);
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub mod paginator;

pub use paginator::PaginationState;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p e2b-rs paginator`
Expected: FAIL — `PaginationState` not found.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `paginator.rs`:

```rust
/// Shared pagination bookkeeping (`has_next` + `next_token`), mirroring the JS
/// `Paginator` base. Concrete list types own an instance and call
/// [`PaginationState::update_from_token`] after fetching each page.
#[derive(Debug, Clone)]
pub struct PaginationState {
    has_next: bool,
    next_token: Option<String>,
    limit: Option<u32>,
}

impl PaginationState {
    /// Create state for a fresh paginator. `has_next` starts `true` so the
    /// first page is always fetched.
    pub fn new(limit: Option<u32>, next_token: Option<String>) -> Self {
        Self {
            has_next: true,
            next_token,
            limit,
        }
    }

    /// Whether more items remain to fetch.
    pub fn has_next(&self) -> bool {
        self.has_next
    }

    /// The cursor for the next page, if any.
    pub fn next_token(&self) -> Option<&str> {
        self.next_token.as_deref()
    }

    /// The requested page-size hint, if any.
    pub fn limit(&self) -> Option<u32> {
        self.limit
    }

    /// Update from a response's `x-next-token` value. An empty or absent token
    /// ends pagination (`has_next` becomes `false`).
    pub fn update_from_token(&mut self, token: Option<String>) {
        self.next_token = token.filter(|t| !t.is_empty());
        self.has_next = self.next_token.is_some();
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p e2b-rs paginator`
Expected: PASS — 4 tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/e2b-rs/src/paginator.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(paginator): add cursor-based pagination state" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Crate quickstart doctest, README, parity checklist & full gate

Tie the foundation together: a runnable crate-level example, a repo README, a parity checklist seeded with the foundation rows, and a full run of every CI gate.

**Files:**
- Modify: `crates/e2b-rs/src/lib.rs` (add crate-level runnable example)
- Create: `README.md`
- Create: `docs/parity-checklist.md`

**Interfaces:**
- Consumes: all foundation modules.
- Produces: a documented public API surface for the foundation milestone.

- [ ] **Step 1: Add a runnable crate-level example**

Replace the crate docs at the top of `crates/e2b-rs/src/lib.rs` with:

```rust
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
//! ## Resolving configuration
//!
//! ```
//! use e2b_rs::{ConnectionConfig, ConnectionConfigOpts};
//!
//! let config = ConnectionConfig::new(ConnectionConfigOpts {
//!     domain: Some("e2b.app".to_string()),
//!     ..Default::default()
//! });
//! assert_eq!(config.api_url, "https://api.e2b.app");
//! ```
```

(Leave the `pub mod` declarations and `pub use` re-exports added in earlier tasks below the docs.)

- [ ] **Step 2: Verify the crate-level doctest runs**

Run: `cargo test --doc -p e2b-rs`
Expected: PASS — the config doctest and the `Logger` doctest both run (2 doctests).

- [ ] **Step 3: Create the README**

Create `README.md`:

````markdown
# e2b-rs

Rust SDK for [E2B](https://e2b.dev) — cloud sandboxes for AI agents. A 1:1 port
of the official [JavaScript SDK](https://github.com/e2b-dev/e2b/tree/main/packages/js-sdk),
built to feel familiar while reading as idiomatic async Rust.

> **Status:** under active development, built in milestones. The foundation
> layer (configuration, errors, logging, pagination, signatures) is in place;
> sandbox creation, commands, filesystem, git, volumes, and templates follow.

## Installation

```toml
[dependencies]
e2b-rs = "0.1"
```

The library is imported as `e2b_rs`:

```rust
use e2b_rs::{ConnectionConfig, ConnectionConfigOpts};
```

## Design

- **Async-only** on `tokio`.
- **Channels, not callbacks:** streaming output (commands, pty, watch, build
  logs) is delivered through `tokio::sync::mpsc` receivers.
- **Panic-free library code:** `unwrap`/`expect` are denied outside tests.
- **MSRV:** Rust 1.95.0, edition 2024.

See `.super/specs/` for the full design and `docs/parity-checklist.md` for the
JS-to-Rust parity matrix.

## License

MIT
````

- [ ] **Step 4: Create the parity checklist**

Create `docs/parity-checklist.md`:

```markdown
# JS → Rust parity checklist

Tracks 1:1 coverage of the E2B JavaScript SDK (`packages/js-sdk`). Each row maps
a JS export to its `e2b-rs` equivalent. Status: ✅ done · 🔶 in progress · ⬜ planned.

## Foundation (Plan 1)

| JS (`src/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `errors.ts` (error classes) | `errors::Error` + `is_not_found`/`is_authentication`/`is_build` + `from_status` | ✅ |
| `logs.ts` `Logger` | `logs::Logger`, `logs::NoopLogger` | ✅ |
| `utils.ts` `sha256`/`timeoutToSeconds`/`shellQuote` | `utils::{sha256_base64, timeout_to_seconds, shell_quote}` (internal) | ✅ |
| `utils.ts` `stripAnsi`/`toBlob`/`toUploadBody` | _(deferred to consuming milestones)_ | ⬜ |
| `connectionConfig.ts` `ConnectionConfig` | `connection_config::ConnectionConfig` | ✅ |
| `paginator.ts` `Paginator` | `paginator::PaginationState` (+ concrete paginators in Plan 3) | 🔶 |
| `sandbox/signature.ts` `getSignature` | `sandbox::signature::{get_signature, get_signature_now}` | ✅ |

## Transports (Plan 2) · Sandbox & I/O (Plan 3) · Git & Volume (Plan 4) · Template (Plan 5)

_Rows added as each milestone lands._
```

- [ ] **Step 5: Run the full gate**

Run: `cargo fmt --all --check`
Expected: clean.

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean (confirms no `unwrap`/`expect`/`missing_docs` in lib code).

Run: `cargo test --all-features`
Expected: all unit tests pass (errors 5, utils 4, logs 3, connection_config 9, signature 3, paginator 4).

Run: `cargo test --doc --all-features`
Expected: doctests pass.

Run: `cargo doc --no-deps --all-features`
Expected: builds with no warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/e2b-rs/src/lib.rs README.md docs/parity-checklist.md
git commit -m "docs: add quickstart doctest, README, and parity checklist" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 1 is complete when:
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo test --doc`, and `cargo doc --no-deps` all pass.
- The public foundation API (`Error`/`Result`, `Logger`/`NoopLogger`, `ConnectionConfig`/`ConnectionConfigOpts` + constants, `PaginationState`, `get_signature`/`get_signature_now`/`Signature`/`SignatureOperation`) is exported from `e2b_rs` and documented.
- `docs/parity-checklist.md` reflects foundation coverage.

**Next:** Plan 2 (Codegen & Transports) adds `tokio`/`reqwest`/`serde`/`prost`/`pbjson` and `progenitor`/`typify` (via a new `crates/xtask`), the `Error::Transport(#[from] reqwest::Error)` variant, the `validate_api_key` check, the API/envd-REST clients, and the hand-rolled Connect-over-JSON client.
```
