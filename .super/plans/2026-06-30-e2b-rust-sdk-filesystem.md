# Sandbox Filesystem / envd I/O (Plan 3b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `sandbox.files` — read/write (incl. gzip, streaming, multipart, metadata), list/stat/exists/makeDir/remove/rename, and directory watching (server-stream → `tokio::sync::mpsc`) — matching the E2B JS SDK 1:1.

**Architecture:** A public `Filesystem`, built once when a `Sandbox` is created/connected and exposed via `Sandbox::files()`. Metadata ops are Connect-over-JSON unary RPCs on the existing `ConnectClient` (sending the generated `envd::proto::filesystem` types, which carry pbjson proto3-JSON serde). Byte I/O is REST on the `EnvdApiClient` (`GET`/`POST /files`). Directory watch uses `ConnectClient::server_stream` (the `WatchDir` RPC) driven by a background task that forwards `FilesystemEvent`s into a `tokio::sync::mpsc::Receiver` exposed by a `WatchHandle`. Generated proto/REST types stay `pub(crate)` and are wrapped in hand-written public types (`EntryInfo`, `WriteInfo`, `FileType`, `FilesystemEvent`, `FilesystemEventType`).

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), tokio (incl. `sync::mpsc`), reqwest 0.13 (add `gzip` feature), `async-compression` (gzip request bodies), futures, serde/serde_json, prost/pbjson (generated proto), chrono; wiremock for tests.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; all crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` allowed ONLY in `#[cfg(test)]`. Prefer `u32/u64/i64::try_from(...).unwrap_or(...)` over `as` casts.
- **Streaming is delivered via `tokio::sync::mpsc` channels, never callbacks.** Byte-body reads may return `impl Stream<Item = Result<Bytes>>`.
- **Do NOT expose generated `envd::proto::*` or REST `rest_gen` types in any `pub` signature/return/re-export** (spec §1 non-goal). Wrap them in hand-written public types.
- Builders finish via `IntoFuture`; instance methods are plain `async fn` (watch is `async fn … -> Result<WatchHandle>`).
- **Honest test fixtures:** mock bodies/headers must match the real wire (proto3-JSON for Connect RPCs; raw bytes / multipart / `WriteInfo[]` JSON for `/files`). A prior milestone shipped a bug from a fixture that didn't match the wire schema.
- Every task: run `cargo fmt --all` before commit. Commit trailer (exact): `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference implementation (source of truth): `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/sandbox/filesystem/` (`index.ts`, `watchHandle.ts`) and `.../sandbox/index.ts` (construction); proto in `crates/e2b-rs/src/envd/proto/filesystem.rs`.

### Pre-verified facts (confirmed against the codebase at `main` = 2d81ff4)
- Generated `envd::proto::filesystem` (`pub(crate)`, prost+pbjson serde):
  - `StatRequest { path }` → `StatResponse { entry: Option<EntryInfo> }`
  - `ListDirRequest { path, depth: u32 }` → `ListDirResponse { entries: Vec<EntryInfo> }`
  - `MakeDirRequest { path }` → `MakeDirResponse { entry: Option<EntryInfo> }`
  - `MoveRequest { source, destination }` → `MoveResponse { entry: Option<EntryInfo> }`
  - `RemoveRequest { path }` → `RemoveResponse {}` (empty)
  - `EntryInfo { name: String, r#type: i32 (FileType), path: String, size: i64, mode: u32, permissions: String, owner: String, group: String, modified_time: Option<pbjson_types::Timestamp>, symlink_target: Option<String>, metadata: HashMap<String,String> }`
  - `WatchDirRequest { path, recursive: bool, include_entry: bool, allow_network_mounts: bool }`
  - `WatchDirResponse { event: Option<watch_dir_response::Event> }` where `Event = Start(StartEvent{}) | Filesystem(FilesystemEvent) | Keepalive(KeepAlive{})`
  - `FilesystemEvent { name: String, r#type: i32 (EventType), entry: Option<EntryInfo> }`
  - `enum FileType { Unspecified=0, File=1, Directory=2 }`; `enum EventType { Unspecified=0, Create=1, Write=2, Remove=3, Rename=4, Chmod=5 }`
- RPC path consts (`crates/e2b-rs/src/connect/mod.rs`, `pub(crate)`): `FS_STAT`, `FS_LIST_DIR`, `FS_MAKE_DIR`, `FS_MOVE`, `FS_REMOVE`, `FS_WATCH_DIR`.
- `ConnectClient` (`pub(crate)`): `new(ConnectClientOpts) -> Result<Self>`; `unary<Req: Serialize, Resp: DeserializeOwned>(path, req: &Req, user: Option<&str>) -> Result<Resp>`; `server_stream<Req: Serialize, Resp: DeserializeOwned + 'static>(path, req: &Req, user: Option<&str>) -> Result<impl Stream<Item = Result<Resp>>>`. `ConnectClientOpts { base_url, access_token: Option<String>, sandbox_id, envd_port: u16, user_agent, envd_version, request_timeout_ms: u64, logger: Option<Arc<dyn Logger>>, proxy: Option<String> }`. Passing the generated proto types as `Req`/`Resp` works (they have pbjson proto3-JSON serde).
- `EnvdApiClient` (`pub(crate)`): `new(EnvdApiClientOpts) -> Result<Self>` (bakes `user-agent`/`e2b-sandbox-id`/`e2b-sandbox-port`/`x-access-token` default headers; `base_url` trimmed); `check_health()`. `EnvdApiClientOpts` has the same fields as `ConnectClientOpts` minus `envd_version`. Fields `http`/`base_url`/`request_timeout`/`logger` are private — Task 4/5 add `read`/`write` methods inside `rest.rs`.
- Connect error mapping (`connect/error.rs`): `Code::NotFound => Error::NotFound`; `Code::AlreadyExists` currently falls through to `_ => Error::Sandbox` (Task 1 fixes this). HTTP 404→`Code::NotFound`, 409→`Code::AlreadyExists`.
- `Error` variants exist: `NotFound`, `FileNotFound`, `Conflict` (added in 3a-extras), `Template`, `InvalidArgument`, `Sandbox`, `Internal`. `Error::from_status` maps 404→`NotFound`.
- Version gates (`envd::versions`, `pub(crate)`): `version_gte(actual, required) -> bool`; consts `ENVD_DEFAULT_USER="0.4.0"`, `ENVD_OCTET_STREAM_UPLOAD="0.5.7"`, `ENVD_FILE_METADATA="0.6.2"`, `ENVD_VERSION_RECURSIVE_WATCH="0.1.4"`, `ENVD_VERSION_FS_EVENT_ENTRY_INFO="0.6.3"`, `ENVD_VERSION_WATCH_NETWORK_MOUNTS="0.6.4"`.
- `ConnectionConfig::get_sandbox_url(sandbox_id, sandbox_domain, envd_port: u16) -> String`. `ConnectionConfig` public fields (CONFIRMED): `debug: bool`, `domain: String`, `api_url: String`, `sandbox_url: Option<String>`, `logger: Option<Arc<dyn Logger>>`, `request_timeout_ms: u64`, `api_key`, `validate_api_key`, `access_token`, `integration: Option<String>`, `headers: BTreeMap`, `proxy: Option<String>`, `api_inflight_requests`. `crate::connection_config::{ENVD_PORT: u16 = 49983, DEFAULT_USERNAME: &str = "user"}`. `crate::utils::build_user_agent(integration: Option<&str>) -> String` (CONFIRMED — takes the integration arg; pass `config.integration.as_deref()`). For the envd clients, thread `config.{request_timeout_ms, logger.clone(), proxy.clone()}` (do NOT hardcode).
- `Sandbox` (`sandbox/sandbox.rs`): `pub(crate)` fields `sandbox_id: String`, `sandbox_domain: Option<String>`, `envd_version: String`, `envd_access_token: Option<String>`, `config: ConnectionConfig`, `api: ApiClient`. Built by the private `from_api_sandbox(s: api::schema::Sandbox, config, api) -> Sandbox` (called in the create/connect `IntoFuture` impls). `resolved_domain(&self)` helper exists (`sandbox_domain` ?? `config.domain`).

---

## File Structure

- `crates/e2b-rs/Cargo.toml` — MODIFY: add `gzip` + `multipart` to reqwest features; add `async-compression = { version = "0.4", features = ["tokio", "gzip"] }`.
- `crates/e2b-rs/src/connect/error.rs` — MODIFY: map `Code::AlreadyExists => Error::Conflict`.
- `crates/e2b-rs/src/sandbox/filesystem/mod.rs` — CREATE: `Filesystem` struct + construction (`pub(crate) fn build(...)`) + unary ops + `pub use` of public types/`WatchHandle`.
- `crates/e2b-rs/src/sandbox/filesystem/types.rs` — CREATE: public `FileType`, `EntryInfo`, `WriteInfo`, `WriteEntry`, `FilesystemEvent`, `FilesystemEventType` + proto mappers.
- `crates/e2b-rs/src/sandbox/filesystem/io.rs` — CREATE: `impl Filesystem` `read*`/`write*` + `FsReadOpts`/`FsWriteOpts`/metadata validation.
- `crates/e2b-rs/src/sandbox/filesystem/watch.rs` — CREATE: `WatchHandle`, `WatchOpts`, `impl Filesystem` `watch_dir`.
- `crates/e2b-rs/src/envd/rest.rs` — MODIFY: add `EnvdApiClient::{get_files, post_files}` low-level helpers.
- `crates/e2b-rs/src/sandbox/sandbox.rs` — MODIFY: store `pub(crate) files: Filesystem`; add `pub fn files(&self) -> &Filesystem`; build it in `from_api_sandbox` (now returns `Result<Sandbox>`).
- `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs` — MODIFY: re-export public filesystem types.
- `docs/parity-checklist.md`, `README.md` — MODIFY (Task 7).

---

### Task 1: Cargo deps, `Code::AlreadyExists` mapping, and public filesystem types

**Files:**
- Modify: `crates/e2b-rs/Cargo.toml`, `crates/e2b-rs/src/connect/error.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`
- Create: `crates/e2b-rs/src/sandbox/filesystem/types.rs`, `crates/e2b-rs/src/sandbox/filesystem/mod.rs` (types-only stub for now)

**Interfaces:**
- Consumes: `envd::proto::filesystem::{EntryInfo as ProtoEntry, FilesystemEvent as ProtoEvent, FileType as ProtoFileType, EventType as ProtoEventType}`.
- Produces (all `pub`, in `types.rs`):
  - `enum FileType { File, Dir }` + `pub(crate) fn from_proto(i32) -> Option<FileType>` (None for Unspecified/unknown).
  - `struct EntryInfo { name, path, r#type: FileType, size: u64, mode: u32, permissions: String, owner: String, group: String, modified_time: Option<chrono::DateTime<chrono::Utc>>, symlink_target: Option<String>, metadata: BTreeMap<String,String> }` + `pub(crate) fn from_proto(ProtoEntry) -> Option<EntryInfo>` (None when type maps to None — JS filters unknown types).
  - `struct WriteInfo { name: String, path: String, r#type: Option<FileType>, metadata: BTreeMap<String,String> }`.
  - `struct WriteEntry { path: String, data: Vec<u8> }` (bytes payload; streaming variant handled in Task 5 via a separate method).
  - `enum FilesystemEventType { Create, Write, Remove, Rename, Chmod }` + `pub(crate) fn from_proto(i32) -> Option<FilesystemEventType>`.
  - `struct FilesystemEvent { name: String, r#type: FilesystemEventType, entry: Option<EntryInfo> }` + `pub(crate) fn from_proto(ProtoEvent) -> Option<FilesystemEvent>` (None when event type unknown).

- [ ] **Step 1: Add dependencies**

Dependencies here use the WORKSPACE pattern (`reqwest = { workspace = true }` in the crate; features defined in the workspace root). Make these edits:

1. **Workspace root `Cargo.toml`** (`/Users/chwzr/flxkpe/superworkspace/rust-sdk/e2b-rs/Cargo.toml`), `[workspace.dependencies]`:
   - Add `"gzip"` and `"multipart"` to the reqwest feature list (currently `["json", "query", "stream", "rustls", "webpki-roots"]`).
   - Add `async-compression = { version = "0.4", default-features = false, features = ["tokio", "gzip"] }`.
2. **Crate `crates/e2b-rs/Cargo.toml`** `[dependencies]`:
   - Add `async-compression = { workspace = true }`.
   - Change the crate's tokio line from `features = ["sync", "time"]` to `features = ["sync", "time", "rt"]` (Task 6's `watch_dir` uses `tokio::spawn`, which needs `rt`). Do NOT add `"macros"` — the implementation deliberately avoids `tokio::select!`/`tokio::pin!` (it uses `futures::pin_mut!` + `task.abort()` instead).

(`read_stream` returns a plain `impl Stream` over `resp.bytes_stream()` — `reqwest` re-exports `bytes::Bytes` as `reqwest::Bytes`, so no direct `bytes`/`tokio-util` dep is needed. Pin `async-compression` to a patch compatible with MSRV 1.95.0; if the latest `0.4.x` raises MSRV, pick the highest patch that builds.)

Run: `cargo build -p e2b-rs` → compiles (no usage yet, just dep resolution).

- [ ] **Step 2: Map `Code::AlreadyExists` → `Error::Conflict`**

In `crates/e2b-rs/src/connect/error.rs` `map_code_to_error`, add an arm above the `_ =>` fallback:
```rust
        Code::AlreadyExists => Error::Conflict(message),
```
Add a test next to the existing `map_code_to_error` tests:
```rust
        assert!(matches!(
            map_code_to_error(Code::AlreadyExists, "x".into()),
            Error::Conflict(_)
        ));
```
Run: `cargo test -p e2b-rs connect::error` → PASS.

- [ ] **Step 3: Write the failing type-mapping test**

Create `crates/e2b-rs/src/sandbox/filesystem/types.rs` with the test module first:
```rust
//! Public filesystem value types (wrapping the generated `envd::proto` types).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envd::proto::filesystem as pb;

    #[test]
    fn entry_info_maps_file_and_filters_unknown_type() {
        let proto = pb::EntryInfo {
            name: "f.txt".into(),
            r#type: pb::FileType::File as i32,
            path: "/home/user/f.txt".into(),
            size: 12,
            mode: 0o644,
            permissions: "-rw-r--r--".into(),
            owner: "user".into(),
            group: "user".into(),
            modified_time: None,
            symlink_target: None,
            metadata: std::collections::HashMap::new(),
        };
        let e = EntryInfo::from_proto(proto).expect("file entry");
        assert_eq!(e.name, "f.txt");
        assert_eq!(e.r#type, FileType::File);
        assert_eq!(e.size, 12);

        let unknown = pb::EntryInfo {
            r#type: pb::FileType::Unspecified as i32,
            ..Default::default()
        };
        assert!(EntryInfo::from_proto(unknown).is_none());
    }

    #[test]
    fn event_maps_type() {
        let proto = pb::FilesystemEvent {
            name: "x".into(),
            r#type: pb::EventType::Write as i32,
            entry: None,
        };
        let ev = FilesystemEvent::from_proto(proto).expect("event");
        assert_eq!(ev.r#type, FilesystemEventType::Write);
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `pub(crate) mod filesystem;`. Create `crates/e2b-rs/src/sandbox/filesystem/mod.rs` with `pub mod types;` and `pub use types::{EntryInfo, FileType, FilesystemEvent, FilesystemEventType, WriteEntry, WriteInfo};` (the `Filesystem` struct is added in Task 2).

- [ ] **Step 4: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::filesystem::types`
Expected: FAIL — types not defined.

- [ ] **Step 5: Implement the public types + mappers**

In `crates/e2b-rs/src/sandbox/filesystem/types.rs` (above the test module):
```rust
use std::collections::BTreeMap;

use crate::envd::proto::filesystem as pb;

/// The kind of a filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// A regular file.
    File,
    /// A directory.
    Dir,
}

impl FileType {
    /// Map the generated proto enum (i32) to the public type; `None` for
    /// unspecified/unknown values (which the JS SDK filters out).
    pub(crate) fn from_proto(value: i32) -> Option<FileType> {
        match pb::FileType::try_from(value) {
            Ok(pb::FileType::File) => Some(FileType::File),
            Ok(pb::FileType::Directory) => Some(FileType::Dir),
            _ => None,
        }
    }
}

/// Metadata for a filesystem entry (`getInfo`, `list`, `rename`).
#[derive(Debug, Clone)]
pub struct EntryInfo {
    /// Base name of the entry.
    pub name: String,
    /// Absolute path of the entry.
    pub path: String,
    /// File or directory.
    pub r#type: FileType,
    /// Size in bytes.
    pub size: u64,
    /// Unix mode bits.
    pub mode: u32,
    /// Human-readable permission string (e.g. `-rw-r--r--`).
    pub permissions: String,
    /// Owning user.
    pub owner: String,
    /// Owning group.
    pub group: String,
    /// Last-modified time, if reported.
    pub modified_time: Option<chrono::DateTime<chrono::Utc>>,
    /// Target path if the entry is a symlink.
    pub symlink_target: Option<String>,
    /// User-defined `user.e2b.*` metadata (prefix stripped by envd).
    pub metadata: BTreeMap<String, String>,
}

impl EntryInfo {
    /// Map a generated proto entry to the public type. Returns `None` when the
    /// file type is unspecified/unknown (matching the JS SDK's filtering).
    pub(crate) fn from_proto(e: pb::EntryInfo) -> Option<EntryInfo> {
        let r#type = FileType::from_proto(e.r#type)?;
        let modified_time = e.modified_time.and_then(|t| {
            let nanos = u32::try_from(t.nanos).unwrap_or(0);
            chrono::DateTime::from_timestamp(t.seconds, nanos)
        });
        Some(EntryInfo {
            name: e.name,
            path: e.path,
            r#type,
            size: u64::try_from(e.size).unwrap_or(0),
            mode: e.mode,
            permissions: e.permissions,
            owner: e.owner,
            group: e.group,
            modified_time,
            symlink_target: e.symlink_target,
            metadata: e.metadata.into_iter().collect(),
        })
    }
}

/// Result of a write — a subset of [`EntryInfo`] returned by `POST /files`.
#[derive(Debug, Clone)]
pub struct WriteInfo {
    /// Base name of the written entry.
    pub name: String,
    /// Absolute path of the written entry.
    pub path: String,
    /// File or directory, if reported.
    pub r#type: Option<FileType>,
    /// Metadata persisted on the entry.
    pub metadata: BTreeMap<String, String>,
}

/// One entry in a multi-file [`Filesystem::write_files`] call.
#[derive(Debug, Clone)]
pub struct WriteEntry {
    /// Destination path in the sandbox.
    pub path: String,
    /// File contents.
    pub data: Vec<u8>,
}

/// The kind of change reported by a directory watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemEventType {
    /// An entry was created.
    Create,
    /// An entry was written.
    Write,
    /// An entry was removed.
    Remove,
    /// An entry was renamed.
    Rename,
    /// An entry's mode changed.
    Chmod,
}

impl FilesystemEventType {
    /// Map the generated proto enum (i32); `None` for unspecified/unknown.
    pub(crate) fn from_proto(value: i32) -> Option<FilesystemEventType> {
        match pb::EventType::try_from(value) {
            Ok(pb::EventType::Create) => Some(FilesystemEventType::Create),
            Ok(pb::EventType::Write) => Some(FilesystemEventType::Write),
            Ok(pb::EventType::Remove) => Some(FilesystemEventType::Remove),
            Ok(pb::EventType::Rename) => Some(FilesystemEventType::Rename),
            Ok(pb::EventType::Chmod) => Some(FilesystemEventType::Chmod),
            _ => None,
        }
    }
}

/// A single directory-watch event.
#[derive(Debug, Clone)]
pub struct FilesystemEvent {
    /// Path (relative to the watched dir) that changed.
    pub name: String,
    /// The kind of change.
    pub r#type: FilesystemEventType,
    /// Entry info, when `include_entry` was requested and the entry still exists.
    pub entry: Option<EntryInfo>,
}

impl FilesystemEvent {
    /// Map a generated proto event; `None` when the event type is unknown.
    pub(crate) fn from_proto(e: pb::FilesystemEvent) -> Option<FilesystemEvent> {
        let r#type = FilesystemEventType::from_proto(e.r#type)?;
        Some(FilesystemEvent {
            name: e.name,
            r#type,
            entry: e.entry.and_then(EntryInfo::from_proto),
        })
    }
}
```

- [ ] **Step 6: Run tests + re-export**

Run: `cargo test -p e2b-rs sandbox::filesystem::types` → PASS.
In `crates/e2b-rs/src/lib.rs`, add to the sandbox re-export block: `EntryInfo, FileType, FilesystemEvent, FilesystemEventType, WriteEntry, WriteInfo` (these come from `sandbox::filesystem`). Add a `pub use sandbox::filesystem::{...}` line, OR re-export them through `sandbox/mod.rs` first (`pub use filesystem::{...}`) then lift in lib.rs — match the existing pattern (the existing `pub use sandbox::{...}` lifts names re-exported by `sandbox/mod.rs`). Since `filesystem` is `pub(crate)`, re-export the public types via `sandbox/mod.rs`: `pub use filesystem::{EntryInfo, FileType, FilesystemEvent, FilesystemEventType, WriteEntry, WriteInfo};`, then add those names to lib.rs's `pub use sandbox::{...}`.

- [ ] **Step 7: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::filesystem` and `cargo test -p e2b-rs connect::error` → pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.
```bash
cargo fmt --all
git add Cargo.toml Cargo.lock crates/e2b-rs/Cargo.toml crates/e2b-rs/src/connect/error.rs crates/e2b-rs/src/sandbox/filesystem crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(fs): add public filesystem types + map AlreadyExists to Conflict" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `Filesystem` struct, construction from `Sandbox`, and Stat-based ops (`get_info`, `exists`)

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/filesystem/mod.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`

**Interfaces:**
- Consumes: `ConnectClient`, `ConnectClientOpts`, `EnvdApiClient`, `EnvdApiClientOpts`, `ConnectionConfig`, `crate::utils::build_user_agent`, `crate::connection_config::{ENVD_PORT, DEFAULT_USERNAME, REQUEST_TIMEOUT_MS}`, `version_gte`, `ENVD_DEFAULT_USER`, proto `StatRequest/StatResponse`, `FS_STAT`.
- Produces:
  - `pub struct Filesystem { connect: ConnectClient, rest: EnvdApiClient, envd_version: String, default_user: Option<String> }` (fields `pub(crate)`).
  - `pub(crate) fn Filesystem::build(sandbox_id: &str, sandbox_domain: &str, envd_version: &str, envd_access_token: Option<&str>, config: &ConnectionConfig) -> Result<Filesystem>`.
  - `pub(crate) fn Filesystem::resolve_user(&self, user: Option<&str>) -> Option<String>` (explicit user, else `DEFAULT_USERNAME` when envd < 0.4.0, else `None`).
  - `pub async fn Filesystem::get_info(&self, path: &str, user: Option<&str>) -> Result<EntryInfo>`.
  - `pub async fn Filesystem::exists(&self, path: &str, user: Option<&str>) -> Result<bool>`.
  - `pub(crate) fn file_not_found_on_missing(err: Error, path: &str) -> Error` (maps `Error::NotFound` → `Error::FileNotFound("... {path} ...")`).
  - `Sandbox`: new `pub(crate) files: Filesystem` field; `pub fn files(&self) -> &Filesystem`; `from_api_sandbox` now returns `Result<Sandbox>` and builds the `Filesystem`.

- [ ] **Step 1: Write the failing tests**

In `crates/e2b-rs/src/sandbox/filesystem/mod.rs`, add a `#[cfg(test)] mod tests`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fs_for(server: &MockServer) -> Filesystem {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            ..Default::default()
        });
        // Point the envd clients straight at the mock server.
        Filesystem::build_with_base_url(server.uri(), "sbx_fs", "0.6.3", None, &config)
            .expect("filesystem")
    }

    fn entry_json(path: &str) -> serde_json::Value {
        serde_json::json!({
            "name": "f.txt", "type": "FILE_TYPE_FILE", "path": path,
            "size": "12", "mode": 420, "permissions": "-rw-r--r--",
            "owner": "user", "group": "user", "metadata": {}
        })
    }

    #[tokio::test]
    async fn get_info_returns_entry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/Stat"))
            .and(body_partial_json(serde_json::json!({ "path": "/home/user/f.txt" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "entry": entry_json("/home/user/f.txt") }),
            ))
            .mount(&server)
            .await;
        let info = fs_for(&server)
            .get_info("/home/user/f.txt", None)
            .await
            .expect("info");
        assert_eq!(info.path, "/home/user/f.txt");
        assert_eq!(info.r#type, FileType::File);
    }

    #[tokio::test]
    async fn exists_false_on_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/Stat"))
            .respond_with(ResponseTemplate::new(404).set_body_json(
                serde_json::json!({ "code": "not_found", "message": "missing" }),
            ))
            .mount(&server)
            .await;
        assert!(!fs_for(&server).exists("/nope", None).await.expect("exists"));
    }
}
```

NOTE: the test uses a test-only `build_with_base_url` shim that bypasses `get_sandbox_url` (so the clients hit the mock server directly). Implement it in Step 3 alongside `build`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests`
Expected: FAIL — `Filesystem` not defined.

- [ ] **Step 3: Implement `Filesystem` + construction + Stat ops**

In `crates/e2b-rs/src/sandbox/filesystem/mod.rs` (above the test module):
```rust
pub mod types;

pub use types::{EntryInfo, FileType, FilesystemEvent, FilesystemEventType, WriteEntry, WriteInfo};

use crate::connect::client::{ConnectClient, ConnectClientOpts};
use crate::connection_config::{ConnectionConfig, DEFAULT_USERNAME, ENVD_PORT};
use crate::envd::proto::filesystem as pb;
use crate::envd::rest::{EnvdApiClient, EnvdApiClientOpts};
use crate::envd::versions::{ENVD_DEFAULT_USER, version_gte};
use crate::errors::{Error, Result};

/// Map a filesystem `NotFound` to the file-specific [`Error::FileNotFound`]
/// (matching the JS SDK, which raises `FileNotFoundError`).
pub(crate) fn file_not_found_on_missing(err: Error, path: &str) -> Error {
    match err {
        Error::NotFound(_) => Error::FileNotFound(format!("File not found: {path}")),
        other => other,
    }
}

/// The sandbox filesystem: read/write byte I/O over the envd REST `/files`
/// surface plus metadata operations over the Connect `Filesystem` service.
pub struct Filesystem {
    pub(crate) connect: ConnectClient,
    pub(crate) rest: EnvdApiClient,
    pub(crate) envd_version: String,
    pub(crate) default_user: Option<String>,
}

impl Filesystem {
    /// Build a `Filesystem` for a sandbox, resolving the envd base URL from the
    /// connection config (envd port `49983`).
    pub(crate) fn build(
        sandbox_id: &str,
        sandbox_domain: &str,
        envd_version: &str,
        envd_access_token: Option<&str>,
        config: &ConnectionConfig,
    ) -> Result<Filesystem> {
        let base_url = config.get_sandbox_url(sandbox_id, sandbox_domain, ENVD_PORT);
        Self::build_with_base_url(base_url, sandbox_id, envd_version, envd_access_token, config)
    }

    /// Build directly from a base URL (used by tests + by [`Filesystem::build`]).
    pub(crate) fn build_with_base_url(
        base_url: String,
        sandbox_id: &str,
        envd_version: &str,
        envd_access_token: Option<&str>,
        config: &ConnectionConfig,
    ) -> Result<Filesystem> {
        let user_agent = crate::utils::build_user_agent(config.integration.as_deref());
        let connect = ConnectClient::new(ConnectClientOpts {
            base_url: base_url.clone(),
            access_token: envd_access_token.map(str::to_string),
            sandbox_id: sandbox_id.to_string(),
            envd_port: ENVD_PORT,
            user_agent: user_agent.clone(),
            envd_version: envd_version.to_string(),
            request_timeout_ms: config.request_timeout_ms,
            logger: config.logger.clone(),
            proxy: config.proxy.clone(),
        })?;
        let rest = EnvdApiClient::new(EnvdApiClientOpts {
            base_url,
            access_token: envd_access_token.map(str::to_string),
            sandbox_id: sandbox_id.to_string(),
            envd_port: ENVD_PORT,
            user_agent,
            request_timeout_ms: config.request_timeout_ms,
            logger: config.logger.clone(),
            proxy: config.proxy.clone(),
        })?;
        // Older envd (<0.4.0) has no per-request user; default to the legacy user.
        let default_user = (!version_gte(envd_version, ENVD_DEFAULT_USER))
            .then(|| DEFAULT_USERNAME.to_string());
        Ok(Filesystem {
            connect,
            rest,
            envd_version: envd_version.to_string(),
            default_user,
        })
    }

    /// Resolve the user for a request: explicit `user`, else the legacy default
    /// on old envd, else `None`.
    pub(crate) fn resolve_user(&self, user: Option<&str>) -> Option<String> {
        match user {
            Some(u) => Some(u.to_string()),
            None => self.default_user.clone(),
        }
    }

    /// Get metadata for a path. Errors with [`Error::FileNotFound`] if missing.
    pub async fn get_info(&self, path: &str, user: Option<&str>) -> Result<EntryInfo> {
        let user = self.resolve_user(user);
        let req = pb::StatRequest { path: path.to_string() };
        let resp: pb::StatResponse = self
            .connect
            .unary(crate::connect::FS_STAT, &req, user.as_deref())
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        resp.entry
            .and_then(EntryInfo::from_proto)
            .ok_or_else(|| Error::Internal(format!("Stat returned no entry for {path}")))
    }

    /// Whether a path exists.
    pub async fn exists(&self, path: &str, user: Option<&str>) -> Result<bool> {
        match self.get_info(path, user).await {
            Ok(_) => Ok(true),
            Err(Error::FileNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}
```
NOTE: confirm `ConnectionConfig` has a `logger: Option<Arc<dyn Logger>>` field (check `connection_config.rs`); if the field has a different name/shape, adjust the two `config.logger.clone()` lines. Confirm `crate::utils::build_user_agent` exists with that exact name (it was built in Plan 1); if its signature differs, adjust.

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests` → 2 pass.

- [ ] **Step 5: Wire `Filesystem` into `Sandbox`**

In `crates/e2b-rs/src/sandbox/sandbox.rs`:
1. Add a field to the `Sandbox` struct: `pub(crate) files: crate::sandbox::filesystem::Filesystem,` (documented `/// Filesystem operations (sandbox.files).`).
2. Change `from_api_sandbox` to return `Result<Sandbox>` and build the filesystem. The current body constructs the struct literal; wrap it:
```rust
    fn from_api_sandbox(
        s: crate::api::schema::Sandbox,
        config: ConnectionConfig,
        api: ApiClient,
    ) -> Result<Sandbox> {
        let sandbox_domain = s.domain.clone();
        let domain = sandbox_domain
            .clone()
            .unwrap_or_else(|| config.domain.clone());
        let files = crate::sandbox::filesystem::Filesystem::build(
            &s.sandbox_id,
            &domain,
            &s.envd_version.0,
            s.envd_access_token.as_deref(),
            &config,
        )?;
        Ok(Sandbox {
            sandbox_id: s.sandbox_id,
            sandbox_domain,
            envd_version: s.envd_version.0,
            envd_access_token: s.envd_access_token,
            config,
            api,
            files,
        })
    }
```
3. Update the two `IntoFuture` impls (create + connect) that call `Sandbox::from_api_sandbox(...)` to use `?`: `Ok(Sandbox::from_api_sandbox(sandbox, config, api)?)`.
4. Add the accessor:
```rust
    /// Access the sandbox filesystem (`read`/`write`/`list`/`watch`/...).
    pub fn files(&self) -> &crate::sandbox::filesystem::Filesystem {
        &self.files
    }
```

- [ ] **Step 6: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::` → all pass (the existing create/connect tests still pass with `from_api_sandbox` now fallible). `cargo clippy --workspace --all-targets -- -D warnings` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/filesystem/mod.rs crates/e2b-rs/src/sandbox/sandbox.rs
git commit -m "feat(fs): add Filesystem (construction + get_info/exists) wired into Sandbox" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Remaining unary ops — `list`, `make_dir`, `remove`, `rename`

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/filesystem/mod.rs`

**Interfaces:**
- Consumes: proto `ListDirRequest/Response`, `MakeDirRequest/Response`, `MoveRequest/Response`, `RemoveRequest`; `FS_LIST_DIR`, `FS_MAKE_DIR`, `FS_MOVE`, `FS_REMOVE`; `Error::{Conflict, InvalidArgument}`.
- Produces (on `Filesystem`):
  - `pub async fn list(&self, path: &str, depth: Option<u32>, user: Option<&str>) -> Result<Vec<EntryInfo>>` (default depth 1; `depth < 1` → `Error::InvalidArgument`; filters unknown types).
  - `pub async fn make_dir(&self, path: &str, user: Option<&str>) -> Result<bool>` (`false` if it already exists).
  - `pub async fn remove(&self, path: &str, user: Option<&str>) -> Result<()>`.
  - `pub async fn rename(&self, old_path: &str, new_path: &str, user: Option<&str>) -> Result<EntryInfo>`.

- [ ] **Step 1: Write failing tests**

In `mod.rs` `mod tests`:
```rust
    #[tokio::test]
    async fn list_returns_entries_and_filters_unknown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/ListDir"))
            .and(body_partial_json(serde_json::json!({ "path": "/d", "depth": 1 })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": [
                    entry_json("/d/f.txt"),
                    { "name": "weird", "type": "FILE_TYPE_UNSPECIFIED", "path": "/d/weird" }
                ]
            })))
            .mount(&server)
            .await;
        let out = fs_for(&server).list("/d", None, None).await.expect("list");
        assert_eq!(out.len(), 1); // unknown-type entry filtered out
        assert_eq!(out[0].path, "/d/f.txt");
    }

    #[tokio::test]
    async fn list_rejects_zero_depth() {
        let server = MockServer::start().await;
        let err = fs_for(&server).list("/d", Some(0), None).await.expect_err("depth");
        assert!(matches!(err, crate::errors::Error::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn make_dir_false_on_already_exists() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/MakeDir"))
            .respond_with(ResponseTemplate::new(409).set_body_json(
                serde_json::json!({ "code": "already_exists", "message": "exists" }),
            ))
            .mount(&server)
            .await;
        assert!(!fs_for(&server).make_dir("/d", None).await.expect("makedir"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests::list sandbox::filesystem::tests::make_dir`
Expected: FAIL — methods not found.

- [ ] **Step 3: Implement**

Add to `impl Filesystem` in `mod.rs`:
```rust
    /// List directory entries, descending `depth` levels (default 1). Entries
    /// with an unknown file type are skipped.
    pub async fn list(
        &self,
        path: &str,
        depth: Option<u32>,
        user: Option<&str>,
    ) -> Result<Vec<EntryInfo>> {
        let depth = depth.unwrap_or(1);
        if depth < 1 {
            return Err(Error::InvalidArgument(
                "list depth must be at least 1".to_string(),
            ));
        }
        let user = self.resolve_user(user);
        let req = pb::ListDirRequest { path: path.to_string(), depth };
        let resp: pb::ListDirResponse = self
            .connect
            .unary(crate::connect::FS_LIST_DIR, &req, user.as_deref())
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        Ok(resp.entries.into_iter().filter_map(EntryInfo::from_proto).collect())
    }

    /// Create a directory. Returns `false` if it already exists.
    pub async fn make_dir(&self, path: &str, user: Option<&str>) -> Result<bool> {
        let user = self.resolve_user(user);
        let req = pb::MakeDirRequest { path: path.to_string() };
        match self
            .connect
            .unary::<_, pb::MakeDirResponse>(crate::connect::FS_MAKE_DIR, &req, user.as_deref())
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::Conflict(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Remove a file or directory.
    pub async fn remove(&self, path: &str, user: Option<&str>) -> Result<()> {
        let user = self.resolve_user(user);
        let req = pb::RemoveRequest { path: path.to_string() };
        self.connect
            .unary::<_, pb::RemoveResponse>(crate::connect::FS_REMOVE, &req, user.as_deref())
            .await
            .map(|_| ())
            .map_err(|e| file_not_found_on_missing(e, path))
    }

    /// Move/rename an entry, returning the moved entry's info.
    pub async fn rename(
        &self,
        old_path: &str,
        new_path: &str,
        user: Option<&str>,
    ) -> Result<EntryInfo> {
        let user = self.resolve_user(user);
        let req = pb::MoveRequest {
            source: old_path.to_string(),
            destination: new_path.to_string(),
        };
        let resp: pb::MoveResponse = self
            .connect
            .unary(crate::connect::FS_MOVE, &req, user.as_deref())
            .await
            .map_err(|e| file_not_found_on_missing(e, old_path))?;
        resp.entry
            .and_then(EntryInfo::from_proto)
            .ok_or_else(|| Error::Internal(format!("Move returned no entry for {new_path}")))
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p e2b-rs sandbox::filesystem` → all pass.

- [ ] **Step 5: Verify & commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/filesystem/mod.rs
git commit -m "feat(fs): add list/make_dir/remove/rename unary ops" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: REST read (`read` text/bytes + `read_stream` + gzip)

**Files:**
- Modify: `crates/e2b-rs/src/envd/rest.rs`, `crates/e2b-rs/src/sandbox/filesystem/mod.rs`
- Create: `crates/e2b-rs/src/sandbox/filesystem/io.rs`

**Interfaces:**
- Consumes: `EnvdApiClient` internals (`http`, `base_url`, `request_timeout`), `reqwest`, `futures::Stream`, `reqwest::Bytes`, `Error::{FileNotFound, Transport, Internal}`.
- Produces:
  - `EnvdApiClient::get_files(&self, path: &str, user: Option<&str>, gzip: bool) -> Result<reqwest::Response>` (builds `GET {base}/files?path&username`, `Accept-Encoding: gzip` when requested, maps 404→`FileNotFound`, non-2xx→error).
  - `Filesystem::read(&self, path: &str, user: Option<&str>) -> Result<String>` (UTF-8 text).
  - `Filesystem::read_bytes(&self, path: &str, user: Option<&str>) -> Result<Vec<u8>>`.
  - `Filesystem::read_stream(&self, path: &str, user: Option<&str>) -> Result<impl futures::Stream<Item = Result<reqwest::Bytes>>>`.
  - A `FsReadOpts { gzip: bool }`-style param folded into a `*_with` variant OR a bool arg — keep the common methods simple (no gzip) and add `read_bytes_gzip` if needed; **decision:** expose `gzip` only on the low-level `get_files` and have `read*` default `gzip=false` (gzip request-decompression is handled transparently by reqwest's `gzip` feature regardless, so the public read methods need not branch — document that responses are auto-decompressed).

- [ ] **Step 1: Write failing tests**

In `crates/e2b-rs/src/sandbox/filesystem/mod.rs` `mod tests` (read methods live on `Filesystem` via `io.rs`):
```rust
    #[tokio::test]
    async fn read_text_and_bytes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .and(wiremock::matchers::query_param("path", "/f.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
            .mount(&server)
            .await;
        let fs = fs_for(&server);
        assert_eq!(fs.read("/f.txt", None).await.expect("text"), "hello");
        assert_eq!(fs.read_bytes("/f.txt", None).await.expect("bytes"), b"hello");
    }

    #[tokio::test]
    async fn read_missing_is_file_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/files"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = fs_for(&server).read("/nope", None).await.expect_err("404");
        assert!(matches!(err, crate::errors::Error::FileNotFound(_)));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests::read`
Expected: FAIL — methods not found.

- [ ] **Step 3: Add `EnvdApiClient::get_files`**

In `crates/e2b-rs/src/envd/rest.rs`, add to `impl EnvdApiClient`:
```rust
    /// `GET {base}/files?path&username`. Maps 404 to [`Error::FileNotFound`] and
    /// other non-2xx statuses to an error; returns the streaming response on success.
    pub(crate) async fn get_files(
        &self,
        path: &str,
        user: Option<&str>,
        gzip: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/files", self.base_url);
        let mut query: Vec<(&str, String)> = vec![("path", path.to_string())];
        if let Some(user) = user {
            query.push(("username", user.to_string()));
        }
        let mut rb = self.http.get(&url).timeout(self.request_timeout).query(&query);
        if gzip {
            rb = rb.header(reqwest::header::ACCEPT_ENCODING, "gzip");
        }
        let resp = rb.send().await?;
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        Err(crate::errors::Error::from_status(status.as_u16(), &body))
    }
```
(Confirm `Error::from_status` exists with `(u16, &str)`; it does, in `errors.rs`. A 404 → `Error::NotFound`; the `Filesystem::read*` wrappers remap to `FileNotFound` via `file_not_found_on_missing`.)

- [ ] **Step 4: Implement `Filesystem::read*` in `io.rs`**

Create `crates/e2b-rs/src/sandbox/filesystem/io.rs`:
```rust
//! Byte I/O for the sandbox filesystem (`read`/`write` over envd `/files`).

use futures::StreamExt as _;

use super::{Filesystem, file_not_found_on_missing};
use crate::errors::{Error, Result};

impl Filesystem {
    /// Read a file as UTF-8 text.
    pub async fn read(&self, path: &str, user: Option<&str>) -> Result<String> {
        let bytes = self.read_bytes(path, user).await?;
        String::from_utf8(bytes)
            .map_err(|e| Error::Internal(format!("file {path} is not valid UTF-8: {e}")))
    }

    /// Read a file as raw bytes.
    pub async fn read_bytes(&self, path: &str, user: Option<&str>) -> Result<Vec<u8>> {
        let user = self.resolve_user(user);
        let resp = self
            .rest
            .get_files(path, user.as_deref(), false)
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Read a file as a stream of byte chunks (for large files). The Global
    /// Constraints allow byte-body reads to be `impl Stream` (no background
    /// task / channel needed — the response body IS already a stream).
    pub async fn read_stream(
        &self,
        path: &str,
        user: Option<&str>,
    ) -> Result<impl futures::Stream<Item = Result<reqwest::Bytes>>> {
        let user = self.resolve_user(user);
        let resp = self
            .rest
            .get_files(path, user.as_deref(), false)
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        Ok(resp.bytes_stream().map(|chunk| chunk.map_err(Error::from)))
    }
}
```
NOTE: `read_stream` returns `impl Stream` directly (idiomatic for a byte body; allowed by the Global Constraints) — no `tokio::spawn`/channel. The mpsc design is reserved for `watch_dir` (Task 6), where a background task is genuinely needed to drive the Connect stream and map proto events. The `use futures::StreamExt as _;` import at the top of `io.rs` provides `.map`. Add `pub(crate) mod io;` to `mod.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests::read` → pass.

- [ ] **Step 6: Verify & commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/envd/rest.rs crates/e2b-rs/src/sandbox/filesystem/io.rs crates/e2b-rs/src/sandbox/filesystem/mod.rs
git commit -m "feat(fs): add read/read_bytes/read_stream over envd /files" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: REST write (single + multi, octet-stream + multipart, metadata, gzip, streaming)

**Files:**
- Modify: `crates/e2b-rs/src/envd/rest.rs`, `crates/e2b-rs/src/sandbox/filesystem/io.rs`

**Interfaces:**
- Consumes: `EnvdApiClient` internals; `reqwest::multipart`; `async_compression`; `version_gte`, `ENVD_OCTET_STREAM_UPLOAD`, `ENVD_FILE_METADATA`; `Error::{Template, InvalidArgument, FileNotFound}`.
- Produces:
  - `EnvdApiClient::post_files(&self, path: Option<&str>, user: Option<&str>, body: reqwest::Body, content_type: &str, content_encoding: Option<&str>, metadata_headers: &[(String, String)]) -> Result<Vec<WriteInfoWire>>` where `WriteInfoWire` is a small `#[derive(Deserialize)]` struct `{ name, #[serde(rename="type")] type_: Option<String>, path, metadata: Option<HashMap<String,String>> }` (private to `rest.rs`) — OR return the parsed JSON `Vec<serde_json::Value>` and map in `io.rs`. **Decision:** parse into a private wire struct in `rest.rs` and return `Vec<WriteInfoWire>`; map to public `WriteInfo` in `io.rs`.
  - `Filesystem::write(&self, path: &str, data: impl Into<Vec<u8>>, opts: FsWriteOpts) -> Result<WriteInfo>`.
  - `Filesystem::write_files(&self, files: Vec<WriteEntry>, opts: FsWriteOpts) -> Result<Vec<WriteInfo>>`.
  - `pub struct FsWriteOpts { pub user: Option<String>, pub metadata: BTreeMap<String,String>, pub gzip: bool, pub use_octet_stream: Option<bool> }` (`#[derive(Default)]`).
  - metadata validation: keys = RFC 7230 token chars, values = US-ASCII (mirror `validateMetadata` in JS); header name `X-Metadata-{key}`.

- [ ] **Step 1: Write failing tests**

In `mod.rs` `mod tests`:
```rust
    #[tokio::test]
    async fn write_single_octet_stream_returns_info() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/files"))
            .and(wiremock::matchers::query_param("path", "/w.txt"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "name": "w.txt", "type": "FILE_TYPE_FILE", "path": "/w.txt", "metadata": {} }
            ])))
            .mount(&server)
            .await;
        let fs = fs_for(&server);
        let opts = crate::sandbox::filesystem::FsWriteOpts {
            use_octet_stream: Some(true),
            ..Default::default()
        };
        let info = fs.write("/w.txt", b"hi".to_vec(), opts).await.expect("write");
        assert_eq!(info.path, "/w.txt");
    }

    #[tokio::test]
    async fn write_rejects_metadata_on_old_envd() {
        // fs_for uses envd 0.6.3 ≥ 0.6.2, so build an old-envd fs explicitly.
        let server = MockServer::start().await;
        let config = ConnectionConfig::new(ConnectionConfigOpts::default());
        let fs = Filesystem::build_with_base_url(server.uri(), "sbx", "0.6.1", None, &config)
            .expect("fs");
        let mut opts = crate::sandbox::filesystem::FsWriteOpts::default();
        opts.metadata.insert("k".into(), "v".into());
        let err = fs.write("/w.txt", b"hi".to_vec(), opts).await.expect_err("gate");
        assert!(matches!(err, crate::errors::Error::Template(_)));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests::write`
Expected: FAIL — methods/types not found.

- [ ] **Step 3: Add `EnvdApiClient::post_files` + wire struct**

In `crates/e2b-rs/src/envd/rest.rs`:
```rust
/// One element of the `POST /files` JSON response.
#[derive(serde::Deserialize)]
pub(crate) struct WriteInfoWire {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub path: String,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

impl EnvdApiClient {
    /// `POST {base}/files` with a prepared body + content-type. `path` is sent as
    /// a query param for single-file writes (omitted for multi-file). Maps non-2xx
    /// to an error; parses the `WriteInfo[]` JSON response.
    pub(crate) async fn post_files(
        &self,
        path: Option<&str>,
        user: Option<&str>,
        body: reqwest::Body,
        content_type: &str,
        content_encoding: Option<&str>,
        metadata_headers: &[(String, String)],
    ) -> Result<Vec<WriteInfoWire>> {
        let url = format!("{}/files", self.base_url);
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(path) = path {
            query.push(("path", path.to_string()));
        }
        if let Some(user) = user {
            query.push(("username", user.to_string()));
        }
        let mut rb = self
            .http
            .post(&url)
            .timeout(self.request_timeout)
            .query(&query)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body);
        if let Some(enc) = content_encoding {
            rb = rb.header(reqwest::header::CONTENT_ENCODING, enc);
        }
        for (name, value) in metadata_headers {
            rb = rb.header(name.as_str(), value.as_str());
        }
        let resp = rb.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let msg = String::from_utf8_lossy(&bytes).to_string();
            return Err(crate::errors::Error::from_status(status.as_u16(), &msg));
        }
        serde_json::from_slice::<Vec<WriteInfoWire>>(&bytes)
            .map_err(|e| crate::errors::Error::Internal(format!("failed to decode write response: {e}")))
    }

    /// `POST {base}/files` as `multipart/form-data` (reqwest sets the
    /// content-type + boundary). Used for the multipart write path. `path` is
    /// sent as a query for single-file writes (the filename in the form part
    /// also carries it); omitted for multi-file writes.
    pub(crate) async fn post_files_multipart(
        &self,
        path: Option<&str>,
        user: Option<&str>,
        form: reqwest::multipart::Form,
        metadata_headers: &[(String, String)],
    ) -> Result<Vec<WriteInfoWire>> {
        let url = format!("{}/files", self.base_url);
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(path) = path {
            query.push(("path", path.to_string()));
        }
        if let Some(user) = user {
            query.push(("username", user.to_string()));
        }
        let mut rb = self
            .http
            .post(&url)
            .timeout(self.request_timeout)
            .query(&query)
            .multipart(form);
        for (name, value) in metadata_headers {
            rb = rb.header(name.as_str(), value.as_str());
        }
        let resp = rb.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let msg = String::from_utf8_lossy(&bytes).to_string();
            return Err(crate::errors::Error::from_status(status.as_u16(), &msg));
        }
        serde_json::from_slice::<Vec<WriteInfoWire>>(&bytes)
            .map_err(|e| crate::errors::Error::Internal(format!("failed to decode write response: {e}")))
    }
}
```

- [ ] **Step 4: Implement `FsWriteOpts` + write methods + validation in `io.rs`**

Add to `crates/e2b-rs/src/sandbox/filesystem/io.rs`:
```rust
use std::collections::BTreeMap;

use super::{WriteEntry, WriteInfo, types::FileType};
use crate::envd::rest::WriteInfoWire;
use crate::envd::versions::{ENVD_FILE_METADATA, ENVD_OCTET_STREAM_UPLOAD, version_gte};

/// Header prefix for user metadata, mirroring the JS SDK.
const METADATA_HEADER_PREFIX: &str = "X-Metadata-";

/// Options for [`Filesystem::write`] / [`Filesystem::write_files`].
#[derive(Default)]
pub struct FsWriteOpts {
    /// The sandbox user to write as.
    pub user: Option<String>,
    /// Metadata to persist on the uploaded file(s) (requires envd >= 0.6.2).
    pub metadata: BTreeMap<String, String>,
    /// Gzip-compress the upload (implies octet-stream; requires envd >= 0.5.7).
    pub gzip: bool,
    /// Force octet-stream (`Some(true)`) or multipart (`Some(false)`); `None` =
    /// auto (octet-stream when `gzip`, else multipart).
    pub use_octet_stream: Option<bool>,
}

/// Validate metadata keys (RFC 7230 token chars) and values (US-ASCII).
fn validate_metadata(metadata: &BTreeMap<String, String>) -> Result<()> {
    fn is_token_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
    }
    for (k, v) in metadata {
        if k.is_empty() || !k.chars().all(is_token_char) {
            return Err(Error::InvalidArgument(format!("Invalid metadata key {k:?}")));
        }
        if !v.is_ascii() {
            return Err(Error::InvalidArgument(format!(
                "Invalid metadata value for key {k:?} (must be US-ASCII)"
            )));
        }
    }
    Ok(())
}

fn map_write_info(w: WriteInfoWire) -> WriteInfo {
    WriteInfo {
        name: w.name,
        path: w.path,
        r#type: w.type_.and_then(|t| match t.as_str() {
            "FILE_TYPE_FILE" => Some(FileType::File),
            "FILE_TYPE_DIRECTORY" => Some(FileType::Dir),
            _ => None,
        }),
        metadata: w.metadata.unwrap_or_default().into_iter().collect(),
    }
}

impl Filesystem {
    /// Common write path: build headers (with version gating) and choose the
    /// octet-stream vs multipart encoding.
    fn write_headers(&self, opts: &FsWriteOpts) -> Result<Vec<(String, String)>> {
        if !opts.metadata.is_empty() && !version_gte(&self.envd_version, ENVD_FILE_METADATA) {
            return Err(Error::Template(format!(
                "File metadata requires a newer template (envd >= {ENVD_FILE_METADATA})"
            )));
        }
        if (opts.gzip || opts.use_octet_stream == Some(true))
            && !version_gte(&self.envd_version, ENVD_OCTET_STREAM_UPLOAD)
        {
            return Err(Error::Template(format!(
                "Octet-stream upload requires a newer template (envd >= {ENVD_OCTET_STREAM_UPLOAD})"
            )));
        }
        validate_metadata(&opts.metadata)?;
        Ok(opts
            .metadata
            .iter()
            .map(|(k, v)| (format!("{METADATA_HEADER_PREFIX}{k}"), v.clone()))
            .collect())
    }

    /// Write a single file, returning its [`WriteInfo`].
    pub async fn write(
        &self,
        path: &str,
        data: impl Into<Vec<u8>>,
        opts: FsWriteOpts,
    ) -> Result<WriteInfo> {
        let headers = self.write_headers(&opts)?;
        let user = self.resolve_user(opts.user.as_deref());
        let data = data.into();
        let use_octet = opts.use_octet_stream.unwrap_or(opts.gzip);

        let infos = if use_octet {
            let (body, encoding) = if opts.gzip {
                (gzip_bytes(&data).await?, Some("gzip"))
            } else {
                (data, None)
            };
            self.rest
                .post_files(
                    Some(path),
                    user.as_deref(),
                    reqwest::Body::from(body),
                    "application/octet-stream",
                    encoding,
                    &headers,
                )
                .await
                .map_err(|e| file_not_found_on_missing(e, path))?
        } else {
            let form = reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::bytes(data).file_name(path.to_string()),
            );
            self.rest
                .post_files_multipart(Some(path), user.as_deref(), form, &headers)
                .await
                .map_err(|e| file_not_found_on_missing(e, path))?
        };
        infos
            .into_iter()
            .next()
            .map(map_write_info)
            .ok_or_else(|| Error::Internal(format!("write to {path} returned no info")))
    }

    /// Write multiple files in one request. Always multipart (one `file` part per
    /// entry); `gzip` is ignored for the multi-file form.
    pub async fn write_files(
        &self,
        files: Vec<WriteEntry>,
        opts: FsWriteOpts,
    ) -> Result<Vec<WriteInfo>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let headers = self.write_headers(&opts)?;
        let user = self.resolve_user(opts.user.as_deref());
        let mut form = reqwest::multipart::Form::new();
        for entry in files {
            form = form.part(
                "file",
                reqwest::multipart::Part::bytes(entry.data).file_name(entry.path),
            );
        }
        let infos = self
            .rest
            .post_files_multipart(None, user.as_deref(), form, &headers)
            .await?;
        Ok(infos.into_iter().map(map_write_info).collect())
    }
}

/// Gzip-compress a byte buffer.
async fn gzip_bytes(data: &[u8]) -> Result<Vec<u8>> {
    use async_compression::tokio::write::GzipEncoder;
    use tokio::io::AsyncWriteExt as _;
    let mut encoder = GzipEncoder::new(Vec::new());
    encoder
        .write_all(data)
        .await
        .map_err(|e| Error::Internal(format!("gzip failed: {e}")))?;
    encoder
        .shutdown()
        .await
        .map_err(|e| Error::Internal(format!("gzip finalize failed: {e}")))?;
    Ok(encoder.into_inner())
}
```
**IMPLEMENTATION NOTE:** the octet-stream path uses `post_files` (raw `reqwest::Body`); the multipart path uses `post_files_multipart` (added in Step 3 — reqwest sets the boundary content-type). `reqwest::multipart::Part::bytes` requires the `multipart` reqwest feature — if `cargo build` reports it missing, add `"multipart"` to the reqwest features in `Cargo.toml` (Task 1 may already need it; add there if so). A streaming octet-stream upload from an `AsyncRead`/`Stream` source (`reqwest::Body::wrap_stream(...)`) is OUT OF SCOPE for this task — the buffered `write`/`write_files` cover the common case; record `write_stream` as a carry-forward.

- [ ] **Step 5: Run tests**

Run: `cargo test -p e2b-rs sandbox::filesystem::tests::write` → pass.

- [ ] **Step 6: Re-export `FsWriteOpts` + verify & commit**

Re-export `FsWriteOpts` (and `FsReadOpts` if added) via `sandbox/filesystem/mod.rs` → `sandbox/mod.rs` → `lib.rs`. Run `cargo clippy --workspace --all-targets -- -D warnings` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/envd/rest.rs crates/e2b-rs/src/sandbox/filesystem/io.rs crates/e2b-rs/src/sandbox/filesystem/mod.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(fs): add write/write_files (octet-stream+multipart, metadata, gzip)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: `watch_dir` — server-stream → `tokio::sync::mpsc` `WatchHandle`

**Files:**
- Create: `crates/e2b-rs/src/sandbox/filesystem/watch.rs`
- Modify: `crates/e2b-rs/src/sandbox/filesystem/mod.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `ConnectClient::server_stream`, proto `WatchDirRequest`/`WatchDirResponse`/`watch_dir_response::Event`; `FS_WATCH_DIR`; `version_gte` + `ENVD_VERSION_RECURSIVE_WATCH`/`ENVD_VERSION_FS_EVENT_ENTRY_INFO`/`ENVD_VERSION_WATCH_NETWORK_MOUNTS`; `tokio::sync::mpsc`, `tokio::task::JoinHandle`, `futures::StreamExt`.
- Produces:
  - `pub struct WatchOpts { pub recursive: bool, pub include_entry: bool, pub allow_network_mounts: bool, pub user: Option<String> }` (`#[derive(Default)]`).
  - `pub struct WatchHandle { events: mpsc::Receiver<FilesystemEvent>, task: JoinHandle<()> }` with `pub async fn next(&mut self) -> Option<FilesystemEvent>`, `pub fn events(&mut self) -> &mut mpsc::Receiver<FilesystemEvent>`, and `pub fn stop(self)` (aborts the background task; `Drop` also aborts).
  - `pub async fn Filesystem::watch_dir(&self, path: &str, opts: WatchOpts) -> Result<WatchHandle>` (validates version gates; opens the stream; waits for the `StartEvent`; spawns the forwarding task).

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/sandbox/filesystem/watch.rs` with the test module first. The mock emits a Connect server-stream (`application/connect+json`, 5-byte envelopes) using the SAME `encode_envelope` + `FLAG_END_STREAM` helpers the existing `connect::client` server-stream tests use (`crates/e2b-rs/src/connect/client.rs` ~line 347). The `WatchDirResponse` oneof serializes (proto3-JSON via pbjson) with the variant's proto field name as the key: `{"start":{}}`, `{"filesystem":{...}}`, `{"keepalive":{}}`; `EventType` serializes as its proto name string (e.g. `EVENT_TYPE_CREATE`).
```rust
//! Directory watching (`watch_dir`) over the Connect `WatchDir` server stream.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::envelope::{FLAG_END_STREAM, encode_envelope};
    use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
    use crate::sandbox::filesystem::{Filesystem, FilesystemEventType, WatchOpts};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fs_for(server: &MockServer) -> Filesystem {
        let config = ConnectionConfig::new(ConnectionConfigOpts::default());
        Filesystem::build_with_base_url(server.uri(), "sbx_w", "0.6.4", None, &config)
            .expect("filesystem")
    }

    #[tokio::test]
    async fn watch_yields_mapped_events_then_ends() {
        let server = MockServer::start().await;
        // StartEvent, one FilesystemEvent, a KeepAlive (must be dropped), end-stream.
        let mut body = encode_envelope(0, br#"{"start":{}}"#);
        body.extend(encode_envelope(
            0,
            br#"{"filesystem":{"name":"a.txt","type":"EVENT_TYPE_CREATE"}}"#,
        ));
        body.extend(encode_envelope(0, br#"{"keepalive":{}}"#));
        body.extend(encode_envelope(FLAG_END_STREAM, b"{}"));
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/WatchDir"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/connect+json")
                    .set_body_bytes(body),
            )
            .mount(&server)
            .await;

        let fs = fs_for(&server);
        let mut handle = fs.watch_dir("/d", WatchOpts::default()).await.expect("watch");
        let ev = handle.next().await.expect("event");
        assert_eq!(ev.r#type, FilesystemEventType::Create);
        assert_eq!(ev.name, "a.txt");
        // StartEvent + KeepAlive were not surfaced; stream ended after the event.
        assert!(handle.next().await.is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::filesystem::watch`
Expected: FAIL — `watch_dir`/`WatchHandle` not found.

- [ ] **Step 3: Implement `WatchOpts`, `WatchHandle`, `watch_dir`**

In `crates/e2b-rs/src/sandbox/filesystem/watch.rs` (above the test module):
```rust
use futures::StreamExt as _;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{Filesystem, FilesystemEvent};
use crate::envd::proto::filesystem as pb;
use crate::envd::proto::filesystem::watch_dir_response::Event as WatchEvent;
use crate::envd::versions::{
    ENVD_VERSION_FS_EVENT_ENTRY_INFO, ENVD_VERSION_RECURSIVE_WATCH,
    ENVD_VERSION_WATCH_NETWORK_MOUNTS, version_gte,
};
use crate::errors::{Error, Result};

/// Options for [`Filesystem::watch_dir`].
#[derive(Default)]
pub struct WatchOpts {
    /// Watch subdirectories recursively (requires envd >= 0.1.4).
    pub recursive: bool,
    /// Include the affected entry's info in events (requires envd >= 0.6.3).
    pub include_entry: bool,
    /// Allow watching network-mounted paths (requires envd >= 0.6.4).
    pub allow_network_mounts: bool,
    /// The sandbox user.
    pub user: Option<String>,
}

/// A live directory watch. Receive events with [`WatchHandle::next`]; the watch
/// stops when the handle is dropped or [`WatchHandle::stop`] is called, or when
/// the server closes the stream.
pub struct WatchHandle {
    events: mpsc::Receiver<FilesystemEvent>,
    task: JoinHandle<()>,
}

impl WatchHandle {
    /// Receive the next filesystem event, or `None` when the watch has ended
    /// (server closed the stream, errored, or the watch was stopped).
    pub async fn next(&mut self) -> Option<FilesystemEvent> {
        self.events.recv().await
    }

    /// Borrow the underlying receiver (e.g. to use in `tokio::select!`).
    pub fn events(&mut self) -> &mut mpsc::Receiver<FilesystemEvent> {
        &mut self.events
    }

    /// Stop watching (aborts the background task + underlying request).
    pub fn stop(self) {
        self.task.abort();
        // `self` (incl. the receiver + JoinHandle) drops here; Drop also aborts.
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Filesystem {
    /// Watch a directory for changes. Returns a [`WatchHandle`] whose
    /// [`WatchHandle::next`] yields [`FilesystemEvent`]s. The stream's initial
    /// `StartEvent` handshake is consumed internally; `KeepAlive` frames are
    /// dropped.
    ///
    /// # Errors
    /// Returns [`Error::Template`] if a requested option (recursive /
    /// include_entry / allow_network_mounts) is unsupported by the sandbox's envd.
    pub async fn watch_dir(&self, path: &str, opts: WatchOpts) -> Result<WatchHandle> {
        if opts.recursive && !version_gte(&self.envd_version, ENVD_VERSION_RECURSIVE_WATCH) {
            return Err(Error::Template(format!(
                "Recursive watch requires envd >= {ENVD_VERSION_RECURSIVE_WATCH}"
            )));
        }
        if opts.include_entry && !version_gte(&self.envd_version, ENVD_VERSION_FS_EVENT_ENTRY_INFO) {
            return Err(Error::Template(format!(
                "include_entry requires envd >= {ENVD_VERSION_FS_EVENT_ENTRY_INFO}"
            )));
        }
        if opts.allow_network_mounts
            && !version_gte(&self.envd_version, ENVD_VERSION_WATCH_NETWORK_MOUNTS)
        {
            return Err(Error::Template(format!(
                "allow_network_mounts requires envd >= {ENVD_VERSION_WATCH_NETWORK_MOUNTS}"
            )));
        }

        let user = self.resolve_user(opts.user.as_deref());
        let req = pb::WatchDirRequest {
            path: path.to_string(),
            recursive: opts.recursive,
            include_entry: opts.include_entry,
            allow_network_mounts: opts.allow_network_mounts,
        };
        let stream = self
            .connect
            .server_stream::<_, pb::WatchDirResponse>(
                crate::connect::FS_WATCH_DIR,
                &req,
                user.as_deref(),
            )
            .await?;

        let (tx, rx) = mpsc::channel(64);
        let task = tokio::spawn(async move {
            futures::pin_mut!(stream);
            while let Some(item) = stream.next().await {
                let Ok(resp) = item else { break }; // stream error ends the watch
                match resp.event {
                    Some(WatchEvent::Filesystem(ev)) => {
                        if let Some(mapped) = FilesystemEvent::from_proto(ev)
                            && tx.send(mapped).await.is_err()
                        {
                            break; // receiver dropped
                        }
                    }
                    // StartEvent + KeepAlive are not surfaced to the consumer.
                    _ => {}
                }
            }
        });

        Ok(WatchHandle { events: rx, task })
    }
}
```
NOTE on the StartEvent handshake: the JS SDK waits for the first `StartEvent` before returning the handle (so connection errors surface eagerly). The implementation above returns as soon as `server_stream` yields the response (the HTTP response headers have arrived). If eager StartEvent confirmation is desired, peek the first stream item inside `watch_dir` before spawning (match `Some(Ok(resp))` with `event == Start`) and propagate an error otherwise; the test's framing (StartEvent first) supports either approach. Keep whichever is simpler but document the choice.

- [ ] **Step 4: Run the test**

Run: `cargo test -p e2b-rs sandbox::filesystem::watch` → pass.

- [ ] **Step 5: Re-export + verify & commit**

Add `pub(crate) mod watch;` to `mod.rs` and `pub use watch::{WatchHandle, WatchOpts};`; lift `WatchHandle`/`WatchOpts` through `sandbox/mod.rs` → `lib.rs`. Run `cargo clippy --workspace --all-targets -- -D warnings` → clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/filesystem/watch.rs crates/e2b-rs/src/sandbox/filesystem/mod.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(fs): add watch_dir streaming into a tokio mpsc WatchHandle" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Parity checklist, quickstart & full gate

**Files:**
- Modify: `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc), `README.md`

- [ ] **Step 1: Crate quickstart**

In `crates/e2b-rs/src/lib.rs`'s `//!` docs, add a `## Files` `no_run` example:
```rust
//! ## Files
//!
//! ```no_run
//! # async fn run() -> e2b_rs::Result<()> {
//! use e2b_rs::Sandbox;
//! let sandbox = Sandbox::create().template("base").await?;
//! sandbox.files().write("/tmp/hello.txt", b"hi".to_vec(), Default::default()).await?;
//! let text = sandbox.files().read("/tmp/hello.txt", None).await?;
//! assert_eq!(text, "hi");
//! let mut watch = sandbox.files().watch_dir("/tmp", Default::default()).await?;
//! while let Some(event) = watch.next().await {
//!     println!("{:?} {}", event.r#type, event.name);
//! }
//! # Ok(())
//! # }
//! ```
```

- [ ] **Step 2: Parity checklist**

In `docs/parity-checklist.md`, add a `## Sandbox filesystem (Plan 3b)` section:
```markdown
## Sandbox filesystem (Plan 3b)

| JS (`sandbox.files.*`) | Rust (`sandbox.files().*`) | Status |
|---|---|---|
| `read` (text/bytes/stream) | `read` / `read_bytes` / `read_stream` | ✅ |
| `write` (single, octet/multipart, gzip, metadata) | `write` | ✅ |
| `write` (multi-file) | `write_files` | ✅ |
| `list` | `list` (depth) | ✅ |
| `exists` / `getInfo` | `exists` / `get_info` | ✅ |
| `makeDir` / `remove` / `rename` | `make_dir` / `remove` / `rename` | ✅ |
| `watchDir` (+ WatchHandle) | `watch_dir` → `WatchHandle` (mpsc) | ✅ |
```

- [ ] **Step 3: README** — add a short files snippet under the usage section. Only stage `README.md` if it changed.

- [ ] **Step 4: Full release gate** — run each; all must pass:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` (report counts; 0 failures)
- `cargo test --doc -p e2b-rs` (the new `Files` doctest compiles under `no_run`)
- `cargo doc --no-deps -p e2b-rs` (fix any broken `[Type]` intra-doc link → `crate::Type`)
- `cargo xtask codegen && git status --porcelain` → empty (codegen idempotent; do NOT commit regenerated files)

- [ ] **Step 5: Commit**
```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(fs): document filesystem quickstart and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 3b is complete when:
- `Sandbox::files()` exposes a `Filesystem` with `read`/`read_bytes`/`read_stream`, `write`/`write_files` (octet-stream + multipart + gzip + metadata, version-gated), `list`/`get_info`/`exists`/`make_dir`/`remove`/`rename`, and `watch_dir` → a `tokio::sync::mpsc`-backed `WatchHandle`.
- All public filesystem types (`EntryInfo`, `FileType`, `WriteInfo`, `WriteEntry`, `FilesystemEvent`, `FilesystemEventType`, `FsWriteOpts`, `WatchOpts`, `WatchHandle`) are re-exported at the crate root; NO generated `envd::proto`/`rest_gen` type leaks into the public API.
- `Code::AlreadyExists` → `Error::Conflict`; filesystem `NotFound` → `Error::FileNotFound`; version gates raise `Error::Template`.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects the filesystem.

**Carry-forwards (out of scope, documented):** the lazy `stream` read format's idle-timeout watchdog (JS resets a timer per chunk) is not ported — `read_stream` relies on the request timeout; streaming *upload* bodies (`write_stream` from an `AsyncRead`/`Stream` source) may be deferred if not landed in Task 5 (buffered `write` covers the common case); the eager `StartEvent` handshake confirmation is optional (see Task 6 note); `signature`-query file access is already provided by `Sandbox::upload_url`/`download_url` (3a-extras).

**Next:** Plan 3c (Commands & Pty) — `sandbox.commands` (run/handle, streaming stdout/stderr via `tokio::sync::mpsc`) and `sandbox.pty` on the same `ConnectClient`, reusing the Filesystem's envd-client construction pattern.
