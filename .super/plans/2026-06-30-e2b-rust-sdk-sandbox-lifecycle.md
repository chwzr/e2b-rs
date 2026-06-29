# E2B Rust SDK — Sandbox Lifecycle (Plan 3a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the public `Sandbox` type and its control-plane lifecycle — `create`, `connect`, `kill`, `get_info`, `set_timeout`, `is_running`, `get_host`, and paginated `list` — wrapping the generated control-plane types into a hand-written public API.

**Architecture:** `Sandbox::create()` / `Sandbox::connect()` return option-builders that `impl IntoFuture`, so `Sandbox::create().template("x").timeout(d).await?` reads naturally (spec D7). The builders construct a `ConnectionConfig` + `ApiClient` and POST to the control plane. The wire types (`api::schema::{NewSandbox, SandboxDetail, ...}`) stay internal; the SDK exposes hand-written public types (`SandboxInfo`, `SandboxState`, …) constructed from them (spec §1 non-goal: don't expose raw generated types). This plan covers the control-plane half; the envd I/O sub-objects (`files`/`commands`/`pty`) are wired by Plans 3b/3c.

**Tech Stack:** `reqwest`/`serde_json` (via the existing `ApiClient`), `tokio` (`IntoFuture`), `time`-free (use `std::time::Duration` + `chrono` from the generated types); dev: `wiremock`.

**Reference spec:** `.super/specs/2026-06-28-e2b-rust-sdk-design.md` §2 (Sandbox surface), §6 (ergonomics), §9. **JS source:** `../E2B/packages/js-sdk/src/sandbox/{index.ts,sandboxApi.ts}`, `paginator.ts`.

## Milestone roadmap (context — Plans 1, 2a, 2b-i, 2b-ii merged to `main`)

| Plan | Deliverable |
|---|---|
| 1, 2a, 2b-i, 2b-ii | DONE, merged (foundation, codegen, REST + Connect transports) |
| **3a — Sandbox lifecycle (this plan)** | public `Sandbox` + create/connect/kill/get_info/set_timeout/is_running/get_host/list; wiremock-tested |
| 3a-extras | pause/resume/get_metrics/snapshots/update_network/MCP/signed-URLs |
| 3b — Filesystem · 3c — Commands & Pty | envd I/O on `ConnectClient` + `EnvdApiClient` |
| 4 — Git & Volume · 5 — Template & Polish | … |

## Global Constraints

- **Repo/workspace:** package `e2b-rs`, lib `e2b_rs`, crates under `crates/`. Edition 2024, MSRV 1.95.0.
- **Lints (panic-free lib):** `clippy::unwrap_used`/`expect_used`/`missing_docs` denied; allowed in tests. No `.unwrap()`/`.expect()`/`panic!` in non-test code. Use the codebase idioms (let-chains, `?`, `.map_err`).
- **Public API:** this milestone introduces the SDK's FIRST public types beyond foundation. They must be `pub`, fully `///`-documented with `no_run` doctests where they touch the network. Do NOT expose the generated `api::schema` types publicly — convert them into hand-written public types. Import generated types under qualified aliases (`use crate::api::schema as api_schema;`).
- **Builders finish via `IntoFuture`** (spec D7): `Sandbox::create()....await?` and `Sandbox::connect(id)....await?`. No `.send()` finisher.
- **Async-only** on `tokio`. Timeouts are `std::time::Duration` at the public API; the wire uses seconds (`timeout_to_seconds`).
- **Parity:** match `sandboxApi.ts` endpoints/bodies and `index.ts` behavior exactly. Sandbox create defaults: template `"base"`, timeout 300_000ms (`DEFAULT_SANDBOX_TIMEOUT_MS`). `get_host` / URL construction reuses `ConnectionConfig` (Plan 1). `list` is cursor-paginated via the `x-next-token` header (reuse `PaginationState`).
- **Commits:** conventional messages ending with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Run `cargo fmt --all` before every commit.

### Wire facts (confirmed in `api::schema` + `sandboxApi.ts`)

- `POST /sandboxes` body = `NewSandbox { template_id (req), timeout (i32 seconds, default 15), metadata?, env_vars?, secure?, allow_internet_access?, auto_pause, network?, ... }` → `201` `SandboxDetail`.
- `SandboxDetail { sandbox_id, domain?, envd_version (newtype), envd_access_token?, state, template_id, started_at, end_at, cpu_count (newtype), memory_mb (newtype), metadata?, alias?, allow_internet_access?, ... }`. The newtypes (`CpuCount`/`MemoryMb`/`EnvdVersion`) wrap `i64`/`String` — access via their `.0` / `Deref`.
- `POST /sandboxes/{id}/connect` body `{ timeout: <seconds> }` → `SandboxDetail` (resumes a paused sandbox).
- `DELETE /sandboxes/{id}` → `204` (kill); `404` → not found (return `false`).
- `GET /sandboxes/{id}` → `SandboxDetail` (info).
- `POST /sandboxes/{id}/timeout` body `{ timeout: <seconds> }` → `204`.
- `GET /v2/sandboxes?state=running&state=paused&limit=&nextToken=&metadata=` → `200` `Vec<ListedSandbox>` + `x-next-token` response header.
- `get_host(port)` = `{port}-{sandboxId}.{sandboxDomain}` (via `ConnectionConfig::get_host`); `is_running` hits envd `/health` (Plan 3b) — in 3a, implement `is_running` against the control-plane `get_info` state instead (note the deviation; 3b can refine to the envd health check).

### File structure (this plan)

```
crates/e2b-rs/src/sandbox/
├── mod.rs            # MODIFY: pub mod for the new modules + re-exports
├── types.rs          # public lifecycle types (SandboxInfo, SandboxState, SandboxMetrics) + From<SandboxDetail>
├── opts.rs           # SandboxCreateOpts / SandboxConnectOpts / SandboxListOpts (+ ConnectionConfigOpts passthrough)
├── api.rs            # internal control-plane calls (create/connect/kill/info/set_timeout/list) on ApiClient
├── sandbox.rs        # the public `Sandbox` struct + create()/connect() IntoFuture builders + instance methods
└── paginator.rs      # SandboxPaginator (list, x-next-token)
```

---

### Task 1: Public lifecycle types (`sandbox/types.rs`)

Hand-written public types + the conversion from the generated `SandboxDetail`.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/types.rs`
- Modify: `crates/e2b-rs/src/sandbox/mod.rs` (`pub mod types;` + re-exports)

**Interfaces:**
- Consumes: `crate::api::schema as api_schema`.
- Produces:
  - `pub enum SandboxState { Running, Paused }` with `from_schema(api_schema::SandboxState) -> SandboxState`.
  - `pub struct SandboxInfo { pub sandbox_id: String, pub template_id: String, pub name: Option<String>, pub metadata: BTreeMap<String, String>, pub started_at: chrono::DateTime<chrono::Utc>, pub end_at: chrono::DateTime<chrono::Utc>, pub state: SandboxState, pub cpu_count: u32, pub memory_mb: u32, pub envd_version: String, pub allow_internet_access: Option<bool>, pub sandbox_domain: Option<String> }` with `pub(crate) fn from_detail(d: api_schema::SandboxDetail) -> SandboxInfo`.

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/sandbox/types.rs`:

```rust
//! Public sandbox lifecycle types.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_info_converts_from_detail_json() {
        // A representative POST /sandboxes / GET /sandboxes/{id} response.
        let json = r#"{
            "sandboxID": "sbx_123",
            "templateID": "base",
            "clientID": "c1",
            "cpuCount": 2,
            "memoryMB": 1024,
            "diskSizeMB": 1024,
            "envdVersion": "0.6.0",
            "state": "running",
            "startedAt": "2026-06-30T10:00:00Z",
            "endAt": "2026-06-30T10:05:00Z",
            "metadata": {"k": "v"},
            "domain": "e2b.app"
        }"#;
        let detail: crate::api::schema::SandboxDetail =
            serde_json::from_str(json).expect("deserialize SandboxDetail");
        let info = SandboxInfo::from_detail(detail);
        assert_eq!(info.sandbox_id, "sbx_123");
        assert_eq!(info.template_id, "base");
        assert_eq!(info.cpu_count, 2);
        assert_eq!(info.memory_mb, 1024);
        assert_eq!(info.envd_version, "0.6.0");
        assert!(matches!(info.state, SandboxState::Running));
        assert_eq!(info.metadata.get("k").map(String::as_str), Some("v"));
        assert_eq!(info.sandbox_domain.as_deref(), Some("e2b.app"));
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `pub mod types;` and `pub use types::{SandboxInfo, SandboxState};`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::types`
Expected: FAIL — `SandboxInfo`/`SandboxState` not found, and the `cpu_count`/`memory_mb` newtype access is undefined.

- [ ] **Step 3: Implement**

The exact generated shapes (verified in `schema.rs`) are: `SandboxState` = enum `Running`/`Paused` (serde "running"/"paused"); `CpuCount(pub NonZeroU32)` → inner via `.0.get()`; `MemoryMb(pub i32)` → `.0`; `EnvdVersion(pub String)` → `.0`; `SandboxMetadata(pub HashMap<String,String>)` → `.0` (convert `HashMap`→`BTreeMap` with `.into_iter().collect()`). The code below uses these exactly.

Insert above the test module:

```rust
use std::collections::BTreeMap;

use crate::api::schema as api_schema;

/// Whether a sandbox is currently running or paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// The sandbox is running.
    Running,
    /// The sandbox is paused (snapshotted).
    Paused,
}

impl SandboxState {
    fn from_schema(state: &api_schema::SandboxState) -> SandboxState {
        // The generated enum serializes as "running"/"paused"; match its variants.
        match state {
            api_schema::SandboxState::Running => SandboxState::Running,
            api_schema::SandboxState::Paused => SandboxState::Paused,
        }
    }
}

/// Metadata and runtime details about a sandbox.
#[derive(Debug, Clone)]
pub struct SandboxInfo {
    /// Sandbox identifier.
    pub sandbox_id: String,
    /// Template the sandbox was created from.
    pub template_id: String,
    /// Optional template alias/name.
    pub name: Option<String>,
    /// User-provided metadata.
    pub metadata: BTreeMap<String, String>,
    /// When the sandbox started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the sandbox will expire.
    pub end_at: chrono::DateTime<chrono::Utc>,
    /// Running or paused.
    pub state: SandboxState,
    /// vCPU count.
    pub cpu_count: u32,
    /// Memory in MB.
    pub memory_mb: u32,
    /// envd version.
    pub envd_version: String,
    /// Whether internet access was explicitly set.
    pub allow_internet_access: Option<bool>,
    /// Base domain serving this sandbox's traffic.
    pub sandbox_domain: Option<String>,
}

impl SandboxInfo {
    /// Build a public [`SandboxInfo`] from the generated control-plane detail.
    pub(crate) fn from_detail(d: api_schema::SandboxDetail) -> SandboxInfo {
        SandboxInfo {
            sandbox_id: d.sandbox_id,
            template_id: d.template_id,
            name: d.alias,
            metadata: d.metadata.map(|m| m.0.into_iter().collect()).unwrap_or_default(),
            started_at: d.started_at,
            end_at: d.end_at,
            state: SandboxState::from_schema(&d.state),
            cpu_count: d.cpu_count.0.get(),
            memory_mb: u32::try_from(d.memory_mb.0).unwrap_or(0),
            envd_version: d.envd_version.0,
            allow_internet_access: d.allow_internet_access,
            sandbox_domain: d.domain,
        }
    }
}
```

- [ ] **Step 4: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::types` → PASS. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/types.rs crates/e2b-rs/src/sandbox/mod.rs
git commit -m "feat(sandbox): add public SandboxInfo/SandboxState types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Create/connect options (`sandbox/opts.rs`)

The option structs the builders fill. They bundle the create-specific params with a `ConnectionConfigOpts` (so a standalone `Sandbox::create()` configures its own connection).

**Files:**
- Create: `crates/e2b-rs/src/sandbox/opts.rs`
- Modify: `crates/e2b-rs/src/sandbox/mod.rs`

**Interfaces:**
- Consumes: `crate::connection_config::ConnectionConfigOpts`.
- Produces:
  - `pub struct SandboxCreateOpts { pub template: Option<String>, pub timeout: Option<std::time::Duration>, pub metadata: BTreeMap<String,String>, pub envs: BTreeMap<String,String>, pub secure: Option<bool>, pub allow_internet_access: Option<bool>, pub connection: ConnectionConfigOpts }` (`#[derive(Default)]`).
  - `pub struct SandboxConnectOpts { pub timeout: Option<std::time::Duration>, pub connection: ConnectionConfigOpts }` (`Default`).
  - `pub struct SandboxListOpts { pub states: Option<Vec<SandboxState>>, pub metadata: BTreeMap<String,String>, pub limit: Option<u32>, pub connection: ConnectionConfigOpts }` (`Default`).

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/sandbox/opts.rs`:

```rust
//! Options for the sandbox lifecycle builders.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opts_default_is_empty() {
        let o = SandboxCreateOpts::default();
        assert!(o.template.is_none());
        assert!(o.timeout.is_none());
        assert!(o.metadata.is_empty());
        let l = SandboxListOpts::default();
        assert!(l.states.is_none());
        assert!(l.limit.is_none());
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `pub mod opts;` and `pub use opts::{SandboxCreateOpts, SandboxConnectOpts, SandboxListOpts};`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::opts`
Expected: FAIL — types not found.

- [ ] **Step 3: Implement**

Insert above the test module:

```rust
use std::collections::BTreeMap;
use std::time::Duration;

use crate::connection_config::ConnectionConfigOpts;
use crate::sandbox::types::SandboxState;

/// Options for [`Sandbox::create`](crate::Sandbox::create).
#[derive(Default)]
pub struct SandboxCreateOpts {
    /// Template id or alias (default `"base"`).
    pub template: Option<String>,
    /// Sandbox lifetime (default 5 minutes).
    pub timeout: Option<Duration>,
    /// Metadata key/values.
    pub metadata: BTreeMap<String, String>,
    /// Environment variables.
    pub envs: BTreeMap<String, String>,
    /// Secure all envd communication (default true).
    pub secure: Option<bool>,
    /// Allow internet access (default true).
    pub allow_internet_access: Option<bool>,
    /// Connection options (api key, domain, debug, ...).
    pub connection: ConnectionConfigOpts,
}

/// Options for [`Sandbox::connect`](crate::Sandbox::connect).
#[derive(Default)]
pub struct SandboxConnectOpts {
    /// Lifetime to set on (re)connect (default 5 minutes).
    pub timeout: Option<Duration>,
    /// Connection options.
    pub connection: ConnectionConfigOpts,
}

/// Options for [`Sandbox::list`](crate::Sandbox::list).
#[derive(Default)]
pub struct SandboxListOpts {
    /// Filter by state (default both running and paused).
    pub states: Option<Vec<SandboxState>>,
    /// Filter by metadata key/values.
    pub metadata: BTreeMap<String, String>,
    /// Page size (default 100).
    pub limit: Option<u32>,
    /// Connection options.
    pub connection: ConnectionConfigOpts,
}
```

- [ ] **Step 4: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::opts` → PASS. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/opts.rs crates/e2b-rs/src/sandbox/mod.rs
git commit -m "feat(sandbox): add create/connect/list option structs" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Control-plane lifecycle calls (`sandbox/api.rs`)

Internal functions that call `ApiClient` for the sandbox endpoints. These return the generated `SandboxDetail`/raw results; the public `Sandbox`/`SandboxInfo` wrapping happens in Task 4.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/api.rs`
- Modify: `crates/e2b-rs/src/sandbox/mod.rs` (`pub(crate) mod api;`)

**Interfaces:**
- Consumes: `crate::api::client::ApiClient`, `crate::api::schema as api_schema`, `crate::errors::{Error, Result}`, `crate::utils::timeout_to_seconds`, `SandboxCreateOpts`, `SandboxConnectOpts`.
- Produces:
  - `pub(crate) async fn create_sandbox(api: &ApiClient, opts: &SandboxCreateOpts) -> Result<api_schema::SandboxDetail>` — `POST /sandboxes` with the opts mapped to `NewSandbox`.
  - `pub(crate) async fn connect_sandbox(api: &ApiClient, sandbox_id: &str, timeout: std::time::Duration) -> Result<api_schema::SandboxDetail>` — `POST /sandboxes/{id}/connect`.
  - `pub(crate) async fn kill_sandbox(api: &ApiClient, sandbox_id: &str) -> Result<bool>` — `DELETE`; `false` on 404.
  - `pub(crate) async fn get_sandbox_info(api: &ApiClient, sandbox_id: &str) -> Result<api_schema::SandboxDetail>` — `GET`.
  - `pub(crate) async fn set_sandbox_timeout(api: &ApiClient, sandbox_id: &str, timeout: std::time::Duration) -> Result<()>` — `POST /timeout`.

- [ ] **Step 1: Write the failing tests**

Create `crates/e2b-rs/src/sandbox/api.rs`:

```rust
//! Control-plane sandbox lifecycle calls.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
    use crate::sandbox::opts::SandboxCreateOpts;
    use std::time::Duration;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn api_for(server: &MockServer) -> ApiClient {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            api_url: Some(server.uri()),
            ..Default::default()
        });
        ApiClient::new(&config, true).expect("api client")
    }

    fn detail_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "cpuCount": 2, "memoryMB": 1024, "diskSizeMB": 1024,
            "envdVersion": "0.6.0", "state": "running",
            "startedAt": "2026-06-30T10:00:00Z", "endAt": "2026-06-30T10:05:00Z"
        })
    }

    #[tokio::test]
    async fn create_posts_new_sandbox_with_template_and_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes"))
            // NewSandbox uses templateID + timeout (seconds).
            .and(body_partial_json(serde_json::json!({"templateID": "base", "timeout": 300})))
            .respond_with(ResponseTemplate::new(201).set_body_json(detail_json("sbx_new")))
            .mount(&server)
            .await;
        let api = api_for(&server);
        let opts = SandboxCreateOpts { timeout: Some(Duration::from_secs(300)), ..Default::default() };
        let detail = create_sandbox(&api, &opts).await.expect("create");
        assert_eq!(detail.sandbox_id, "sbx_new");
    }

    #[tokio::test]
    async fn kill_returns_false_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/sandboxes/sbx_gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(!kill_sandbox(&api_for(&server), "sbx_gone").await.expect("kill"));
    }

    #[tokio::test]
    async fn kill_returns_true_on_204() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/sandboxes/sbx_ok"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(kill_sandbox(&api_for(&server), "sbx_ok").await.expect("kill"));
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `pub(crate) mod api;`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::api`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement**

Insert above the test module. NOTE: `ApiClient::request_unit`/`request` map non-2xx to `Error` already, so `kill`/`get_info` need to special-case 404 — but the current `ApiClient` does not expose the status. Add a small helper that calls `request_unit` and maps `Error::NotFound` back to "not found" for `kill` (the control-plane DELETE returns 404 → `ApiClient` maps it to `Error::NotFound`; we translate that to `Ok(false)`). For non-404 errors, propagate.

```rust
use crate::api::client::ApiClient;
use crate::api::schema as api_schema;
use crate::errors::{Error, Result};
use crate::sandbox::opts::SandboxCreateOpts;
use crate::utils::timeout_to_seconds;
use std::time::Duration;

/// Default sandbox lifetime (5 minutes), matching `DEFAULT_SANDBOX_TIMEOUT_MS`.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(crate::connection_config::DEFAULT_SANDBOX_TIMEOUT_MS);

/// Map create options to the generated `NewSandbox` body and POST it.
pub(crate) async fn create_sandbox(
    api: &ApiClient,
    opts: &SandboxCreateOpts,
) -> Result<api_schema::SandboxDetail> {
    let timeout = opts.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let timeout_secs = timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX));
    let template = opts.template.clone().unwrap_or_else(|| "base".to_string());

    // Build the request JSON by NewSandbox field names (camelCase on the wire).
    let mut body = serde_json::json!({
        "templateID": template,
        "timeout": timeout_secs,
    });
    if !opts.metadata.is_empty() {
        body["metadata"] = serde_json::to_value(&opts.metadata).unwrap_or_default();
    }
    if !opts.envs.is_empty() {
        body["envVars"] = serde_json::to_value(&opts.envs).unwrap_or_default();
    }
    if let Some(secure) = opts.secure {
        body["secure"] = serde_json::Value::Bool(secure);
    }
    if let Some(allow) = opts.allow_internet_access {
        body["allowInternetAccess"] = serde_json::Value::Bool(allow);
    }

    api.request(reqwest::Method::POST, "/sandboxes", &[], Some(&body)).await
}

/// Resume/connect to a sandbox.
pub(crate) async fn connect_sandbox(
    api: &ApiClient,
    sandbox_id: &str,
    timeout: Duration,
) -> Result<api_schema::SandboxDetail> {
    let body = serde_json::json!({ "timeout": timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)) });
    let path = format!("/sandboxes/{sandbox_id}/connect");
    api.request(reqwest::Method::POST, &path, &[], Some(&body)).await
}

/// Kill a sandbox. Returns `false` if it was not found (already gone).
pub(crate) async fn kill_sandbox(api: &ApiClient, sandbox_id: &str) -> Result<bool> {
    let path = format!("/sandboxes/{sandbox_id}");
    match api.request_unit(reqwest::Method::DELETE, &path, &[], None).await {
        Ok(()) => Ok(true),
        Err(Error::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Fetch sandbox info.
pub(crate) async fn get_sandbox_info(
    api: &ApiClient,
    sandbox_id: &str,
) -> Result<api_schema::SandboxDetail> {
    let path = format!("/sandboxes/{sandbox_id}");
    api.request(reqwest::Method::GET, &path, &[], None).await
}

/// Set the sandbox timeout (from now).
pub(crate) async fn set_sandbox_timeout(
    api: &ApiClient,
    sandbox_id: &str,
    timeout: Duration,
) -> Result<()> {
    let body = serde_json::json!({ "timeout": timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)) });
    let path = format!("/sandboxes/{sandbox_id}/timeout");
    api.request_unit(reqwest::Method::POST, &path, &[], Some(&body)).await
}
```

- [ ] **Step 4: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::api` → PASS (3 tests). `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/api.rs crates/e2b-rs/src/sandbox/mod.rs
git commit -m "feat(sandbox): add control-plane create/connect/kill/info/timeout calls" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: The public `Sandbox` struct + create/connect builders + instance methods (`sandbox/sandbox.rs`)

The headline type. `Sandbox::create()`/`connect()` return `IntoFuture` builders; instance methods wrap the control-plane calls.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/sandbox.rs`
- Modify: `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs` (re-export `Sandbox`)

**Interfaces:**
- Consumes: `ApiClient`, `ConnectionConfig`, `sandbox::api::*`, `SandboxInfo`, `SandboxCreateOpts`, `SandboxConnectOpts`.
- Produces:
  - `pub struct Sandbox { sandbox_id, sandbox_domain: Option<String>, envd_version, envd_access_token: Option<String>, config: ConnectionConfig, api: ApiClient }` (fields `pub(crate)` except accessors).
  - `pub fn Sandbox::create() -> SandboxCreateBuilder`; `pub fn Sandbox::connect(sandbox_id: impl Into<String>) -> SandboxConnectBuilder`. Both builders `impl IntoFuture<Output = Result<Sandbox>>` and have chainable setters (`template`, `timeout`, `metadata`, `envs`, `api_key`, `domain`, `debug`).
  - Instance: `pub fn sandbox_id(&self) -> &str`, `pub fn get_host(&self, port: u16) -> String`, `pub async fn kill(&self) -> Result<bool>`, `pub async fn get_info(&self) -> Result<SandboxInfo>`, `pub async fn set_timeout(&self, timeout: Duration) -> Result<()>`, `pub async fn is_running(&self) -> Result<bool>`.

- [ ] **Step 1: Write the failing tests**

Create `crates/e2b-rs/src/sandbox/sandbox.rs`:

```rust
//! The public `Sandbox` type and its control-plane lifecycle.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn detail_json(id: &str, state: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "cpuCount": 2, "memoryMB": 1024, "diskSizeMB": 1024,
            "envdVersion": "0.6.0", "state": state, "domain": "e2b.app",
            "startedAt": "2026-06-30T10:00:00Z", "endAt": "2026-06-30T10:05:00Z"
        })
    }

    #[tokio::test]
    async fn create_builds_a_sandbox_and_awaits_directly() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/sandboxes"))
            .respond_with(ResponseTemplate::new(201).set_body_json(detail_json("sbx_c", "running")))
            .mount(&server).await;
        // Builder + direct `.await` (IntoFuture).
        let sandbox = Sandbox::create()
            .template("base")
            .timeout(Duration::from_secs(60))
            .api_key("e2b_0123456789abcdef")
            .domain("e2b.app")
            .api_url(server.uri())
            .await
            .expect("create");
        assert_eq!(sandbox.sandbox_id(), "sbx_c");
        assert_eq!(sandbox.get_host(3000), "3000-sbx_c.e2b.app");
    }

    #[tokio::test]
    async fn get_info_and_kill_round_trip() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/sandboxes"))
            .respond_with(ResponseTemplate::new(201).set_body_json(detail_json("sbx_i", "running")))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/sandboxes/sbx_i"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_json("sbx_i", "running")))
            .mount(&server).await;
        Mock::given(method("DELETE")).and(path("/sandboxes/sbx_i"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server).await;

        let sandbox = Sandbox::create().api_key("e2b_0123456789abcdef").api_url(server.uri()).await.expect("create");
        let info = sandbox.get_info().await.expect("info");
        assert_eq!(info.sandbox_id, "sbx_i");
        assert!(sandbox.is_running().await.expect("running")); // state == running
        assert!(sandbox.kill().await.expect("kill"));
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `mod sandbox;` and `pub use sandbox::Sandbox;`. Add to `crates/e2b-rs/src/lib.rs`: `pub use sandbox::{Sandbox, SandboxInfo, SandboxState};` (lifting the public sandbox types to the crate root so `e2b_rs::SandboxState` resolves).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::sandbox`
Expected: FAIL — `Sandbox` / the builders not found.

- [ ] **Step 3: Implement**

Insert above the test module:

```rust
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::time::Duration;

use crate::api::client::ApiClient;
use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts, ENVD_PORT};
use crate::errors::{Error, Result};
use crate::sandbox::api;
use crate::sandbox::opts::{SandboxConnectOpts, SandboxCreateOpts};
use crate::sandbox::types::{SandboxInfo, SandboxState};

/// A boxed future returned by the lifecycle builders.
type SandboxFuture = Pin<Box<dyn Future<Output = Result<Sandbox>> + Send>>;

/// A running or paused E2B sandbox.
///
/// Create one with [`Sandbox::create`] or reconnect with [`Sandbox::connect`].
///
/// # Example
/// ```no_run
/// # async fn run() -> e2b_rs::Result<()> {
/// use e2b_rs::Sandbox;
/// let sandbox = Sandbox::create().template("base").await?;
/// println!("sandbox {} at {}", sandbox.sandbox_id(), sandbox.get_host(3000));
/// sandbox.kill().await?;
/// # Ok(())
/// # }
/// ```
pub struct Sandbox {
    pub(crate) sandbox_id: String,
    pub(crate) sandbox_domain: Option<String>,
    pub(crate) envd_version: String,
    pub(crate) envd_access_token: Option<String>,
    pub(crate) config: ConnectionConfig,
    pub(crate) api: ApiClient,
}

impl Sandbox {
    /// Start configuring a new sandbox. Await the builder to create it.
    pub fn create() -> SandboxCreateBuilder {
        SandboxCreateBuilder { opts: SandboxCreateOpts::default() }
    }

    /// Start configuring a reconnect to an existing (possibly paused) sandbox.
    pub fn connect(sandbox_id: impl Into<String>) -> SandboxConnectBuilder {
        SandboxConnectBuilder { sandbox_id: sandbox_id.into(), opts: SandboxConnectOpts::default() }
    }

    /// The sandbox identifier.
    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }

    /// The external host for a sandbox port, e.g. `3000-<id>.e2b.app`.
    pub fn get_host(&self, port: u16) -> String {
        let domain = self.sandbox_domain.clone().unwrap_or_else(|| self.config.domain.clone());
        self.config.get_host(&self.sandbox_id, port, Some(&domain))
    }

    /// Kill the sandbox. Returns `false` if it was already gone.
    pub async fn kill(&self) -> Result<bool> {
        api::kill_sandbox(&self.api, &self.sandbox_id).await
    }

    /// Fetch current sandbox info.
    pub async fn get_info(&self) -> Result<SandboxInfo> {
        let detail = api::get_sandbox_info(&self.api, &self.sandbox_id).await?;
        Ok(SandboxInfo::from_detail(detail))
    }

    /// Set the sandbox timeout, measured from now.
    pub async fn set_timeout(&self, timeout: Duration) -> Result<()> {
        api::set_sandbox_timeout(&self.api, &self.sandbox_id, timeout).await
    }

    /// Whether the sandbox is currently running (control-plane state).
    ///
    /// Note: Plan 3b refines this to the envd `/health` probe; for now it
    /// reflects the control-plane `state`.
    pub async fn is_running(&self) -> Result<bool> {
        Ok(matches!(self.get_info().await?.state, SandboxState::Running))
    }

    /// Build a `Sandbox` from a control-plane detail + the resolved config/client.
    fn from_detail(detail: crate::api::schema::SandboxDetail, config: ConnectionConfig, api: ApiClient) -> Sandbox {
        Sandbox {
            sandbox_id: detail.sandbox_id,
            sandbox_domain: detail.domain,
            envd_version: detail.envd_version.0,
            envd_access_token: detail.envd_access_token,
            config,
            api,
        }
    }
}

/// Builder for [`Sandbox::create`]; await it to create the sandbox.
pub struct SandboxCreateBuilder {
    opts: SandboxCreateOpts,
}

impl SandboxCreateBuilder {
    /// Template id or alias (default `"base"`).
    pub fn template(mut self, template: impl Into<String>) -> Self {
        self.opts.template = Some(template.into());
        self
    }
    /// Sandbox lifetime (default 5 minutes).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.opts.timeout = Some(timeout);
        self
    }
    /// Add metadata entries.
    pub fn metadata<K: Into<String>, V: Into<String>>(
        mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.opts.metadata.extend(entries.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }
    /// Add environment variables.
    pub fn envs<K: Into<String>, V: Into<String>>(
        mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.opts.envs.extend(entries.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }
    /// Set the API key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.opts.connection.api_key = Some(key.into());
        self
    }
    /// Set the domain.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.opts.connection.domain = Some(domain.into());
        self
    }
    /// Override the API base URL (mainly for tests/self-hosting).
    pub fn api_url(mut self, url: impl Into<String>) -> Self {
        self.opts.connection.api_url = Some(url.into());
        self
    }
    /// Enable debug mode.
    pub fn debug(mut self, debug: bool) -> Self {
        self.opts.connection.debug = Some(debug);
        self
    }
}

impl IntoFuture for SandboxCreateBuilder {
    type Output = Result<Sandbox>;
    type IntoFuture = SandboxFuture;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let config = ConnectionConfig::new(self.opts.connection.clone());
            let api = ApiClient::new(&config, true)?;
            let detail = api::create_sandbox(&api, &self.opts).await?;
            Ok(Sandbox::from_detail(detail, config, api))
        })
    }
}

/// Builder for [`Sandbox::connect`]; await it to (re)connect.
pub struct SandboxConnectBuilder {
    sandbox_id: String,
    opts: SandboxConnectOpts,
}

impl SandboxConnectBuilder {
    /// Lifetime to set on (re)connect (default 5 minutes).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.opts.timeout = Some(timeout);
        self
    }
    /// Set the API key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.opts.connection.api_key = Some(key.into());
        self
    }
    /// Override the API base URL.
    pub fn api_url(mut self, url: impl Into<String>) -> Self {
        self.opts.connection.api_url = Some(url.into());
        self
    }
    /// Set the domain.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.opts.connection.domain = Some(domain.into());
        self
    }
}

impl IntoFuture for SandboxConnectBuilder {
    type Output = Result<Sandbox>;
    type IntoFuture = SandboxFuture;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let config = ConnectionConfig::new(self.opts.connection.clone());
            let api = ApiClient::new(&config, true)?;
            let timeout = self.opts.timeout.unwrap_or(Duration::from_millis(
                crate::connection_config::DEFAULT_SANDBOX_TIMEOUT_MS,
            ));
            let detail = api::connect_sandbox(&api, &self.sandbox_id, timeout).await?;
            Ok(Sandbox::from_detail(detail, config, api))
        })
    }
}

```

(`ConnectionConfigOpts` derives `Clone` from Plan 1, so `self.opts.connection.clone()` is valid; `create`'s `into_future` clones it because it also borrows `&self.opts` for `create_sandbox`. Drop the now-unused `ConnectionConfigOpts` import from the `use` block if clippy flags it.)

- [ ] **Step 4: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::sandbox` → PASS (2 tests). `cargo test --doc -p e2b-rs` (the `Sandbox` doctest compiles, `no_run`). `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox crates/e2b-rs/src/lib.rs
git commit -m "feat(sandbox): add public Sandbox with create/connect builders and lifecycle" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `Sandbox::list()` + `SandboxPaginator` (`sandbox/paginator.rs`)

Cursor-paginated listing over `GET /v2/sandboxes`, reusing the foundation `PaginationState` and the `x-next-token` header. Requires reading the response header, which the current `ApiClient::request` discards — so this task adds a header-returning request variant to `ApiClient`.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/paginator.rs`
- Modify: `crates/e2b-rs/src/api/client.rs` (add `request_with_headers`), `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs` (add `Sandbox::list`)

**Interfaces:**
- Consumes: `ApiClient`, `PaginationState`, `SandboxListOpts`, `SandboxInfo`, `api_schema`.
- Produces:
  - `ApiClient::request_with_headers<T>(...) -> Result<(T, reqwest::header::HeaderMap)>`.
  - `pub struct SandboxPaginator { ... }` with `pub fn has_next(&self) -> bool` and `pub async fn next_items(&mut self) -> Result<Vec<SandboxInfo>>`.
  - `pub fn Sandbox::list(opts: SandboxListOpts) -> SandboxPaginator` (associated fn, NOT requiring an instance).

- [ ] **Step 1: Add `request_with_headers` to `ApiClient`**

In `crates/e2b-rs/src/api/client.rs`, refactor `send` to also return the response headers, and add a public method. Change the private `send` to return `Result<(Vec<u8>, reqwest::header::HeaderMap)>` (capture `resp.headers().clone()` before consuming the body), update `request`/`request_unit` to ignore the headers (`.map(|(b, _)| ...)`), and add:

```rust
    /// Like [`ApiClient::request`] but also returns the response headers (for
    /// pagination cursors).
    pub(crate) async fn request_with_headers<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<(T, reqwest::header::HeaderMap)> {
        let (bytes, headers) = self.send(method, path, query, body).await?;
        let value = serde_json::from_slice::<T>(&bytes)
            .map_err(|e| Error::Internal(format!("failed to decode response from {path}: {e}")))?;
        Ok((value, headers))
    }
```

(Update `send`'s signature + its two existing callers accordingly; keep all existing `api::client` tests green.)

- [ ] **Step 2: Write the failing test**

Create `crates/e2b-rs/src/sandbox/paginator.rs`:

```rust
//! Cursor-paginated sandbox listing.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::opts::SandboxListOpts;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn listed(id: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "cpuCount": 2, "memoryMB": 1024, "diskSizeMB": 1024,
            "envdVersion": "0.6.0", "state": "running",
            "startedAt": "2026-06-30T10:00:00Z", "endAt": "2026-06-30T10:05:00Z"
        })
    }

    #[tokio::test]
    async fn lists_one_page_and_reports_no_next() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/v2/sandboxes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([listed("sbx_a")])))
            .mount(&server).await;
        let opts = SandboxListOpts {
            connection: crate::connection_config::ConnectionConfigOpts {
                api_key: Some("e2b_0123456789abcdef".to_string()),
                api_url: Some(server.uri()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pager = crate::Sandbox::list(opts).expect("pager");
        assert!(pager.has_next()); // true before the first fetch
        let items = pager.next_items().await.expect("page");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].sandbox_id, "sbx_a");
        assert!(!pager.has_next()); // no x-next-token header -> done
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `pub mod paginator;` and `pub use paginator::SandboxPaginator;`.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::paginator`
Expected: FAIL — `SandboxPaginator` / `Sandbox::list` not found.

- [ ] **Step 4: Implement the paginator + `Sandbox::list`**

Insert above the test module in `paginator.rs`:

```rust
use crate::api::client::ApiClient;
use crate::api::schema as api_schema;
use crate::connection_config::ConnectionConfig;
use crate::errors::Result;
use crate::paginator::PaginationState;
use crate::sandbox::opts::SandboxListOpts;
use crate::sandbox::types::{SandboxInfo, SandboxState};

/// A cursor-paginated listing of sandboxes (`GET /v2/sandboxes`).
pub struct SandboxPaginator {
    api: ApiClient,
    state: PaginationState,
    states: Vec<&'static str>,
    metadata: std::collections::BTreeMap<String, String>,
}

impl SandboxPaginator {
    pub(crate) fn new(opts: SandboxListOpts) -> Result<Self> {
        let config = ConnectionConfig::new(opts.connection);
        let api = ApiClient::new(&config, true)?;
        let states = opts
            .states
            .unwrap_or_else(|| vec![SandboxState::Running, SandboxState::Paused])
            .iter()
            .map(|s| match s {
                SandboxState::Running => "running",
                SandboxState::Paused => "paused",
            })
            .collect();
        Ok(Self {
            api,
            state: PaginationState::new(opts.limit, None),
            states,
            metadata: opts.metadata,
        })
    }

    /// Whether more pages remain.
    pub fn has_next(&self) -> bool {
        self.state.has_next()
    }

    /// Fetch the next page. Returns an empty vec (and stops) when exhausted.
    pub async fn next_items(&mut self) -> Result<Vec<SandboxInfo>> {
        if !self.state.has_next() {
            return Ok(Vec::new());
        }
        let mut query: Vec<(&str, String)> = Vec::new();
        // Control-plane arrays are form-style, NOT exploded: `state=running,paused`
        // (don't push repeated `state` pairs — reqwest would explode them).
        query.push(("state", self.states.join(",")));
        if let Some(limit) = self.state.limit() {
            query.push(("limit", limit.to_string()));
        }
        if let Some(token) = self.state.next_token() {
            query.push(("nextToken", token.to_string()));
        }
        if !self.metadata.is_empty() {
            // metadata is a urlencoded `key=value&key2=value2` querystring (JS uses
            // URLSearchParams); reqwest url-encodes the whole value.
            let joined = self
                .metadata
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("&");
            query.push(("metadata", joined));
        }

        let (details, headers): (Vec<api_schema::SandboxDetail>, reqwest::header::HeaderMap) = self
            .api
            .request_with_headers(reqwest::Method::GET, "/v2/sandboxes", &query, None)
            .await?;

        let next = headers
            .get("x-next-token")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        self.state.update_from_token(next);

        Ok(details.into_iter().map(SandboxInfo::from_detail).collect())
    }
}
```

In `crates/e2b-rs/src/sandbox/sandbox.rs`, add to `impl Sandbox`:

```rust
    /// List sandboxes (paginated). Filter by state/metadata via [`SandboxListOpts`].
    pub fn list(opts: crate::sandbox::opts::SandboxListOpts) -> Result<crate::sandbox::paginator::SandboxPaginator> {
        crate::sandbox::paginator::SandboxPaginator::new(opts)
    }
```

(`Sandbox::list` returns `Result<SandboxPaginator>` — constructing the `ApiClient` validates the API key and can fail eagerly. The test unwraps it with `.expect("pager")`. This is a deliberate, documented deviation from JS's infallible `Sandbox.list` (Rust surfaces the key-validation error up front rather than on the first `next_items`).)

- [ ] **Step 5: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::paginator` → PASS. `cargo test -p e2b-rs api::client` → still PASS (the `send` refactor didn't break it). `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox crates/e2b-rs/src/api/client.rs
git commit -m "feat(sandbox): add paginated Sandbox::list with x-next-token cursor" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Parity checklist, crate quickstart & full gate

**Files:**
- Modify: `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc example update), `README.md`

**Interfaces:**
- Consumes: everything in this plan.

- [ ] **Step 1: Update the crate-level quickstart**

In `crates/e2b-rs/src/lib.rs`'s `//!` docs, add a `## Creating a sandbox` section with a `no_run` example:

```rust
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
```

(Ensure `SandboxState` is re-exported from the crate root for the example.)

- [ ] **Step 2: Update the parity checklist**

In `docs/parity-checklist.md`, add:

```markdown
## Sandbox lifecycle (Plan 3a)

| JS (`src/sandbox/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `Sandbox.create` | `Sandbox::create()` (IntoFuture builder) | ✅ |
| `Sandbox.connect` | `Sandbox::connect(id)` (IntoFuture builder) | ✅ |
| `sandbox.kill` / `SandboxApi.kill` | `Sandbox::kill` | ✅ |
| `sandbox.getInfo` | `Sandbox::get_info` → `SandboxInfo` | ✅ |
| `sandbox.setTimeout` | `Sandbox::set_timeout` | ✅ |
| `sandbox.getHost` | `Sandbox::get_host` | ✅ |
| `sandbox.isRunning` | `Sandbox::is_running` (control-plane state; envd `/health` in 3b) | 🔶 |
| `Sandbox.list` / `SandboxPaginator` | `Sandbox::list` + `SandboxPaginator` | ✅ |
| `pause`/`betaPause`/resume/`getMetrics`/snapshots/`updateNetwork`/MCP/signed-URLs | _(Plan 3a-extras)_ | ⬜ |
| `files`/`commands`/`pty` | _(Plans 3b/3c)_ | ⬜ |
```

- [ ] **Step 3: Full release gate**

Run each and confirm it passes:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` (report counts; 0 failures)
- `cargo test --doc -p e2b-rs` (the new Sandbox doctests compile under `no_run`)
- `cargo doc --no-deps -p e2b-rs`
- `cargo xtask codegen && git status --porcelain` → empty

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(sandbox): document lifecycle quickstart and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 3a is complete when:
- `Sandbox::create()`/`connect()` work as `IntoFuture` builders (`Sandbox::create().template("x").await?`), constructing a `Sandbox` from `SandboxDetail`.
- `kill`/`get_info`/`set_timeout`/`get_host`/`is_running` and paginated `list` are implemented and wiremock-tested, with the public `SandboxInfo`/`SandboxState` types (generated types not exposed).
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects the lifecycle.

**Next:** Plan 3a-extras (pause/resume/get_metrics/snapshots/update_network/MCP/signed-URLs) and then Plan 3b (Filesystem) — which constructs the `EnvdApiClient` + `ConnectClient` from the `Sandbox`'s connection details (`sandbox_id`, `sandbox_domain`, `envd_version`, `envd_access_token`) via `ConnectionConfig::get_sandbox_url`, and adds `sandbox.files`. Carry-forwards from 2b-i/2b-ii (array-query comma-join — used here for `metadata`; per-call timeouts; error-message decoration; per-stream `connect-timeout-ms`) apply as those code paths land.
