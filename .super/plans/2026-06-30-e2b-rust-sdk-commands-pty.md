# Sandbox Commands & Pty (Plan 3c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `sandbox.commands()` (run/list/kill/send_stdin/close_stdin/connect, foreground + background, streaming stdout/stderr via `tokio::sync::mpsc`) and `sandbox.pty()` (create/send_input/resize/kill/connect) — matching the E2B JS SDK 1:1.

**Architecture:** A `Commands` and a `Pty`, each built once when a `Sandbox` is created/connected (like `Filesystem`) and exposed via `Sandbox::commands()` / `Sandbox::pty()`. They run over the envd Connect `Process` service: `Start`/`Connect` are server-streams (process output), `List`/`SendInput`/`SendSignal`/`Update`/`CloseStdin` are unary. `run` opens the `Start` server-stream, peeks the first `StartEvent` for the pid, then a background `tokio::spawn` forwards `Stdout`/`Stderr`/`Pty` chunks into an `mpsc` channel (and accumulates them) and captures the final `EndEvent` into a `oneshot`. The public `CommandHandle` exposes `next()` (live output), `wait()` (the `CommandResult`), `kill()`/`send_stdin()`/`close_stdin()`. Generated `envd::proto::process` types stay `pub(crate)` and are wrapped in hand-written public types.

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), tokio (`sync::mpsc`, `sync::oneshot`, `rt`), futures, the existing `ConnectClient` (Connect-over-JSON), serde/serde_json, prost/pbjson (generated proto); wiremock for tests.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; all crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` allowed ONLY in `#[cfg(test)]`. Prefer `try_from(...).unwrap_or(...)` over `as` casts. Use `[crate::Type]` for cross-module intra-doc links.
- **Streaming is delivered via `tokio::sync::mpsc` channels, never callbacks.**
- **Non-zero exit is NOT an error:** `run`/`CommandHandle::wait` return `Ok(CommandResult { exit_code, ... })` regardless of exit code (caller inspects `exit_code`). `Err` is reserved for RPC/transport/timeout failures. (Documented divergence from JS, which throws `CommandExitError`.)
- **Do NOT expose generated `envd::proto::*` types in any `pub` signature/return/re-export** (spec §1 non-goal). Wrap them in hand-written public types.
- **Honest test fixtures:** mock the REAL Connect server-stream wire — `application/connect+json`, 5-byte envelopes (`encode_envelope`/`FLAG_END_STREAM`), and **pbjson proto3-JSON** for the messages: the `ProcessEvent` oneof serializes with the VARIANT FIELD NAME as the key (`{"start":…}`, `{"data":…}`, `{"end":…}`, `{"keepalive":…}`), `DataEvent` as `{"stdout":"<base64>"}` / `{"stderr":…}` / `{"pty":…}` (bytes are base64), `EndEvent` as `{"exitCode":N,"exited":bool,"status":"…"}` (camelCase). **NOT** the TS connect-web `{"case":…,"value":…}` form. (A prior milestone shipped a bug from a fixture that didn't match the real wire.)
- Every task: run `cargo fmt --all` before commit. Commit trailer (exact): `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference (source of truth): `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/sandbox/commands/` + `.../pty/`; proto in `crates/e2b-rs/src/envd/proto/process.rs`.

### Pre-verified facts (confirmed against the codebase at `main` = 280478a)
- Generated `envd::proto::process` (`pub(crate)`, prost+pbjson):
  - `ProcessConfig { cmd: String, args: Vec<String>, envs: HashMap<String,String>, cwd: Option<String> }` (NO user field — user travels via the auth header).
  - `StartRequest { process: Option<ProcessConfig>, pty: Option<Pty>, tag: Option<String>, stdin: Option<bool> }` → `StartResponse { event: Option<ProcessEvent> }`.
  - `ConnectRequest { process: Option<ProcessSelector> }` → `ConnectResponse { event: Option<ProcessEvent> }`.
  - `ProcessEvent { event: Option<process_event::Event> }` where `process_event::Event = Start(StartEvent{pid: u32}) | Data(DataEvent{output: Option<data_event::Output>}) | End(EndEvent{exit_code: i32, exited: bool, status: String, error: Option<String>}) | Keepalive(KeepAlive{})`. `data_event::Output = Stdout(Vec<u8>) | Stderr(Vec<u8>) | Pty(Vec<u8>)`.
  - `ListRequest {}` → `ListResponse { processes: Vec<ProcessInfo> }`; `ProcessInfo { config: Option<ProcessConfig>, pid: u32, tag: Option<String> }`.
  - `SendInputRequest { process: Option<ProcessSelector>, input: Option<ProcessInput> }` → `SendInputResponse {}`; `ProcessInput { input: Option<process_input::Input> }`, `process_input::Input = Stdin(Vec<u8>) | Pty(Vec<u8>)`.
  - `SendSignalRequest { process: Option<ProcessSelector>, signal: i32 }` → `SendSignalResponse {}`; `enum Signal { Unspecified=0, Sigterm=15, Sigkill=9 }`.
  - `CloseStdinRequest { process: Option<ProcessSelector> }` → `CloseStdinResponse {}`.
  - `UpdateRequest { process: Option<ProcessSelector>, pty: Option<Pty> }` → `UpdateResponse {}`.
  - `ProcessSelector { selector: Option<process_selector::Selector> }`, `process_selector::Selector = Pid(u32) | Tag(String)`.
  - `Pty { size: Option<pty::Size> }`, `pty::Size { cols: u32, rows: u32 }`.
- RPC path consts (`crates/e2b-rs/src/connect/mod.rs`, `pub(crate)`): `PROC_LIST`, `PROC_UPDATE`, `PROC_SEND_INPUT`, `PROC_SEND_SIGNAL`, `PROC_CLOSE_STDIN`, `PROC_START`, `PROC_CONNECT`.
- `ConnectClient` (`pub(crate)`): `unary<Req: Serialize, Resp: DeserializeOwned>(path, &req, user: Option<&str>) -> Result<Resp>`; `server_stream<Req: Serialize, Resp: DeserializeOwned + 'static>(path, &req, user: Option<&str>) -> Result<impl futures::Stream<Item = Result<Resp>> + Send>`. The stream is owned + `Send + 'static` (usable in `tokio::spawn` after `Box::pin`). `ConnectClientOpts` + construction are exactly as used by `Filesystem::build_with_base_url`.
- Connect error mapping: `Code::NotFound => Error::NotFound`. `kill` maps `NotFound → Ok(false)`.
- Version gates (`envd::versions`, `pub(crate)`, CONFIRMED): `version_gte`, `ENVD_COMMANDS_STDIN = "0.3.0"`, `ENVD_ENVD_CLOSE = "0.5.2"`, `ENVD_DEFAULT_USER = "0.4.0"` (JS `envd/versions.ts` matches). Always reference the CONST, never hardcode the value.
- JS `run(cmd, opts)` builds `ProcessConfig { cmd: "/bin/bash", args: ["-l", "-c", cmd], envs: opts.envs, cwd: opts.cwd }`, `StartRequest { process, pty: None, tag: None, stdin: opts.stdin.unwrap_or(false) }`, user via `server_stream`'s `user`. The stdin gate: if `stdin == Some(false)` AND envd < `ENVD_COMMANDS_STDIN` → error (you can't disable stdin on old envd).
- The `Filesystem` pattern to MIRROR (`crates/e2b-rs/src/sandbox/filesystem/mod.rs`): `build`/`build_with_base_url` construct a `ConnectClient` from `(sandbox_id, sandbox_domain, envd_version, envd_access_token, &ConnectionConfig)` using `config.get_sandbox_url(.., ENVD_PORT)`, `build_user_agent(config.integration.as_deref())`, `config.{request_timeout_ms, logger.clone(), proxy.clone()}`. `resolve_user` returns the explicit user or the legacy default (`DEFAULT_USERNAME` when envd < 0.4.0). `Sandbox::from_api_sandbox` (now `Result`) builds the per-sandbox clients and stores them; `Sandbox::files()` exposes `&Filesystem`. Plan 3c adds `commands: Commands` + `pty: Pty` fields the same way.
- `Sandbox` (`pub(crate)` fields): `sandbox_id`, `sandbox_domain: Option<String>`, `envd_version: String`, `envd_access_token: Option<String>`, `config: ConnectionConfig`. `from_api_sandbox(s, config, api) -> Result<Sandbox>` resolves `domain = s.domain ?? config.domain`.

---

## File Structure

- `crates/e2b-rs/src/sandbox/commands/mod.rs` — CREATE: `Commands` struct + construction (`build`/`build_with_base_url`) + `run`/`list`/`kill`/`send_stdin`/`close_stdin`/`connect` + a shared `pub(crate)` connect-client builder reused by `Pty`.
- `crates/e2b-rs/src/sandbox/commands/types.rs` — CREATE: public `CommandOutput`, `CommandResult`, `ProcessInfo`.
- `crates/e2b-rs/src/sandbox/commands/handle.rs` — CREATE: `CommandHandle` + the `pub(crate)` stream-driving helpers (peek-pid + spawn-forwarder + the unary by-pid ops).
- `crates/e2b-rs/src/sandbox/pty/mod.rs` — CREATE: `Pty` struct + `PtySize`/`PtyCreateOpts` + `create`/`send_input`/`resize`/`kill`/`connect`.
- `crates/e2b-rs/src/sandbox/sandbox.rs` — MODIFY: add `commands: Commands` + `pty: Pty` fields; `Sandbox::commands()` / `Sandbox::pty()`; build both in `from_api_sandbox`.
- `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs` — MODIFY: re-export the public command/pty types.
- `docs/parity-checklist.md`, `README.md` — MODIFY (Task 6).

---

### Task 1: Public command types + `CommandHandle` + the stream-driving infra

The headline + riskiest task — the streaming decoder and the handle. No `Commands`/`run` yet (Task 2); this builds the reusable pieces against a `pub(crate)` helper that takes an already-opened stream.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/commands/types.rs`, `crates/e2b-rs/src/sandbox/commands/handle.rs`, `crates/e2b-rs/src/sandbox/commands/mod.rs` (module wiring + re-exports; the `Commands` struct arrives in Task 2)
- Modify: `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `ConnectClient` (in `Arc`), proto `process` types, `PROC_SEND_SIGNAL`/`PROC_SEND_INPUT`/`PROC_CLOSE_STDIN`, `Signal`, `version_gte` + `ENVD_ENVD_CLOSE`, `tokio::sync::{mpsc, oneshot}`, `tokio::task::JoinHandle`, `futures::StreamExt`.
- Produces:
  - `pub enum CommandOutput { Stdout(Vec<u8>), Stderr(Vec<u8>), Pty(Vec<u8>) }` (types.rs).
  - `pub struct CommandResult { pub exit_code: i32, pub error: Option<String>, pub stdout: String, pub stderr: String }` (types.rs).
  - `pub struct ProcessInfo { pub pid: u32, pub tag: Option<String>, pub cmd: String, pub args: Vec<String>, pub envs: std::collections::BTreeMap<String,String>, pub cwd: Option<String> }` + `pub(crate) fn from_proto(pb::ProcessInfo) -> ProcessInfo` (types.rs).
  - `pub struct CommandHandle { pid, output: mpsc::Receiver<CommandOutput>, result: oneshot::Receiver<CommandResult>, task: JoinHandle<()>, connect: Arc<ConnectClient>, envd_version: String, user: Option<String> }` with `pub fn pid(&self) -> u32`, `pub async fn next(&mut self) -> Option<CommandOutput>`, `pub async fn wait(self) -> Result<CommandResult>`, `pub async fn kill(&self) -> Result<bool>`, `pub async fn send_stdin(&self, data: &[u8]) -> Result<()>`, `pub async fn close_stdin(&self) -> Result<()>`, `impl Drop` (abort) (handle.rs).
  - `pub(crate) async fn open_handle(connect: Arc<ConnectClient>, path: &str, req: &impl Serialize, user: Option<&str>, envd_version: &str) -> Result<CommandHandle>` — opens the server-stream (`Start` or `Connect`), `Box::pin`s it, peeks the first `StartEvent` for the pid (skipping leading `KeepAlive`), spawns the forwarding task, returns the handle. Shared by `run` (Task 2) + `connect` (Task 4) + pty (Task 5).
  - `pub(crate) fn pid_selector(pid: u32) -> pb::ProcessSelector`.

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/sandbox/commands/handle.rs` with a test module that mocks a `Start`-style stream and drives a handle through `open_handle`. Use `crate::connect::envelope::{encode_envelope, FLAG_END_STREAM}` (as the `filesystem/watch.rs` tests do). The stream: a `StartEvent` (pid 42), a `DataEvent` stdout `"hi"` (base64 `aGk=`), an `EndEvent` exit 0, then end-stream.
```rust
//! The `CommandHandle` and the stream-driving infrastructure for Commands/Pty.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::client::{ConnectClient, ConnectClientOpts};
    use crate::connect::envelope::{FLAG_END_STREAM, encode_envelope};
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn connect_for(server: &MockServer) -> Arc<ConnectClient> {
        Arc::new(
            ConnectClient::new(ConnectClientOpts {
                base_url: server.uri(),
                access_token: None,
                sandbox_id: "sbx_c".to_string(),
                envd_port: 49983,
                user_agent: "e2b-rs-test".to_string(),
                envd_version: "0.6.3".to_string(),
                request_timeout_ms: 60_000,
                logger: None,
                proxy: None,
            })
            .expect("connect client"),
        )
    }

    #[tokio::test]
    async fn handle_streams_stdout_then_exit() {
        let server = MockServer::start().await;
        // pbjson proto3-JSON: ProcessEvent oneof = field-name key; DataEvent stdout = base64.
        let mut body = encode_envelope(0, br#"{"event":{"start":{"pid":42}}}"#);
        body.extend(encode_envelope(0, br#"{"event":{"data":{"stdout":"aGk="}}}"#));
        body.extend(encode_envelope(
            0,
            br#"{"event":{"end":{"exitCode":0,"exited":true,"status":"exited"}}}"#,
        ));
        body.extend(encode_envelope(FLAG_END_STREAM, b"{}"));
        Mock::given(method("POST"))
            .and(path("/process.Process/Start"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/connect+json")
                    .set_body_bytes(body),
            )
            .mount(&server)
            .await;

        let connect = connect_for(&server);
        let req = crate::envd::proto::process::StartRequest {
            process: Some(crate::envd::proto::process::ProcessConfig {
                cmd: "/bin/bash".to_string(),
                args: vec!["-l".to_string(), "-c".to_string(), "echo hi".to_string()],
                envs: Default::default(),
                cwd: None,
            }),
            pty: None,
            tag: None,
            stdin: Some(false),
        };
        let mut handle = open_handle(connect, crate::connect::PROC_START, &req, None, "0.6.3")
            .await
            .expect("handle");
        assert_eq!(handle.pid(), 42);
        match handle.next().await {
            Some(CommandOutput::Stdout(b)) => assert_eq!(b, b"hi"),
            other => panic!("expected stdout, got {other:?}"),
        }
        assert!(handle.next().await.is_none()); // stream ended
        let result = handle.wait().await.expect("result");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hi");
    }
}
```
NOTE for the test: derive `Debug` on `CommandOutput` so `panic!("{other:?}")` compiles. Add `pub(crate) mod commands;` to `crates/e2b-rs/src/sandbox/mod.rs`; in `commands/mod.rs` add `pub(crate) mod handle; pub mod types;` + `pub use handle::CommandHandle; pub use types::{CommandOutput, CommandResult, ProcessInfo};` + `pub(crate) use handle::{open_handle, pid_selector};`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::commands::handle`
Expected: FAIL — `open_handle`/`CommandHandle` not defined.

- [ ] **Step 3: Implement the public types**

In `crates/e2b-rs/src/sandbox/commands/types.rs`:
```rust
//! Public value types for sandbox commands.

use std::collections::BTreeMap;

use crate::envd::proto::process as pb;

/// A chunk of live output from a running command.
#[derive(Debug, Clone)]
pub enum CommandOutput {
    /// Bytes written to stdout.
    Stdout(Vec<u8>),
    /// Bytes written to stderr.
    Stderr(Vec<u8>),
    /// Raw PTY output bytes (for `pty` sessions).
    Pty(Vec<u8>),
}

/// The result of a finished command.
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Process exit code (a non-zero code is NOT an SDK error — inspect it here).
    pub exit_code: i32,
    /// Error description if the process failed to run/exit cleanly.
    pub error: Option<String>,
    /// Full accumulated stdout (lossy UTF-8).
    pub stdout: String,
    /// Full accumulated stderr (lossy UTF-8).
    pub stderr: String,
}

/// Info about a running process (returned by `Commands::list`).
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process id.
    pub pid: u32,
    /// Optional process tag.
    pub tag: Option<String>,
    /// Command binary.
    pub cmd: String,
    /// Command arguments.
    pub args: Vec<String>,
    /// Environment variables.
    pub envs: BTreeMap<String, String>,
    /// Working directory, if set.
    pub cwd: Option<String>,
}

impl ProcessInfo {
    /// Map the generated proto type to the public one.
    pub(crate) fn from_proto(p: pb::ProcessInfo) -> ProcessInfo {
        let config = p.config.unwrap_or_default();
        ProcessInfo {
            pid: p.pid,
            tag: p.tag,
            cmd: config.cmd,
            args: config.args,
            envs: config.envs.into_iter().collect(),
            cwd: config.cwd,
        }
    }
}
```

- [ ] **Step 4: Implement `CommandHandle` + `open_handle` + helpers**

In `crates/e2b-rs/src/sandbox/commands/handle.rs` (above the test module):
```rust
use std::sync::Arc;

use futures::StreamExt as _;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::types::{CommandOutput, CommandResult};
use crate::connect::client::ConnectClient;
use crate::envd::proto::process as pb;
use crate::envd::proto::process::process_event::Event as ProcEvent;
// NOTE: `DataEvent` (and its `Output` oneof) are nested UNDER `process_event`
// by prost, so the path is `process_event::data_event::Output` (NOT top-level).
use crate::envd::proto::process::process_event::data_event::Output as DataOutput;
use crate::envd::versions::{ENVD_ENVD_CLOSE, version_gte};
use crate::errors::{Error, Result};

/// Build a `ProcessSelector` selecting a process by pid.
pub(crate) fn pid_selector(pid: u32) -> pb::ProcessSelector {
    pb::ProcessSelector {
        selector: Some(pb::process_selector::Selector::Pid(pid)),
    }
}

/// A running command (or pty). Receive live output with [`CommandHandle::next`]
/// and the final result with [`CommandHandle::wait`].
pub struct CommandHandle {
    pid: u32,
    output: mpsc::Receiver<CommandOutput>,
    result: oneshot::Receiver<CommandResult>,
    task: JoinHandle<()>,
    connect: Arc<ConnectClient>,
    envd_version: String,
    user: Option<String>,
}

impl CommandHandle {
    /// The process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Receive the next chunk of live output, or `None` when the process has
    /// produced all output (it may still be finishing — call [`Self::wait`]).
    pub async fn next(&mut self) -> Option<CommandOutput> {
        self.output.recv().await
    }

    /// Wait for the command to finish and return its [`CommandResult`]. Drains
    /// any unread output first (so the background task can complete). A non-zero
    /// `exit_code` is returned in the result, NOT as an error.
    pub async fn wait(mut self) -> Result<CommandResult> {
        while self.output.recv().await.is_some() {}
        self.result
            .await
            .map_err(|_| Error::Internal("command ended without a result".to_string()))
    }

    /// Kill the process (SIGKILL). Returns `false` if it was not found.
    pub async fn kill(&self) -> Result<bool> {
        let req = pb::SendSignalRequest {
            process: Some(pid_selector(self.pid)),
            signal: pb::Signal::Sigkill as i32,
        };
        match self
            .connect
            .unary::<_, pb::SendSignalResponse>(
                crate::connect::PROC_SEND_SIGNAL,
                &req,
                self.user.as_deref(),
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Write bytes to the process's stdin.
    pub async fn send_stdin(&self, data: &[u8]) -> Result<()> {
        let req = pb::SendInputRequest {
            process: Some(pid_selector(self.pid)),
            input: Some(pb::ProcessInput {
                input: Some(pb::process_input::Input::Stdin(data.to_vec())),
            }),
        };
        self.connect
            .unary::<_, pb::SendInputResponse>(
                crate::connect::PROC_SEND_INPUT,
                &req,
                self.user.as_deref(),
            )
            .await
            .map(|_| ())
    }

    /// Close the process's stdin (requires a newer envd).
    pub async fn close_stdin(&self) -> Result<()> {
        if !version_gte(&self.envd_version, ENVD_ENVD_CLOSE) {
            return Err(Error::Template(format!(
                "close_stdin requires a newer template (envd >= {ENVD_ENVD_CLOSE})"
            )));
        }
        let req = pb::CloseStdinRequest {
            process: Some(pid_selector(self.pid)),
        };
        self.connect
            .unary::<_, pb::CloseStdinResponse>(
                crate::connect::PROC_CLOSE_STDIN,
                &req,
                self.user.as_deref(),
            )
            .await
            .map(|_| ())
    }
}

impl Drop for CommandHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Extract the `Event` from a `StartResponse`/`ConnectResponse`-shaped message
/// (both wrap `ProcessEvent`).
fn response_event(resp: pb::StartResponse) -> Option<ProcEvent> {
    resp.event.and_then(|e| e.event)
}

/// Open a process server-stream (`Start`/`Connect`), peek the first `StartEvent`
/// for the pid, then spawn a task forwarding output into the handle's channel.
pub(crate) async fn open_handle(
    connect: Arc<ConnectClient>,
    path: &str,
    req: &impl Serialize,
    user: Option<&str>,
    envd_version: &str,
) -> Result<CommandHandle> {
    // `Box::pin` so the stream is `Unpin` and can move into the spawned task.
    let mut stream = Box::pin(
        connect
            .server_stream::<_, pb::StartResponse>(path, req, user)
            .await?,
    );

    // Wait for the StartEvent (skip any leading KeepAlive).
    let pid = loop {
        match stream.next().await {
            Some(Ok(resp)) => match response_event(resp) {
                Some(ProcEvent::Start(s)) => break s.pid,
                Some(ProcEvent::Keepalive(_)) | None => continue,
                Some(_) => {
                    return Err(Error::Internal(
                        "process stream sent output before the start event".to_string(),
                    ));
                }
            },
            Some(Err(e)) => return Err(e),
            None => {
                return Err(Error::Internal(
                    "process stream ended before the start event".to_string(),
                ));
            }
        }
    };

    let (output_tx, output_rx) = mpsc::channel(128);
    let (result_tx, result_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut exit_code: i32 = 0;
        let mut error: Option<String> = None;
        while let Some(item) = stream.next().await {
            let Ok(resp) = item else { break }; // stream error ends the command
            match response_event(resp) {
                Some(ProcEvent::Data(d)) => match d.output {
                    Some(DataOutput::Stdout(b)) => {
                        stdout.extend_from_slice(&b);
                        if output_tx.send(CommandOutput::Stdout(b)).await.is_err() {
                            // receiver dropped; keep accumulating for wait()
                        }
                    }
                    Some(DataOutput::Stderr(b)) => {
                        stderr.extend_from_slice(&b);
                        let _ = output_tx.send(CommandOutput::Stderr(b)).await;
                    }
                    Some(DataOutput::Pty(b)) => {
                        let _ = output_tx.send(CommandOutput::Pty(b)).await;
                    }
                    None => {}
                },
                Some(ProcEvent::End(end)) => {
                    exit_code = end.exit_code;
                    error = end.error;
                    break;
                }
                // Start (already consumed) + KeepAlive are not surfaced.
                _ => {}
            }
        }
        let _ = result_tx.send(CommandResult {
            exit_code,
            error,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        });
    });

    Ok(CommandHandle {
        pid,
        output: output_rx,
        result: result_rx,
        task,
        connect,
        envd_version: envd_version.to_string(),
        user: user.map(str::to_string),
    })
}
```
NOTE: if `output_tx.send` returning `Err` (receiver dropped) makes the loop spin uselessly for a chatty process whose consumer dropped the handle, that's acceptable — `Drop` aborts the task when the handle is dropped. The `_ =` discards confirm the design (accumulate for `wait()` even if nobody is reading live output). Confirm `ConnectClient::server_stream`'s returned stream is `Send + 'static` (it is — used in `filesystem/watch.rs` `tokio::spawn`); `Box::pin` makes it `Unpin` for the peek-then-move.

- [ ] **Step 5: Run the test + re-export**

Run: `cargo test -p e2b-rs sandbox::commands::handle` → PASS.
Re-export the public types: `sandbox/mod.rs` `pub use commands::{CommandHandle, CommandOutput, CommandResult, ProcessInfo};`; add the same names to `lib.rs`'s `pub use sandbox::{...}`.

- [ ] **Step 6: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::commands` and `cargo clippy --workspace --all-targets -- -D warnings` → clean. `cargo doc --no-deps -p e2b-rs` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/commands crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(commands): add CommandHandle + process-stream infrastructure" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `Commands` struct, construction, and `run` (foreground + background)

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/commands/mod.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`

**Interfaces:**
- Consumes: `open_handle`, `CommandHandle`, `ConnectClient`/`ConnectClientOpts`, `ConnectionConfig`, `build_user_agent`, `ENVD_PORT`/`DEFAULT_USERNAME`, `version_gte` + `ENVD_COMMANDS_STDIN`, proto `StartRequest`/`ProcessConfig`.
- Produces:
  - `pub struct Commands { connect: Arc<ConnectClient>, envd_version: String, default_user: Option<String> }` (`pub(crate)` fields).
  - `pub(crate) fn Commands::build(sandbox_id, sandbox_domain, envd_version, envd_access_token, &ConnectionConfig) -> Result<Commands>` (+ a test-friendly `build_with_connect(Arc<ConnectClient>, envd_version) -> Commands` OR a `build_with_base_url`). The connect-client construction is shared with `Pty` via a `pub(crate) fn build_connect_client(...) -> Result<ConnectClient>` in `commands/mod.rs`.
  - `pub(crate) fn Commands::resolve_user(&self, Option<&str>) -> Option<String>`.
  - `pub struct CommandStartOpts { pub cwd: Option<String>, pub user: Option<String>, pub envs: BTreeMap<String,String>, pub stdin: Option<bool> }` (`#[derive(Default)]`).
  - `pub async fn Commands::run(&self, cmd: &str, opts: CommandStartOpts) -> Result<CommandResult>` (foreground: `start` then `wait`).
  - `pub fn Commands::start(&self, cmd: &str, opts: CommandStartOpts) -> impl Future<Output = Result<CommandHandle>>` OR `pub async fn Commands::start(...) -> Result<CommandHandle>` (background).
  - `Sandbox`: `pub(crate) commands: Commands` field; `pub fn commands(&self) -> &Commands`; built in `from_api_sandbox`.

- [ ] **Step 1: Write the failing tests**

In `crates/e2b-rs/src/sandbox/commands/mod.rs` `mod tests` (build a `Commands` pointed at a mock server — add a `pub(crate) fn build_with_connect(Arc<ConnectClient>, envd_version: &str) -> Commands` for tests):
```rust
    #[tokio::test]
    async fn run_foreground_returns_result() {
        let server = MockServer::start().await;
        let mut body = encode_envelope(0, br#"{"event":{"start":{"pid":7}}}"#);
        body.extend(encode_envelope(0, br#"{"event":{"data":{"stdout":"b3V0"}}}"#)); // "out"
        body.extend(encode_envelope(
            0,
            br#"{"event":{"end":{"exitCode":3,"exited":true,"status":"exited"}}}"#,
        ));
        body.extend(encode_envelope(FLAG_END_STREAM, b"{}"));
        Mock::given(method("POST"))
            .and(path("/process.Process/Start"))
            .and(body_partial_json(serde_json::json!({
                "process": { "cmd": "/bin/bash", "args": ["-l", "-c", "echo out"] }
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/connect+json")
                    .set_body_bytes(body),
            )
            .mount(&server)
            .await;
        let commands = build_with_connect(connect_for(&server), "0.6.3");
        let result = commands
            .run("echo out", CommandStartOpts::default())
            .await
            .expect("run");
        // Non-zero exit is data, not an error.
        assert_eq!(result.exit_code, 3);
        assert_eq!(result.stdout, "out");
    }
```
(`connect_for` + `encode_envelope`/`FLAG_END_STREAM` + `body_partial_json` come from the same imports as Task 1's test; share a small test helper.)

- [ ] **Step 2: Run to verify failure** — `cargo test -p e2b-rs sandbox::commands::tests::run_foreground` → FAIL.

- [ ] **Step 3: Implement `Commands` + construction + `run`/`start`**

In `crates/e2b-rs/src/sandbox/commands/mod.rs` (module wiring from Task 1 plus):
```rust
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::connect::client::{ConnectClient, ConnectClientOpts};
use crate::connection_config::{ConnectionConfig, DEFAULT_USERNAME, ENVD_PORT};
use crate::envd::proto::process as pb;
use crate::envd::versions::{ENVD_COMMANDS_STDIN, ENVD_DEFAULT_USER, version_gte};
use crate::errors::{Error, Result};

/// Build a `ConnectClient` for a sandbox's envd RPC surface (shared by
/// [`Commands`] and the pty module).
pub(crate) fn build_connect_client(
    sandbox_id: &str,
    sandbox_domain: &str,
    envd_version: &str,
    envd_access_token: Option<&str>,
    config: &ConnectionConfig,
) -> Result<ConnectClient> {
    let base_url = config.get_sandbox_url(sandbox_id, sandbox_domain, ENVD_PORT);
    ConnectClient::new(ConnectClientOpts {
        base_url,
        access_token: envd_access_token.map(str::to_string),
        sandbox_id: sandbox_id.to_string(),
        envd_port: ENVD_PORT,
        user_agent: crate::utils::build_user_agent(config.integration.as_deref()),
        envd_version: envd_version.to_string(),
        request_timeout_ms: config.request_timeout_ms,
        logger: config.logger.clone(),
        proxy: config.proxy.clone(),
    })
}

/// Options for [`Commands::run`] / [`Commands::start`].
#[derive(Default)]
pub struct CommandStartOpts {
    /// Working directory.
    pub cwd: Option<String>,
    /// User to run as.
    pub user: Option<String>,
    /// Environment variables.
    pub envs: BTreeMap<String, String>,
    /// Whether stdin is enabled (default on; `Some(false)` requires newer envd).
    pub stdin: Option<bool>,
}

/// Run and manage commands in the sandbox (`sandbox.commands()`).
pub struct Commands {
    pub(crate) connect: Arc<ConnectClient>,
    pub(crate) envd_version: String,
    pub(crate) default_user: Option<String>,
}

impl Commands {
    pub(crate) fn build(
        sandbox_id: &str,
        sandbox_domain: &str,
        envd_version: &str,
        envd_access_token: Option<&str>,
        config: &ConnectionConfig,
    ) -> Result<Commands> {
        let connect = build_connect_client(
            sandbox_id,
            sandbox_domain,
            envd_version,
            envd_access_token,
            config,
        )?;
        Ok(Self::build_with_connect(Arc::new(connect), envd_version))
    }

    pub(crate) fn build_with_connect(connect: Arc<ConnectClient>, envd_version: &str) -> Commands {
        let default_user = (!version_gte(envd_version, ENVD_DEFAULT_USER))
            .then(|| DEFAULT_USERNAME.to_string());
        Commands {
            connect,
            envd_version: envd_version.to_string(),
            default_user,
        }
    }

    pub(crate) fn resolve_user(&self, user: Option<&str>) -> Option<String> {
        match user {
            Some(u) => Some(u.to_string()),
            None => self.default_user.clone(),
        }
    }

    fn start_request(&self, cmd: &str, opts: &CommandStartOpts) -> Result<pb::StartRequest> {
        if opts.stdin == Some(false) && !version_gte(&self.envd_version, ENVD_COMMANDS_STDIN) {
            return Err(Error::Sandbox(format!(
                "this template's envd cannot disable stdin (requires >= {ENVD_COMMANDS_STDIN}); rebuild the template"
            )));
        }
        Ok(pb::StartRequest {
            process: Some(pb::ProcessConfig {
                cmd: "/bin/bash".to_string(),
                args: vec!["-l".to_string(), "-c".to_string(), cmd.to_string()],
                envs: opts.envs.clone().into_iter().collect(),
                cwd: opts.cwd.clone(),
            }),
            pty: None,
            tag: None,
            stdin: Some(opts.stdin.unwrap_or(false)),
        })
    }

    /// Start a command in the background, returning a [`CommandHandle`] for live
    /// output + control.
    pub async fn start(&self, cmd: &str, opts: CommandStartOpts) -> Result<CommandHandle> {
        let user = self.resolve_user(opts.user.as_deref());
        let req = self.start_request(cmd, &opts)?;
        open_handle(
            Arc::clone(&self.connect),
            crate::connect::PROC_START,
            &req,
            user.as_deref(),
            &self.envd_version,
        )
        .await
    }

    /// Run a command to completion and return its [`CommandResult`]. A non-zero
    /// `exit_code` is returned in the result, NOT as an error.
    pub async fn run(&self, cmd: &str, opts: CommandStartOpts) -> Result<CommandResult> {
        self.start(cmd, opts).await?.wait().await
    }
}
```

- [ ] **Step 4: Run the test** — `cargo test -p e2b-rs sandbox::commands::tests::run_foreground` → PASS.

- [ ] **Step 5: Wire `Commands` into `Sandbox`**

In `sandbox/sandbox.rs`: add `pub(crate) commands: crate::sandbox::commands::Commands` field (documented); in `from_api_sandbox` build it (`Commands::build(&s.sandbox_id, &domain, &s.envd_version.0, s.envd_access_token.as_deref(), &config)?`) and add to the struct literal (BOTH the real constructor AND the `local_sandbox` test helper in sandbox.rs); add `pub fn commands(&self) -> &crate::sandbox::commands::Commands { &self.commands }`.

- [ ] **Step 6: Re-export `CommandStartOpts` + `Commands`; verify & commit**

Re-export `Commands` + `CommandStartOpts` via `sandbox/mod.rs` → `lib.rs`. Run `cargo test -p e2b-rs sandbox::`, `cargo clippy ... -D warnings`, `cargo doc --no-deps -p e2b-rs` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/commands crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(commands): add Commands::run/start wired into Sandbox" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `Commands` unary ops — `list`, `kill`, `send_stdin`, `close_stdin`

**Files:** Modify `crates/e2b-rs/src/sandbox/commands/mod.rs`.

**Interfaces:**
- Produces (on `Commands`):
  - `pub async fn list(&self) -> Result<Vec<ProcessInfo>>` (List unary).
  - `pub async fn kill(&self, pid: u32) -> Result<bool>` (SendSignal SIGKILL; `NotFound → false`).
  - `pub async fn send_stdin(&self, pid: u32, data: &[u8]) -> Result<()>` (SendInput, stdin).
  - `pub async fn close_stdin(&self, pid: u32) -> Result<()>` (CloseStdin; gated by `ENVD_ENVD_CLOSE`).

- [ ] **Step 1: Write failing tests** (in `commands/mod.rs` `mod tests`):
```rust
    #[tokio::test]
    async fn list_maps_processes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/process.Process/List"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "processes": [
                    { "pid": 11, "config": { "cmd": "/bin/bash", "args": ["-l","-c","sleep 1"], "envs": {} } }
                ]
            })))
            .mount(&server)
            .await;
        let out = build_with_connect(connect_for(&server), "0.6.3").list().await.expect("list");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pid, 11);
        assert_eq!(out[0].cmd, "/bin/bash");
    }

    #[tokio::test]
    async fn kill_false_on_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/process.Process/SendSignal"))
            .respond_with(ResponseTemplate::new(404).set_body_json(
                serde_json::json!({ "code": "not_found", "message": "no such process" }),
            ))
            .mount(&server)
            .await;
        assert!(!build_with_connect(connect_for(&server), "0.6.3").kill(99).await.expect("kill"));
    }
```

- [ ] **Step 2: Run to verify failure** — FAIL (methods not found).

- [ ] **Step 3: Implement** (add to `impl Commands`):
```rust
    /// List running processes.
    pub async fn list(&self) -> Result<Vec<ProcessInfo>> {
        let user = self.default_user.clone();
        let resp: pb::ListResponse = self
            .connect
            .unary(crate::connect::PROC_LIST, &pb::ListRequest {}, user.as_deref())
            .await?;
        Ok(resp.processes.into_iter().map(ProcessInfo::from_proto).collect())
    }

    /// Kill a process by pid (SIGKILL). Returns `false` if it was not found.
    pub async fn kill(&self, pid: u32) -> Result<bool> {
        let user = self.default_user.clone();
        let req = pb::SendSignalRequest {
            process: Some(pid_selector(pid)),
            signal: pb::Signal::Sigkill as i32,
        };
        match self
            .connect
            .unary::<_, pb::SendSignalResponse>(crate::connect::PROC_SEND_SIGNAL, &req, user.as_deref())
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Write bytes to a process's stdin.
    pub async fn send_stdin(&self, pid: u32, data: &[u8]) -> Result<()> {
        let user = self.default_user.clone();
        let req = pb::SendInputRequest {
            process: Some(pid_selector(pid)),
            input: Some(pb::ProcessInput {
                input: Some(pb::process_input::Input::Stdin(data.to_vec())),
            }),
        };
        self.connect
            .unary::<_, pb::SendInputResponse>(crate::connect::PROC_SEND_INPUT, &req, user.as_deref())
            .await
            .map(|_| ())
    }

    /// Close a process's stdin (requires envd >= `ENVD_ENVD_CLOSE`).
    pub async fn close_stdin(&self, pid: u32) -> Result<()> {
        if !version_gte(&self.envd_version, crate::envd::versions::ENVD_ENVD_CLOSE) {
            return Err(Error::Template(format!(
                "close_stdin requires a newer template (envd >= {})",
                crate::envd::versions::ENVD_ENVD_CLOSE
            )));
        }
        let user = self.default_user.clone();
        let req = pb::CloseStdinRequest { process: Some(pid_selector(pid)) };
        self.connect
            .unary::<_, pb::CloseStdinResponse>(crate::connect::PROC_CLOSE_STDIN, &req, user.as_deref())
            .await
            .map(|_| ())
    }
```
(`ProcessInfo`/`pid_selector` are in scope via the module's `use`s; add `use super::types::ProcessInfo;` and the `handle::pid_selector` `use` if not already present.)

- [ ] **Step 4: Run tests** — `cargo test -p e2b-rs sandbox::commands` → pass.

- [ ] **Step 5: Verify & commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/commands/mod.rs
git commit -m "feat(commands): add list/kill/send_stdin/close_stdin" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `Commands::connect` (reconnect to a running process)

**Files:** Modify `crates/e2b-rs/src/sandbox/commands/mod.rs`.

**Interfaces:**
- Produces: `pub async fn Commands::connect(&self, pid: u32, user: Option<&str>) -> Result<CommandHandle>` — `Connect` server-stream (reuses `open_handle`).

- [ ] **Step 1: Write the failing test** — mock `/process.Process/Connect` with a StartEvent(pid)+Data+End stream (as Task 1), assert `handle.pid() == pid` and the stdout arrives.
```rust
    #[tokio::test]
    async fn connect_reattaches_to_process() {
        let server = MockServer::start().await;
        let mut body = encode_envelope(0, br#"{"event":{"start":{"pid":5}}}"#);
        body.extend(encode_envelope(0, br#"{"event":{"data":{"stdout":"aGk="}}}"#));
        body.extend(encode_envelope(0, br#"{"event":{"end":{"exitCode":0,"exited":true,"status":"exited"}}}"#));
        body.extend(encode_envelope(FLAG_END_STREAM, b"{}"));
        Mock::given(method("POST"))
            .and(path("/process.Process/Connect"))
            .and(body_partial_json(serde_json::json!({ "process": { "pid": 5 } })))
            .respond_with(ResponseTemplate::new(200)
                .insert_header("content-type", "application/connect+json")
                .set_body_bytes(body))
            .mount(&server)
            .await;
        let mut handle = build_with_connect(connect_for(&server), "0.6.3")
            .connect(5, None).await.expect("connect");
        assert_eq!(handle.pid(), 5);
        assert!(matches!(handle.next().await, Some(CommandOutput::Stdout(_))));
    }
```
NOTE: `ProcessSelector{pid}` serializes (pbjson oneof) as `{"pid":5}` — the `body_partial_json` matcher uses `{ "process": { "pid": 5 } }`. Confirm against the generated `ProcessSelector` serde (variant field-name key).

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement**:
```rust
    /// Reconnect to a running process by pid, returning a [`CommandHandle`].
    pub async fn connect(&self, pid: u32, user: Option<&str>) -> Result<CommandHandle> {
        let user = self.resolve_user(user);
        let req = pb::ConnectRequest { process: Some(pid_selector(pid)) };
        open_handle(
            Arc::clone(&self.connect),
            crate::connect::PROC_CONNECT,
            &req,
            user.as_deref(),
            &self.envd_version,
        )
        .await
    }
```
NOTE: `open_handle` decodes `pb::StartResponse`, but `Connect` returns `pb::ConnectResponse`. Both are `{ event: Option<ProcessEvent> }` with identical proto3-JSON shape, so deserializing `ConnectResponse`'s wire bytes as `StartResponse` works (same fields). If the implementer prefers strictness, generalize `open_handle`/`response_event` over a trait or accept `ConnectResponse` too — but the simplest correct approach is to reuse `StartResponse` (verify the wire shapes are identical: both wrap `ProcessEvent` under `event`).

- [ ] **Step 4: Run test + commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/commands/mod.rs
git commit -m "feat(commands): add connect (reattach to a running process)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `Pty` — create / send_input / resize / kill / connect

**Files:**
- Create: `crates/e2b-rs/src/sandbox/pty/mod.rs`
- Modify: `crates/e2b-rs/src/sandbox/sandbox.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `open_handle`, `pid_selector`, `CommandHandle`, `build_connect_client`, proto `StartRequest`/`Pty`/`UpdateRequest`/`SendInputRequest`/`SendSignalRequest`.
- Produces:
  - `pub struct PtySize { pub cols: u32, pub rows: u32 }`.
  - `pub struct PtyCreateOpts { pub user: Option<String>, pub envs: BTreeMap<String,String>, pub cwd: Option<String> }` (`#[derive(Default)]`).
  - `pub struct Pty { connect: Arc<ConnectClient>, envd_version: String, default_user: Option<String> }`.
  - `pub(crate) fn Pty::build(...) -> Result<Pty>` (mirror `Commands::build`, reuse `build_connect_client`).
  - `pub async fn Pty::create(&self, size: PtySize, opts: PtyCreateOpts) -> Result<CommandHandle>` (Start RPC with a pty config; bash `-i -l`, default env `TERM`/`LANG`/`LC_ALL`). Output arrives as `CommandOutput::Pty`.
  - `pub async fn Pty::send_input(&self, pid: u32, data: &[u8]) -> Result<()>` (SendInput, `Pty` variant).
  - `pub async fn Pty::resize(&self, pid: u32, size: PtySize) -> Result<()>` (Update RPC).
  - `pub async fn Pty::kill(&self, pid: u32) -> Result<bool>` (SendSignal; `NotFound → false`).
  - `pub async fn Pty::connect(&self, pid: u32, user: Option<&str>) -> Result<CommandHandle>` (Connect RPC).
  - `Sandbox`: `pub(crate) pty: Pty` field; `pub fn pty(&self) -> &Pty`; built in `from_api_sandbox`.

- [ ] **Step 1: Write failing tests** — (1) `create` mock at `/process.Process/Start` matching `body_partial_json({ "process": { "cmd": "/bin/bash" }, "pty": { "size": { "cols": 80, "rows": 24 } } })`, stream StartEvent(pid)+Data pty+End; assert `handle.pid()` + a `CommandOutput::Pty` arrives. (2) `resize` mock at `/process.Process/Update` matching `{ "process": { "pid": 9 }, "pty": { "size": { "cols": 100, "rows": 40 } } }` → 200 `{}`; assert Ok.

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement `Pty`** in `crates/e2b-rs/src/sandbox/pty/mod.rs`:
```rust
//! Pseudo-terminal sessions in the sandbox (`sandbox.pty()`).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::connect::client::ConnectClient;
use crate::connection_config::{ConnectionConfig, DEFAULT_USERNAME};
use crate::envd::proto::process as pb;
use crate::envd::versions::{ENVD_DEFAULT_USER, version_gte};
use crate::errors::{Error, Result};
use crate::sandbox::commands::{CommandHandle, build_connect_client, open_handle, pid_selector};

/// Terminal dimensions.
#[derive(Debug, Clone, Copy)]
pub struct PtySize {
    /// Columns.
    pub cols: u32,
    /// Rows.
    pub rows: u32,
}

/// Options for [`Pty::create`].
#[derive(Default)]
pub struct PtyCreateOpts {
    /// User to run the shell as.
    pub user: Option<String>,
    /// Extra environment variables.
    pub envs: BTreeMap<String, String>,
    /// Working directory.
    pub cwd: Option<String>,
}

/// Manage pseudo-terminal sessions (`sandbox.pty()`).
pub struct Pty {
    pub(crate) connect: Arc<ConnectClient>,
    pub(crate) envd_version: String,
    pub(crate) default_user: Option<String>,
}

impl Pty {
    pub(crate) fn build(
        sandbox_id: &str,
        sandbox_domain: &str,
        envd_version: &str,
        envd_access_token: Option<&str>,
        config: &ConnectionConfig,
    ) -> Result<Pty> {
        let connect = build_connect_client(
            sandbox_id,
            sandbox_domain,
            envd_version,
            envd_access_token,
            config,
        )?;
        let default_user = (!version_gte(envd_version, ENVD_DEFAULT_USER))
            .then(|| DEFAULT_USERNAME.to_string());
        Ok(Pty {
            connect: Arc::new(connect),
            envd_version: envd_version.to_string(),
            default_user,
        })
    }

    pub(crate) fn build_with_connect(connect: Arc<ConnectClient>, envd_version: &str) -> Pty {
        let default_user = (!version_gte(envd_version, ENVD_DEFAULT_USER))
            .then(|| DEFAULT_USERNAME.to_string());
        Pty {
            connect,
            envd_version: envd_version.to_string(),
            default_user,
        }
    }

    fn resolve_user(&self, user: Option<&str>) -> Option<String> {
        match user {
            Some(u) => Some(u.to_string()),
            None => self.default_user.clone(),
        }
    }

    /// Start an interactive pty session (`/bin/bash -i -l`). Output arrives as
    /// [`crate::CommandOutput::Pty`] on the returned handle.
    pub async fn create(&self, size: PtySize, opts: PtyCreateOpts) -> Result<CommandHandle> {
        let user = self.resolve_user(opts.user.as_deref());
        let mut envs: BTreeMap<String, String> = opts.envs.clone();
        envs.entry("TERM".to_string()).or_insert_with(|| "xterm-256color".to_string());
        envs.entry("LANG".to_string()).or_insert_with(|| "C.UTF-8".to_string());
        envs.entry("LC_ALL".to_string()).or_insert_with(|| "C.UTF-8".to_string());
        let req = pb::StartRequest {
            process: Some(pb::ProcessConfig {
                cmd: "/bin/bash".to_string(),
                args: vec!["-i".to_string(), "-l".to_string()],
                envs: envs.into_iter().collect(),
                cwd: opts.cwd.clone(),
            }),
            pty: Some(pb::Pty {
                size: Some(pb::pty::Size { cols: size.cols, rows: size.rows }),
            }),
            tag: None,
            stdin: Some(true),
        };
        open_handle(
            Arc::clone(&self.connect),
            crate::connect::PROC_START,
            &req,
            user.as_deref(),
            &self.envd_version,
        )
        .await
    }

    /// Send raw bytes to the pty.
    pub async fn send_input(&self, pid: u32, data: &[u8]) -> Result<()> {
        let user = self.default_user.clone();
        let req = pb::SendInputRequest {
            process: Some(pid_selector(pid)),
            input: Some(pb::ProcessInput {
                input: Some(pb::process_input::Input::Pty(data.to_vec())),
            }),
        };
        self.connect
            .unary::<_, pb::SendInputResponse>(crate::connect::PROC_SEND_INPUT, &req, user.as_deref())
            .await
            .map(|_| ())
    }

    /// Resize the pty.
    pub async fn resize(&self, pid: u32, size: PtySize) -> Result<()> {
        let user = self.default_user.clone();
        let req = pb::UpdateRequest {
            process: Some(pid_selector(pid)),
            pty: Some(pb::Pty {
                size: Some(pb::pty::Size { cols: size.cols, rows: size.rows }),
            }),
        };
        self.connect
            .unary::<_, pb::UpdateResponse>(crate::connect::PROC_UPDATE, &req, user.as_deref())
            .await
            .map(|_| ())
    }

    /// Kill the pty's process (SIGKILL). Returns `false` if not found.
    pub async fn kill(&self, pid: u32) -> Result<bool> {
        let user = self.default_user.clone();
        let req = pb::SendSignalRequest {
            process: Some(pid_selector(pid)),
            signal: pb::Signal::Sigkill as i32,
        };
        match self
            .connect
            .unary::<_, pb::SendSignalResponse>(crate::connect::PROC_SEND_SIGNAL, &req, user.as_deref())
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Reconnect to a running pty by pid.
    pub async fn connect(&self, pid: u32, user: Option<&str>) -> Result<CommandHandle> {
        let user = self.resolve_user(user);
        let req = pb::ConnectRequest { process: Some(pid_selector(pid)) };
        open_handle(
            Arc::clone(&self.connect),
            crate::connect::PROC_CONNECT,
            &req,
            user.as_deref(),
            &self.envd_version,
        )
        .await
    }
}
```
NOTE: `build_connect_client`/`open_handle`/`pid_selector` must be `pub(crate)` exports from `commands` (Task 1/2 make them so). Add `pub(crate) mod pty;` to `sandbox/mod.rs`; build the test `Pty` via `Pty::build_with_connect`.

- [ ] **Step 4: Wire into `Sandbox` + re-export** — add `pub(crate) pty: Pty` field + `pub fn pty(&self) -> &Pty`, built in `from_api_sandbox` (+ `local_sandbox` test helper). Re-export `Pty`/`PtySize`/`PtyCreateOpts` via `sandbox/mod.rs` → `lib.rs`.

- [ ] **Step 5: Run tests + verify & commit** — `cargo test -p e2b-rs sandbox::`, clippy, `cargo doc` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/pty crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(pty): add Pty create/send_input/resize/kill/connect" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Parity checklist, quickstart & full gate

**Files:** Modify `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc), `README.md`.

- [ ] **Step 1: Crate quickstart** — add a `## Commands` `no_run` doctest to `lib.rs` `//!` docs:
```rust
//! ## Commands & PTY
//!
//! ```no_run
//! # async fn run() -> e2b_rs::Result<()> {
//! use e2b_rs::Sandbox;
//! let sandbox = Sandbox::create().template("base").await?;
//! let result = sandbox.commands().run("echo hello", Default::default()).await?;
//! println!("exit {}: {}", result.exit_code, result.stdout);
//!
//! let mut cmd = sandbox.commands().start("sleep 1; echo done", Default::default()).await?;
//! while let Some(out) = cmd.next().await {
//!     if let e2b_rs::CommandOutput::Stdout(bytes) = out {
//!         print!("{}", String::from_utf8_lossy(&bytes));
//!     }
//! }
//! let _ = cmd.wait().await?;
//! # Ok(())
//! # }
//! ```
```
(Verify the exact method signatures against the implementation so the doctest compiles under `no_run`.)

- [ ] **Step 2: Parity checklist** — add a `## Sandbox commands & pty (Plan 3c)` table: `commands.run` (fg/bg) → `Commands::run`/`start`; `list`/`kill`/`sendStdin`/`closeStdin`/`connect`; `pty.create`/`sendInput`/`resize`/`kill`/`connect`. Note the non-zero-exit divergence (`Ok(CommandResult)` vs JS throw).

- [ ] **Step 3: README** — short commands snippet under usage. Only stage if changed.

- [ ] **Step 4: Full release gate** — run each; all must pass:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` (report counts; 0 failures)
- `cargo test --doc -p e2b-rs` (the new doctest compiles under `no_run`)
- `cargo doc --no-deps -p e2b-rs` (no broken intra-doc links)
- `cargo xtask codegen && git status --porcelain` → empty (codegen idempotent)

- [ ] **Step 5: Commit**
```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(commands): document commands & pty quickstart and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 3c is complete when:
- `Sandbox::commands()` exposes `run` (→ `Ok(CommandResult)` regardless of exit code), `start` (→ `CommandHandle` streaming `CommandOutput` via mpsc + `wait`/`kill`/`send_stdin`/`close_stdin`), `list`/`kill`/`send_stdin`/`close_stdin`/`connect`.
- `Sandbox::pty()` exposes `create`/`send_input`/`resize`/`kill`/`connect`.
- All public types (`Commands`, `Pty`, `CommandHandle`, `CommandOutput`, `CommandResult`, `ProcessInfo`, `CommandStartOpts`, `PtySize`, `PtyCreateOpts`) are re-exported at the crate root; NO generated `envd::proto` type leaks into the public API.
- Streaming is via `tokio::sync::mpsc`; the `ProcessEvent` wire decode uses the pbjson field-name oneof form; version gates raise `Error::{Sandbox,Template}`.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects commands & pty.

**Carry-forwards (out of scope, documented):** `StreamInput` client-streaming RPC (JS doesn't use it; `ConnectClient` has no client-streaming); tag-based process selection (only pid used); a per-stream `connect-timeout-ms` / `KEEPALIVE_PING_HEADER` on the process streams (a Plan-2b carry-forward — note if the long-lived `Start` stream needs it); sharing ONE `ConnectClient`/`EnvdApiClient` across `Filesystem`/`Commands`/`Pty` instead of one per subsystem (currently three reqwest clients per sandbox — a construction-dedup refactor).

**Next:** Plan 4 (Git & Volume), then Plan 5 (Template build pipeline & polish).
