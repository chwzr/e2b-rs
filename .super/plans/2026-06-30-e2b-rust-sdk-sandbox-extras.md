# Sandbox Control-Plane Extras (Plan 3a-extras) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Round out the public `Sandbox` control-plane surface with pause, metrics, snapshots (create/list/delete), network updates, and signed file URLs — matching the E2B JS SDK 1:1.

**Architecture:** Each operation is a thin async call on the existing `pub(crate)` `ApiClient` (REST/Connect protocol over JSON), added to `crates/e2b-rs/src/sandbox/api.rs`, then exposed as an idiomatic public method on `Sandbox` (`crates/e2b-rs/src/sandbox/sandbox.rs`). Generated `api::schema` wire types stay `pub(crate)` and are wrapped behind hand-written public types (`SandboxMetrics`, `SnapshotInfo`, `SandboxNetworkUpdate`, `SandboxUrlOpts`), exactly as Plan 3a wrapped `SandboxInfo`/`SandboxState`. Snapshot listing reuses the foundation `PaginationState` + `x-next-token` cursor in a new `SnapshotPaginator` mirroring the existing `SandboxPaginator`. Signed URLs are built client-side from the sandbox's `envd_access_token`/`envd_version` via the existing `sandbox::signature::get_signature` — no server call.

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), tokio, reqwest 0.13, serde/serde_json, chrono, semver; wiremock for tests.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; all crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` allowed ONLY in `#[cfg(test)]` (clippy.toml whitelists tests). Prefer `u32/u64/i64::try_from(...).unwrap_or(...)` over `as` casts.
- Streaming (none in this plan) is delivered via `tokio::sync::mpsc`, never callbacks.
- Builders finish via `IntoFuture` (`.await` directly). Instance control-plane methods are plain `async fn`.
- **Do NOT expose generated `api::schema::*` types in any `pub` signature/return/re-export** (spec §1 non-goal). Wrap them in hand-written public types.
- **Honest test fixtures:** mock response bodies must match the REAL wire schema for that endpoint (Plan 3a shipped a bug because a create test mocked `SandboxDetail` when the wire returns the lean `Sandbox`). For this plan: `pause`/`updateNetwork`/`deleteSnapshot` return `204` (no body); `getMetrics` returns `SandboxMetric[]`; `createSnapshot` returns `SnapshotInfo` (201); `listSnapshots` returns `SnapshotInfo[]` + `x-next-token` header.
- Every task: run `cargo fmt --all` before commit. Commit trailer (exact): `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference implementation (source of truth for wire shapes/behavior): `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/sandbox/` and `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/spec/openapi.yml`.

### Pre-verified facts (confirmed against schema.rs + openapi.yml at merge `192fca3`)
- Generated types exist (all `pub(crate)` in `crates/e2b-rs/src/api/schema.rs`):
  - `SandboxPauseRequest { memory: bool }`
  - `SandboxMetric { cpu_count: i32 (rename cpuCount), cpu_used_pct: f32 (cpuUsedPct), disk_total: i64 (diskTotal), disk_used: i64 (diskUsed), mem_cache: i64 (memCache), mem_total: i64 (memTotal), mem_used: i64 (memUsed), timestamp: chrono::DateTime<Utc>, timestamp_unix: i64 (timestampUnix) }`
  - `SnapshotInfo { names: Vec<String>, snapshot_id: String (rename snapshotID) }`
  - `SandboxNetworkUpdateConfig { allow_internet_access: Option<bool>, allow_out: Vec<String> (rename allowOut), deny_out: Vec<String> (denyOut), rules: ... }`
- Endpoints: `POST /sandboxes/{id}/pause` (2365), `GET /sandboxes/{id}/metrics` (2319), `PUT /sandboxes/{id}/network` (2505), `POST /sandboxes/{id}/snapshots` (2566), `GET /snapshots` (2604), `DELETE /templates/{templateID}` (2801).
- Existing reusable code (`pub(crate)` unless noted):
  - `ApiClient::request<T>(method, path, query: &[(&str, String)], body: Option<&serde_json::Value>) -> Result<T>`, `request_unit(...) -> Result<()>`, `request_with_headers<T>(...) -> Result<(T, reqwest::header::HeaderMap)>`.
  - `Error::{NotFound, SandboxNotFound, Template, InvalidArgument, Sandbox}` (errors.rs); `Error::from_status` maps 404→NotFound, 409→Conflict? **CHECK**: verify the 409 mapping in errors.rs before relying on it (see Task 1).
  - `crate::paginator::PaginationState::{new(Option<u32>, Option<String>), has_next()->bool, next_token()->Option<&str>, limit()->Option<u32>, update_from_token(Option<String>)}`.
  - `crate::sandbox::signature::{get_signature(path, op: SignatureOperation, user: Option<&str>, expiration_in_seconds: Option<i64>, envd_access_token: Option<&str>, now_unix: i64) -> Result<Signature>, get_signature_now(...), SignatureOperation::{Read, Write}, Signature { signature: String, expiration: Option<i64> }}`.
  - `ConnectionConfig::{get_sandbox_direct_url(sandbox_id, sandbox_domain, envd_port: u16) -> String, domain: String}`; `crate::connection_config::{ENVD_PORT: u16 = 49983, DEFAULT_USERNAME: &str = "user"}`.
  - `crate::envd::versions::version_gte(actual: &str, required: &str) -> bool`.
  - `Sandbox` fields (`pub(crate)`): `sandbox_id: String`, `sandbox_domain: Option<String>`, `envd_version: String` (currently `#[allow(dead_code)]`), `envd_access_token: Option<String>` (currently `#[allow(dead_code)]`), `config: ConnectionConfig`, `api: ApiClient`.
  - Crate-root re-exports live in `crates/e2b-rs/src/lib.rs` (`pub use sandbox::{...}`); `sandbox/mod.rs` re-exports per-module public types.

---

## File Structure

- `crates/e2b-rs/src/sandbox/api.rs` — MODIFY: add `pause_sandbox`, `get_sandbox_metrics`, `create_snapshot`, `delete_snapshot`, `update_sandbox_network`, `list_snapshots_page` control-plane calls.
- `crates/e2b-rs/src/sandbox/types.rs` — MODIFY: add public `SandboxMetrics` and `SnapshotInfo` output types (+ `from_*` mappers).
- `crates/e2b-rs/src/sandbox/network.rs` — CREATE: public `SandboxNetworkUpdate` + `NetworkRule` input types + `to_wire_body()`.
- `crates/e2b-rs/src/sandbox/opts.rs` — MODIFY: add `SandboxUrlOpts` and `SnapshotListOpts`.
- `crates/e2b-rs/src/sandbox/snapshot_paginator.rs` — CREATE: `SnapshotPaginator` (mirrors `paginator.rs`).
- `crates/e2b-rs/src/sandbox/sandbox.rs` — MODIFY: add public methods `pause`, `get_metrics`, `create_snapshot`, `delete_snapshot`, `list_snapshots`, `update_network`, `upload_url`, `download_url`. Remove the two `#[allow(dead_code)]` attrs on `envd_version`/`envd_access_token` once read.
- `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs` — MODIFY: re-export new public types.
- `docs/parity-checklist.md`, `README.md` — MODIFY (Task 7).

---

### Task 1: `pause` (`POST /sandboxes/{id}/pause`)

Pause a running sandbox. `204` → `true`; `409` (already paused) → `false`; `404` propagates. JS defaults `memory: true` (full memory snapshot → warm resume).

**Files:**
- Modify: `crates/e2b-rs/src/errors.rs`, `crates/e2b-rs/src/sandbox/api.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`

**Interfaces:**
- Consumes: `ApiClient::request_unit`, `Error`.
- Produces:
  - `Error::Conflict(String)` variant + `409 => Error::Conflict(message)` in `from_status` (so 409 is distinguishable; currently 409 falls through to the generic `Error::Sandbox`).
  - `pub(crate) async fn pause_sandbox(api: &ApiClient, sandbox_id: &str, keep_memory: bool) -> Result<bool>`
  - `pub async fn Sandbox::pause(&self) -> Result<bool>` (sends `keep_memory = true`).

> **Pre-verified:** `crates/e2b-rs/src/errors.rs` `from_status` has NO `409` arm — it currently maps to `_ => Error::Sandbox(message)`, which discards the status code. There is NO `Conflict` variant yet. Pause's idempotent 409→false (and a clear error for updateNetwork's 409-on-paused in Task 5) both need 409 to be distinguishable, so add the variant + arm first.

- [ ] **Step 1: Add the `Conflict` variant + 409 mapping**

In `crates/e2b-rs/src/errors.rs`, add a new variant to the `Error` enum (place it near `RateLimit`, with a `///` doc):

```rust
    /// The request conflicts with the resource's current state (HTTP 409),
    /// e.g. pausing an already-paused sandbox or updating a paused sandbox.
    Conflict(String),
```

If the `Error` enum derives `thiserror::Error` with `#[error("...")]` attributes (check the surrounding variants), add a matching display attribute, e.g. `#[error("conflict: {0}")]` above the variant. Then add the arm to `from_status` (above the `_ =>` fallback):

```rust
            409 => Error::Conflict(message),
```

Add a unit test next to the existing `from_status` tests in `errors.rs`:

```rust
        assert!(matches!(Error::from_status(409, "x"), Error::Conflict(_)));
```

Run: `cargo test -p e2b-rs errors` → PASS (incl. the new 409 assertion).

- [ ] **Step 2: Write the failing tests**

In `crates/e2b-rs/src/sandbox/api.rs`, inside `mod tests`, add (reuse the existing `api_for` helper):

```rust
    #[tokio::test]
    async fn pause_returns_true_on_204() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes/sbx_p/pause"))
            .and(body_partial_json(serde_json::json!({ "memory": true })))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(
            pause_sandbox(&api_for(&server), "sbx_p", true)
                .await
                .expect("pause")
        );
    }

    #[tokio::test]
    async fn pause_returns_false_when_already_paused_409() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes/sbx_p/pause"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;
        assert!(
            !pause_sandbox(&api_for(&server), "sbx_p", true)
                .await
                .expect("pause idempotent")
        );
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::api::tests::pause`
Expected: FAIL — `pause_sandbox` not found.

- [ ] **Step 4: Implement `pause_sandbox`**

In `crates/e2b-rs/src/sandbox/api.rs`, add (place after `connect_sandbox`). `Error::Conflict` now exists from Step 1:

```rust
/// Pause a sandbox. `keep_memory` controls whether a full memory snapshot is
/// taken (warm resume) vs filesystem-only. Returns `false` if the sandbox is
/// already paused (HTTP 409, idempotent), matching the JS SDK.
pub(crate) async fn pause_sandbox(
    api: &ApiClient,
    sandbox_id: &str,
    keep_memory: bool,
) -> Result<bool> {
    let body = serde_json::json!({ "memory": keep_memory });
    let path = format!("/sandboxes/{sandbox_id}/pause");
    match api
        .request_unit(reqwest::Method::POST, &path, &[], Some(&body))
        .await
    {
        Ok(()) => Ok(true),
        // 409 = already paused; idempotent, not an error.
        Err(Error::Conflict(_)) => Ok(false),
        Err(e) => Err(e),
    }
}
```

- [ ] **Step 5: Add the public method**

In `crates/e2b-rs/src/sandbox/sandbox.rs`, inside `impl Sandbox` (place after `set_timeout`):

```rust
    /// Pause the sandbox, returning `false` if it was already paused.
    ///
    /// Takes a full memory snapshot so a later [`Sandbox::connect`] warm-boots.
    pub async fn pause(&self) -> Result<bool> {
        api::pause_sandbox(&self.api, &self.sandbox_id, true).await
    }
```

- [ ] **Step 6: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::api::tests::pause` → 2 pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/errors.rs crates/e2b-rs/src/sandbox/api.rs crates/e2b-rs/src/sandbox/sandbox.rs
git commit -m "feat(sandbox): add pause (idempotent 409->false) + Error::Conflict" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `get_metrics` (`GET /sandboxes/{id}/metrics`)

Fetch resource metrics. Wraps generated `SandboxMetric` in a public `SandboxMetrics`. JS hard-gates envd `>= 0.1.5` (else `TemplateError`).

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/types.rs`, `crates/e2b-rs/src/sandbox/api.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `ApiClient::request`, `api_schema::SandboxMetric`, `version_gte`, `Error::Template`.
- Produces:
  - `pub struct SandboxMetrics { pub timestamp, pub cpu_count: u32, pub cpu_used_pct: f64, pub mem_used_bytes: u64, pub mem_total_bytes: u64, pub mem_cache_bytes: u64, pub disk_used_bytes: u64, pub disk_total_bytes: u64 }` + `pub(crate) fn from_metric(api_schema::SandboxMetric) -> SandboxMetrics`.
  - `pub(crate) async fn get_sandbox_metrics(api, id, start: Option<i64>, end: Option<i64>) -> Result<Vec<api_schema::SandboxMetric>>`.
  - `pub async fn Sandbox::get_metrics(&self) -> Result<Vec<SandboxMetrics>>` (envd `>= 0.1.5` gate).

- [ ] **Step 1: Write the failing test (public type mapping)**

In `crates/e2b-rs/src/sandbox/types.rs`, inside the existing `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn metrics_map_from_generated() {
        let raw = crate::api::schema::SandboxMetric {
            cpu_count: 2,
            cpu_used_pct: 12.5,
            disk_total: 1000,
            disk_used: 100,
            mem_cache: 10,
            mem_total: 2048,
            mem_used: 512,
            timestamp: "2026-06-30T10:00:00Z".parse().expect("ts"),
            timestamp_unix: 1_780_000_000,
        };
        let m = SandboxMetrics::from_metric(raw);
        assert_eq!(m.cpu_count, 2);
        assert_eq!(m.mem_used_bytes, 512);
        assert_eq!(m.disk_total_bytes, 1000);
        assert!((m.cpu_used_pct - 12.5).abs() < f64::EPSILON);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::types::tests::metrics_map`
Expected: FAIL — `SandboxMetrics` not found.

- [ ] **Step 3: Implement the public type**

In `crates/e2b-rs/src/sandbox/types.rs` (above the test module), add:

```rust
/// Point-in-time resource usage for a sandbox (see [`Sandbox::get_metrics`]).
#[derive(Debug, Clone)]
pub struct SandboxMetrics {
    /// When the sample was taken.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Number of virtual CPUs.
    pub cpu_count: u32,
    /// CPU usage as a percentage (0–100).
    pub cpu_used_pct: f64,
    /// Memory used, in bytes.
    pub mem_used_bytes: u64,
    /// Total memory, in bytes.
    pub mem_total_bytes: u64,
    /// Page-cache memory, in bytes.
    pub mem_cache_bytes: u64,
    /// Disk used, in bytes.
    pub disk_used_bytes: u64,
    /// Total disk, in bytes.
    pub disk_total_bytes: u64,
}

impl SandboxMetrics {
    /// Map the generated wire metric to the public type (clamping negatives to 0).
    pub(crate) fn from_metric(m: crate::api::schema::SandboxMetric) -> SandboxMetrics {
        SandboxMetrics {
            timestamp: m.timestamp,
            cpu_count: u32::try_from(m.cpu_count).unwrap_or(0),
            cpu_used_pct: f64::from(m.cpu_used_pct),
            mem_used_bytes: u64::try_from(m.mem_used).unwrap_or(0),
            mem_total_bytes: u64::try_from(m.mem_total).unwrap_or(0),
            mem_cache_bytes: u64::try_from(m.mem_cache).unwrap_or(0),
            disk_used_bytes: u64::try_from(m.disk_used).unwrap_or(0),
            disk_total_bytes: u64::try_from(m.disk_total).unwrap_or(0),
        }
    }
}
```

- [ ] **Step 4: Run the mapping test**

Run: `cargo test -p e2b-rs sandbox::types::tests::metrics_map` → PASS.

- [ ] **Step 5: Write the failing API + integration test**

In `crates/e2b-rs/src/sandbox/api.rs` `mod tests`, add:

```rust
    fn metric_json() -> serde_json::Value {
        serde_json::json!({
            "cpuCount": 2, "cpuUsedPct": 12.5,
            "diskTotal": 1000, "diskUsed": 100,
            "memCache": 10, "memTotal": 2048, "memUsed": 512,
            "timestamp": "2026-06-30T10:00:00Z", "timestampUnix": 1780000000
        })
    }

    #[tokio::test]
    async fn get_metrics_returns_samples() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sandboxes/sbx_m/metrics"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([metric_json()])))
            .mount(&server)
            .await;
        let out = get_sandbox_metrics(&api_for(&server), "sbx_m", None, None)
            .await
            .expect("metrics");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].cpu_count, 2);
    }
```

- [ ] **Step 6: Implement `get_sandbox_metrics`**

In `crates/e2b-rs/src/sandbox/api.rs` (after `get_sandbox_info`):

```rust
/// Fetch sandbox resource metrics over an optional `[start, end]` unix-second
/// window. `GET /sandboxes/{id}/metrics` returns `SandboxMetric[]`.
pub(crate) async fn get_sandbox_metrics(
    api: &ApiClient,
    sandbox_id: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> Result<Vec<api_schema::SandboxMetric>> {
    let mut query: Vec<(&str, String)> = Vec::new();
    if let Some(start) = start {
        query.push(("start", start.to_string()));
    }
    if let Some(end) = end {
        query.push(("end", end.to_string()));
    }
    let path = format!("/sandboxes/{sandbox_id}/metrics");
    api.request(reqwest::Method::GET, &path, &query, None).await
}
```

- [ ] **Step 7: Run the API test**

Run: `cargo test -p e2b-rs sandbox::api::tests::get_metrics` → PASS.

- [ ] **Step 8: Add the public method + version gate**

In `crates/e2b-rs/src/sandbox/sandbox.rs`, import `version_gte` at the top (`use crate::envd::versions::version_gte;`). Remove the `#[allow(dead_code)]` on the `envd_version` field (now read). Add to `impl Sandbox`:

```rust
    /// Fetch the sandbox's resource-usage metrics.
    ///
    /// # Errors
    /// Returns [`Error::Template`] if the sandbox's envd is older than `0.1.5`
    /// (metrics are unsupported), matching the JS SDK.
    pub async fn get_metrics(&self) -> Result<Vec<SandboxMetrics>> {
        if !version_gte(&self.envd_version, "0.1.5") {
            return Err(Error::Template(
                "Metrics require a newer template (envd >= 0.1.5); rebuild the template."
                    .to_string(),
            ));
        }
        let raw = api::get_sandbox_metrics(&self.api, &self.sandbox_id, None, None).await?;
        Ok(raw.into_iter().map(SandboxMetrics::from_metric).collect())
    }
```

Ensure `Error` is in scope in `sandbox.rs` (Plan 3a removed the unused `Error` import; re-add `use crate::errors::{Error, Result};` if only `Result` is imported — check the current `use` line and adjust).

- [ ] **Step 9: Re-export `SandboxMetrics`**

In `crates/e2b-rs/src/sandbox/mod.rs`, extend the types re-export: `pub use types::{SandboxInfo, SandboxMetrics, SandboxState};`. In `crates/e2b-rs/src/lib.rs`, add `SandboxMetrics` to the `pub use sandbox::{...}` list (alphabetical).

- [ ] **Step 10: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::` → all pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/types.rs crates/e2b-rs/src/sandbox/api.rs crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(sandbox): add get_metrics with public SandboxMetrics + envd gate" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Snapshot create + delete

`createSnapshot` (instance) → `SnapshotInfo`; `deleteSnapshot` (static, by id) → bool. Note: delete hits `DELETE /templates/{snapshotID}` (a snapshot IS a template), 404 → false.

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/types.rs`, `crates/e2b-rs/src/sandbox/api.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `ApiClient::{request, request_unit}`, `api_schema::SnapshotInfo`, `ConnectionConfig`, `ConnectionConfigOpts`, `Error::NotFound`.
- Produces:
  - `pub struct SnapshotInfo { pub snapshot_id: String, pub names: Vec<String> }` + `pub(crate) fn from_schema(api_schema::SnapshotInfo) -> SnapshotInfo`.
  - `pub(crate) async fn create_snapshot(api, sandbox_id, name: Option<&str>) -> Result<api_schema::SnapshotInfo>`.
  - `pub(crate) async fn delete_snapshot(api, snapshot_id) -> Result<bool>`.
  - `pub async fn Sandbox::create_snapshot(&self, name: Option<String>) -> Result<SnapshotInfo>`.
  - `pub async fn Sandbox::delete_snapshot(snapshot_id: impl Into<String>, connection: ConnectionConfigOpts) -> Result<bool>` (static).

- [ ] **Step 1: Write the failing public-type test**

In `crates/e2b-rs/src/sandbox/types.rs` `mod tests`:

```rust
    #[test]
    fn snapshot_info_maps_from_generated() {
        let raw = crate::api::schema::SnapshotInfo {
            names: vec!["my-snap".to_string()],
            snapshot_id: "snap_1".to_string(),
        };
        let s = SnapshotInfo::from_schema(raw);
        assert_eq!(s.snapshot_id, "snap_1");
        assert_eq!(s.names, vec!["my-snap".to_string()]);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::types::tests::snapshot_info_maps`
Expected: FAIL — `SnapshotInfo` (the public one) not found.

- [ ] **Step 3: Implement public `SnapshotInfo`**

In `crates/e2b-rs/src/sandbox/types.rs` (above the test module):

```rust
/// Metadata for a sandbox snapshot (see [`Sandbox::create_snapshot`]).
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// The snapshot's identifier (also usable as a template id).
    pub snapshot_id: String,
    /// Template names/aliases this snapshot is registered under.
    pub names: Vec<String>,
}

impl SnapshotInfo {
    /// Map the generated wire type to the public one.
    pub(crate) fn from_schema(s: crate::api::schema::SnapshotInfo) -> SnapshotInfo {
        SnapshotInfo {
            snapshot_id: s.snapshot_id,
            names: s.names,
        }
    }
}
```

- [ ] **Step 4: Run the mapping test**

Run: `cargo test -p e2b-rs sandbox::types::tests::snapshot_info_maps` → PASS.

- [ ] **Step 5: Write failing API tests**

In `crates/e2b-rs/src/sandbox/api.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn create_snapshot_returns_info() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes/sbx_s/snapshots"))
            .and(body_partial_json(serde_json::json!({ "name": "snap-a" })))
            .respond_with(ResponseTemplate::new(201).set_body_json(
                serde_json::json!({ "snapshotID": "snap_1", "names": ["snap-a"] }),
            ))
            .mount(&server)
            .await;
        let info = create_snapshot(&api_for(&server), "sbx_s", Some("snap-a"))
            .await
            .expect("snapshot");
        assert_eq!(info.snapshot_id, "snap_1");
    }

    #[tokio::test]
    async fn delete_snapshot_false_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/templates/snap_gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(
            !delete_snapshot(&api_for(&server), "snap_gone")
                .await
                .expect("delete")
        );
    }
```

- [ ] **Step 6: Implement the API calls**

In `crates/e2b-rs/src/sandbox/api.rs` (after `get_sandbox_metrics`):

```rust
/// Create a snapshot of a sandbox. `POST /sandboxes/{id}/snapshots` → `SnapshotInfo`.
pub(crate) async fn create_snapshot(
    api: &ApiClient,
    sandbox_id: &str,
    name: Option<&str>,
) -> Result<api_schema::SnapshotInfo> {
    let mut body = serde_json::json!({});
    if let Some(name) = name {
        body["name"] = serde_json::Value::String(name.to_string());
    }
    let path = format!("/sandboxes/{sandbox_id}/snapshots");
    api.request(reqwest::Method::POST, &path, &[], Some(&body))
        .await
}

/// Delete a snapshot (a snapshot is a template). `DELETE /templates/{id}`;
/// `false` if it was not found.
pub(crate) async fn delete_snapshot(api: &ApiClient, snapshot_id: &str) -> Result<bool> {
    let path = format!("/templates/{snapshot_id}");
    match api
        .request_unit(reqwest::Method::DELETE, &path, &[], None)
        .await
    {
        Ok(()) => Ok(true),
        Err(Error::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}
```

- [ ] **Step 7: Run the API tests**

Run: `cargo test -p e2b-rs sandbox::api::tests::create_snapshot sandbox::api::tests::delete_snapshot` → PASS.

- [ ] **Step 8: Add the public methods**

In `crates/e2b-rs/src/sandbox/sandbox.rs`, `impl Sandbox`:

```rust
    /// Create a snapshot of this sandbox. `name` registers (or re-points) a
    /// template alias for the snapshot.
    pub async fn create_snapshot(&self, name: Option<String>) -> Result<SnapshotInfo> {
        let raw = api::create_snapshot(&self.api, &self.sandbox_id, name.as_deref()).await?;
        Ok(SnapshotInfo::from_schema(raw))
    }

    /// Delete a snapshot by id. Returns `false` if it was already gone.
    pub async fn delete_snapshot(
        snapshot_id: impl Into<String>,
        connection: crate::connection_config::ConnectionConfigOpts,
    ) -> Result<bool> {
        let config = ConnectionConfig::new(connection);
        let api = ApiClient::new(&config, true)?;
        api::delete_snapshot(&api, &snapshot_id.into()).await
    }
```

Add `use crate::sandbox::types::SnapshotInfo;` (or extend the existing `types::` import line in `sandbox.rs`).

- [ ] **Step 9: Re-export public `SnapshotInfo`**

In `crates/e2b-rs/src/sandbox/mod.rs`: `pub use types::{SandboxInfo, SandboxMetrics, SandboxState, SnapshotInfo};`. In `crates/e2b-rs/src/lib.rs`, add `SnapshotInfo` to the `pub use sandbox::{...}` list.

- [ ] **Step 10: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::` → all pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/types.rs crates/e2b-rs/src/sandbox/api.rs crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(sandbox): add create_snapshot + delete_snapshot" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `list_snapshots` + `SnapshotPaginator` (`GET /snapshots`)

Cursor-paginated snapshot listing, mirroring the Plan 3a `SandboxPaginator`. Optional `sandboxID` filter + `limit`; `x-next-token` cursor.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/snapshot_paginator.rs`
- Modify: `crates/e2b-rs/src/sandbox/opts.rs`, `crates/e2b-rs/src/sandbox/api.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `ApiClient::request_with_headers`, `PaginationState`, `api_schema::SnapshotInfo`, `SnapshotInfo` (public), `ConnectionConfig`, `ConnectionConfigOpts`.
- Produces:
  - `pub struct SnapshotListOpts { pub sandbox_id: Option<String>, pub limit: Option<u32>, pub connection: ConnectionConfigOpts }` (in `opts.rs`, `#[derive(Default)]`).
  - `pub(crate) async fn list_snapshots_page(api, sandbox_id: Option<&str>, query_state: &mut PaginationState) -> Result<Vec<api_schema::SnapshotInfo>>` (sends `sandboxID`/`limit`/`nextToken`, updates the cursor from `x-next-token`). [Alternatively inline this in the paginator — see Step 4.]
  - `pub struct SnapshotPaginator` with `pub fn has_next(&self) -> bool` and `pub async fn next_items(&mut self) -> Result<Vec<SnapshotInfo>>`.
  - `pub fn Sandbox::list_snapshots(opts: SnapshotListOpts) -> Result<SnapshotPaginator>` (static).

- [ ] **Step 1: Add `SnapshotListOpts`**

In `crates/e2b-rs/src/sandbox/opts.rs`, add (with `///` docs on every field, `#[derive(Default)]`):

```rust
/// Options for [`Sandbox::list_snapshots`].
#[derive(Default)]
pub struct SnapshotListOpts {
    /// Only list snapshots created from this sandbox id.
    pub sandbox_id: Option<String>,
    /// Maximum number of snapshots per page.
    pub limit: Option<u32>,
    /// Connection configuration (API key, URL, domain, debug).
    pub connection: ConnectionConfigOpts,
}
```

- [ ] **Step 2: Write the failing paginator test**

Create `crates/e2b-rs/src/sandbox/snapshot_paginator.rs` with ONLY the test module first:

```rust
//! Cursor-paginated snapshot listing.

#[cfg(test)]
mod tests {
    use crate::sandbox::opts::SnapshotListOpts;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn lists_one_page_and_stops() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/snapshots"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "snapshotID": "snap_1", "names": ["a"] }
            ])))
            .mount(&server)
            .await;
        let opts = SnapshotListOpts {
            connection: crate::connection_config::ConnectionConfigOpts {
                api_key: Some("e2b_0123456789abcdef".to_string()),
                api_url: Some(server.uri()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pager = crate::Sandbox::list_snapshots(opts).expect("pager");
        assert!(pager.has_next());
        let items = pager.next_items().await.expect("page");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].snapshot_id, "snap_1");
        assert!(!pager.has_next());
    }
}
```

Add to `crates/e2b-rs/src/sandbox/mod.rs`: `pub(crate) mod snapshot_paginator;` and `pub use snapshot_paginator::SnapshotPaginator;`.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::snapshot_paginator`
Expected: FAIL — `SnapshotPaginator` / `Sandbox::list_snapshots` not found.

- [ ] **Step 4: Implement the paginator**

In `crates/e2b-rs/src/sandbox/snapshot_paginator.rs` (above the test module):

```rust
use crate::api::client::ApiClient;
use crate::api::schema as api_schema;
use crate::connection_config::ConnectionConfig;
use crate::errors::Result;
use crate::paginator::PaginationState;
use crate::sandbox::opts::SnapshotListOpts;
use crate::sandbox::types::SnapshotInfo;

/// A cursor-paginated listing of snapshots (`GET /snapshots`).
pub struct SnapshotPaginator {
    api: ApiClient,
    state: PaginationState,
    sandbox_id: Option<String>,
}

impl SnapshotPaginator {
    /// Build a paginator from list options (validates the API key eagerly).
    pub(crate) fn new(opts: SnapshotListOpts) -> Result<Self> {
        let config = ConnectionConfig::new(opts.connection);
        let api = ApiClient::new(&config, true)?;
        Ok(Self {
            api,
            state: PaginationState::new(opts.limit, None),
            sandbox_id: opts.sandbox_id,
        })
    }

    /// Whether more pages remain.
    pub fn has_next(&self) -> bool {
        self.state.has_next()
    }

    /// Fetch the next page. Returns an empty vec (and stops) when exhausted.
    pub async fn next_items(&mut self) -> Result<Vec<SnapshotInfo>> {
        if !self.state.has_next() {
            return Ok(Vec::new());
        }
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(id) = &self.sandbox_id {
            query.push(("sandboxID", id.clone()));
        }
        if let Some(limit) = self.state.limit() {
            query.push(("limit", limit.to_string()));
        }
        if let Some(token) = self.state.next_token() {
            query.push(("nextToken", token.to_string()));
        }

        let (items, headers): (Vec<api_schema::SnapshotInfo>, reqwest::header::HeaderMap) = self
            .api
            .request_with_headers(reqwest::Method::GET, "/snapshots", &query, None)
            .await?;

        let next = headers
            .get("x-next-token")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        self.state.update_from_token(next);

        Ok(items.into_iter().map(SnapshotInfo::from_schema).collect())
    }
}
```

- [ ] **Step 5: Add `Sandbox::list_snapshots`**

In `crates/e2b-rs/src/sandbox/sandbox.rs`, `impl Sandbox`:

```rust
    /// List snapshots (paginated). Filter by source sandbox via
    /// [`SnapshotListOpts::sandbox_id`].
    pub fn list_snapshots(
        opts: crate::sandbox::opts::SnapshotListOpts,
    ) -> Result<crate::sandbox::snapshot_paginator::SnapshotPaginator> {
        crate::sandbox::snapshot_paginator::SnapshotPaginator::new(opts)
    }
```

- [ ] **Step 6: Re-export**

In `crates/e2b-rs/src/sandbox/mod.rs`: extend the opts re-export to include `SnapshotListOpts`, and ensure `pub use snapshot_paginator::SnapshotPaginator;` is present. In `crates/e2b-rs/src/lib.rs`, add `SnapshotListOpts` and `SnapshotPaginator` to the `pub use sandbox::{...}` list (alphabetical).

- [ ] **Step 7: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::snapshot_paginator` → PASS. `cargo test -p e2b-rs` → all pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/snapshot_paginator.rs crates/e2b-rs/src/sandbox/opts.rs crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(sandbox): add paginated list_snapshots + SnapshotPaginator" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: `update_network` (`PUT /sandboxes/{id}/network`)

Update the sandbox's network rules. **Atomic replacement** — omitted fields are CLEARED on the server, not merged. `204` success; `409` if the sandbox is paused. Public `SandboxNetworkUpdate` wraps the generated `SandboxNetworkUpdateConfig`.

**Files:**
- Create: `crates/e2b-rs/src/sandbox/network.rs`
- Modify: `crates/e2b-rs/src/sandbox/api.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `ApiClient::request_unit`.
- Produces:
  - `pub struct SandboxNetworkUpdate { pub allow_internet_access: Option<bool>, pub allow_out: Vec<String>, pub deny_out: Vec<String>, pub rules: BTreeMap<String, Vec<NetworkRule>> }` (`#[derive(Default)]`) + `pub struct NetworkRule { pub transform_headers: BTreeMap<String, String> }` (`#[derive(Default)]`), with `pub(crate) fn to_wire_body(&self) -> serde_json::Value`.
  - `pub(crate) async fn update_sandbox_network(api, sandbox_id, body: &serde_json::Value) -> Result<()>`.
  - `pub async fn Sandbox::update_network(&self, update: SandboxNetworkUpdate) -> Result<()>`.

- [ ] **Step 1: Write the failing wire-body test**

Create `crates/e2b-rs/src/sandbox/network.rs` with the test module first:

```rust
//! Public sandbox network-update types.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_wire_body_uses_correct_casing() {
        let mut update = SandboxNetworkUpdate {
            allow_internet_access: Some(true),
            allow_out: vec!["1.1.1.1".to_string()],
            ..Default::default()
        };
        update
            .rules
            .entry("example.com".to_string())
            .or_default()
            .push(NetworkRule {
                transform_headers: [("X-Test".to_string(), "1".to_string())].into_iter().collect(),
            });
        let body = update.to_wire_body();
        // snake_case allow_internet_access, camelCase allowOut/denyOut.
        assert_eq!(body["allow_internet_access"], serde_json::json!(true));
        assert_eq!(body["allowOut"], serde_json::json!(["1.1.1.1"]));
        assert_eq!(body["denyOut"], serde_json::json!([]));
        assert_eq!(
            body["rules"]["example.com"][0]["transform"]["headers"]["X-Test"],
            serde_json::json!("1")
        );
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::network`
Expected: FAIL — `SandboxNetworkUpdate` not found.

- [ ] **Step 3: Implement the public types + wire mapping**

In `crates/e2b-rs/src/sandbox/network.rs` (above the test module):

```rust
use std::collections::BTreeMap;

/// A single network rule's request transform (e.g. injected headers).
#[derive(Debug, Clone, Default)]
pub struct NetworkRule {
    /// Headers to inject on requests matched by this rule.
    pub transform_headers: BTreeMap<String, String>,
}

/// An atomic update to a sandbox's egress network policy.
///
/// **Replacement semantics:** the update fully replaces the sandbox's policy —
/// any field left empty/`None` is CLEARED on the server, not merged.
#[derive(Debug, Clone, Default)]
pub struct SandboxNetworkUpdate {
    /// Whether the sandbox may reach the public internet.
    pub allow_internet_access: Option<bool>,
    /// Allowed egress destinations (domains/CIDRs).
    pub allow_out: Vec<String>,
    /// Denied egress destinations (domains/CIDRs).
    pub deny_out: Vec<String>,
    /// Per-destination request rules, keyed by destination.
    pub rules: BTreeMap<String, Vec<NetworkRule>>,
}

impl SandboxNetworkUpdate {
    /// Build the `SandboxNetworkUpdateConfig` request body. Field casing matches
    /// the spec: snake_case `allow_internet_access`, camelCase `allowOut`/`denyOut`.
    pub(crate) fn to_wire_body(&self) -> serde_json::Value {
        let rules: serde_json::Map<String, serde_json::Value> = self
            .rules
            .iter()
            .map(|(dest, rules)| {
                let arr: Vec<serde_json::Value> = rules
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "transform": { "headers": r.transform_headers }
                        })
                    })
                    .collect();
                (dest.clone(), serde_json::Value::Array(arr))
            })
            .collect();

        let mut body = serde_json::json!({
            "allowOut": self.allow_out,
            "denyOut": self.deny_out,
            "rules": rules,
        });
        if let Some(allow) = self.allow_internet_access {
            body["allow_internet_access"] = serde_json::Value::Bool(allow);
        }
        body
    }
}
```

- [ ] **Step 4: Run the wire-body test**

Run: `cargo test -p e2b-rs sandbox::network` → PASS.

- [ ] **Step 5: Write the failing API test**

In `crates/e2b-rs/src/sandbox/api.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn update_network_puts_config() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/sandboxes/sbx_n/network"))
            .and(body_partial_json(serde_json::json!({ "allowOut": ["1.1.1.1"] })))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let body = serde_json::json!({ "allowOut": ["1.1.1.1"], "denyOut": [], "rules": {} });
        update_sandbox_network(&api_for(&server), "sbx_n", &body)
            .await
            .expect("update network");
    }
```

- [ ] **Step 6: Implement `update_sandbox_network`**

In `crates/e2b-rs/src/sandbox/api.rs` (after `update`/`set_sandbox_timeout`):

```rust
/// Replace a sandbox's network policy. `PUT /sandboxes/{id}/network` (204).
pub(crate) async fn update_sandbox_network(
    api: &ApiClient,
    sandbox_id: &str,
    body: &serde_json::Value,
) -> Result<()> {
    let path = format!("/sandboxes/{sandbox_id}/network");
    api.request_unit(reqwest::Method::PUT, &path, &[], Some(body))
        .await
}
```

- [ ] **Step 7: Run the API test**

Run: `cargo test -p e2b-rs sandbox::api::tests::update_network` → PASS.

- [ ] **Step 8: Add the public method**

In `crates/e2b-rs/src/sandbox/sandbox.rs`, `impl Sandbox` (add `use crate::sandbox::network::SandboxNetworkUpdate;` or a path reference):

```rust
    /// Atomically replace the sandbox's egress network policy.
    ///
    /// Note: this REPLACES the policy — fields left empty clear the
    /// corresponding server-side rules (no merge). A `409` here means the
    /// sandbox is paused (resume it first).
    pub async fn update_network(
        &self,
        update: crate::sandbox::network::SandboxNetworkUpdate,
    ) -> Result<()> {
        api::update_sandbox_network(&self.api, &self.sandbox_id, &update.to_wire_body()).await
    }
```

- [ ] **Step 9: Wire the module + re-export**

In `crates/e2b-rs/src/sandbox/mod.rs`: add `pub mod network;` and `pub use network::{NetworkRule, SandboxNetworkUpdate};`. In `crates/e2b-rs/src/lib.rs`, add `NetworkRule, SandboxNetworkUpdate` to the `pub use sandbox::{...}` list (alphabetical).

- [ ] **Step 10: Verify & commit**

Run: `cargo test -p e2b-rs sandbox::` → all pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/network.rs crates/e2b-rs/src/sandbox/api.rs crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(sandbox): add update_network (atomic replace) with public types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Signed file URLs (`upload_url` / `download_url`)

Build signed `/files` URLs client-side using the sandbox's `envd_access_token` + `get_signature` — no server call. URL = `{envd_direct_url}/files?username={user}&path={path}[&signature=…&signature_expiration=…]`. Mirrors JS `uploadUrl`/`downloadUrl`.

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/opts.rs`, `crates/e2b-rs/src/sandbox/sandbox.rs`, `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Consumes: `sandbox::signature::{get_signature_now, SignatureOperation}`, `ConnectionConfig::get_sandbox_direct_url`, `ENVD_PORT`, `DEFAULT_USERNAME`, `version_gte`, `Error::InvalidArgument`.
- Produces:
  - `pub struct SandboxUrlOpts { pub user: Option<String>, pub signature_expiration_secs: Option<i64> }` (`#[derive(Default)]`) in `opts.rs`.
  - `pub fn Sandbox::upload_url(&self, path: Option<&str>, opts: SandboxUrlOpts) -> Result<String>`.
  - `pub fn Sandbox::download_url(&self, path: &str, opts: SandboxUrlOpts) -> Result<String>`.

- [ ] **Step 1: Add `SandboxUrlOpts`**

In `crates/e2b-rs/src/sandbox/opts.rs`:

```rust
/// Options for building signed file URLs ([`Sandbox::upload_url`] /
/// [`Sandbox::download_url`]).
#[derive(Default)]
pub struct SandboxUrlOpts {
    /// The sandbox user the URL authorizes (defaults to `user` on older envd).
    pub user: Option<String>,
    /// If set, produce an expiring signature valid for this many seconds.
    pub signature_expiration_secs: Option<i64>,
}
```

- [ ] **Step 2: Write the failing tests**

In `crates/e2b-rs/src/sandbox/sandbox.rs` `mod tests`, add a helper to build a `Sandbox` directly (no server) and assert URL shape. Place near the other tests:

```rust
    fn local_sandbox(token: Option<&str>) -> Sandbox {
        let config = crate::connection_config::ConnectionConfig::new(
            crate::connection_config::ConnectionConfigOpts {
                api_key: Some("e2b_0123456789abcdef".to_string()),
                domain: Some("e2b.app".to_string()),
                ..Default::default()
            },
        );
        let api = ApiClient::new(&config, true).expect("api");
        Sandbox {
            sandbox_id: "sbx_u".to_string(),
            sandbox_domain: Some("e2b.app".to_string()),
            envd_version: "0.6.0".to_string(),
            envd_access_token: token.map(str::to_string),
            config,
            api,
        }
    }

    #[test]
    fn download_url_without_token_is_unsigned() {
        let sandbox = local_sandbox(None);
        let url = sandbox
            .download_url("/home/user/f.txt", Default::default())
            .expect("url");
        assert!(url.contains("/files"));
        assert!(url.contains("path=%2Fhome%2Fuser%2Ff.txt") || url.contains("path=/home/user/f.txt"));
        assert!(!url.contains("signature="));
    }

    #[test]
    fn download_url_with_token_is_signed() {
        let sandbox = local_sandbox(Some("tok_abc"));
        let url = sandbox
            .download_url("/f.txt", Default::default())
            .expect("url");
        assert!(url.contains("signature=v1_"));
    }

    #[test]
    fn expiration_without_token_is_an_error() {
        let sandbox = local_sandbox(None);
        let err = sandbox
            .upload_url(
                Some("/f.txt"),
                crate::sandbox::opts::SandboxUrlOpts {
                    signature_expiration_secs: Some(60),
                    ..Default::default()
                },
            )
            .expect_err("must reject expiration without token");
        assert!(matches!(err, Error::InvalidArgument(_)));
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs sandbox::sandbox::tests::download_url`
Expected: FAIL — `download_url` not found.

- [ ] **Step 4: Implement the URL builders**

In `crates/e2b-rs/src/sandbox/sandbox.rs`. Add imports at top: `use crate::connection_config::{DEFAULT_USERNAME, ENVD_PORT};`, `use crate::sandbox::signature::{get_signature_now, SignatureOperation};`, `use crate::sandbox::opts::SandboxUrlOpts;`. Remove the `#[allow(dead_code)]` on the `envd_access_token` field (now read). Use `reqwest::Url` (already a dependency via `reqwest`) for safe percent-encoding — do NOT add the `urlencoding` crate. Add a private helper + the two public methods to `impl Sandbox`:

```rust
    /// Resolve the per-sandbox domain (override, else config default).
    fn resolved_domain(&self) -> &str {
        self.sandbox_domain
            .as_deref()
            .unwrap_or(&self.config.domain)
    }

    /// Build the base `/files` URL (`{envd_direct_url}/files?username&path`),
    /// percent-encoding the query values via `reqwest::Url`.
    fn file_url(&self, path: &str, user: Option<&str>) -> Result<reqwest::Url> {
        let base =
            self.config
                .get_sandbox_direct_url(&self.sandbox_id, self.resolved_domain(), ENVD_PORT);
        let mut url = reqwest::Url::parse(&format!("{base}/files"))
            .map_err(|e| Error::Internal(format!("invalid sandbox url: {e}")))?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(user) = user {
                q.append_pair("username", user);
            }
            if !path.is_empty() {
                q.append_pair("path", path);
            }
        }
        Ok(url)
    }

    /// Build a signed (or unsigned) URL for `op` on `path`.
    fn signed_file_url(
        &self,
        path: &str,
        op: SignatureOperation,
        opts: &SandboxUrlOpts,
    ) -> Result<String> {
        let use_signature = self
            .envd_access_token
            .as_deref()
            .is_some_and(|t| !t.is_empty());
        if !use_signature && opts.signature_expiration_secs.is_some() {
            return Err(Error::InvalidArgument(
                "Signature expiration can be used only when the sandbox is created as secured."
                    .to_string(),
            ));
        }

        // Older envd (<0.4.0) has no per-request user; default to the legacy user.
        let user = match opts.user.as_deref() {
            Some(u) => Some(u.to_string()),
            None if !version_gte(&self.envd_version, "0.4.0") => Some(DEFAULT_USERNAME.to_string()),
            None => None,
        };

        let mut url = self.file_url(path, user.as_deref())?;
        if use_signature {
            let sig = get_signature_now(
                path,
                op,
                user.as_deref(),
                opts.signature_expiration_secs,
                self.envd_access_token.as_deref(),
            )?;
            url.query_pairs_mut()
                .append_pair("signature", &sig.signature);
            if let Some(exp) = sig.expiration {
                url.query_pairs_mut()
                    .append_pair("signature_expiration", &exp.to_string());
            }
        }
        Ok(url.to_string())
    }

    /// Build a URL for uploading a file to the sandbox (empty `path` = the
    /// default upload directory). Signed when the sandbox is secured.
    pub fn upload_url(&self, path: Option<&str>, opts: SandboxUrlOpts) -> Result<String> {
        self.signed_file_url(path.unwrap_or(""), SignatureOperation::Write, &opts)
    }

    /// Build a URL for downloading `path` from the sandbox. Signed when the
    /// sandbox is secured.
    pub fn download_url(&self, path: &str, opts: SandboxUrlOpts) -> Result<String> {
        self.signed_file_url(path, SignatureOperation::Read, &opts)
    }
```

NOTE: `reqwest::Url::query_pairs_mut()` uses `application/x-www-form-urlencoded`, so `/` encodes as `%2F` and the Step-2 test's `path=%2Fhome%2Fuser%2Ff.txt` assertion holds; `signature=v1_…` stays literal (no special chars in the `v1_`+base64url-without-`=` signature). The `Error::Internal` import: `Error` is already used elsewhere in this method (`InvalidArgument`), so no import change beyond what Step 8/Task 2 added.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p e2b-rs sandbox::sandbox::tests` → all pass (incl. the 3 new URL tests).

- [ ] **Step 6: Re-export `SandboxUrlOpts`**

In `crates/e2b-rs/src/sandbox/mod.rs`: add `SandboxUrlOpts` to the opts re-export. In `crates/e2b-rs/src/lib.rs`: add `SandboxUrlOpts` to the `pub use sandbox::{...}` list.

- [ ] **Step 7: Verify & commit**

Run: `cargo test -p e2b-rs` → all pass. `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/opts.rs crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(sandbox): add signed upload_url/download_url builders" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Parity checklist, quickstart & full gate

**Files:**
- Modify: `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc), `README.md`

- [ ] **Step 1: Update the crate quickstart**

In `crates/e2b-rs/src/lib.rs`'s `//!` docs, extend the `## Creating a sandbox` example (or add a `## Snapshots & metrics` section) with a `no_run` snippet exercising the new surface:

```rust
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
```

- [ ] **Step 2: Update the parity checklist**

In `docs/parity-checklist.md`, add under the Plan 3a section:

```markdown
## Sandbox control-plane extras (Plan 3a-extras)

| JS (`src/sandbox/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `sandbox.pause` / `betaPause` | `Sandbox::pause` (409→false) | ✅ |
| `sandbox.getMetrics` | `Sandbox::get_metrics` → `SandboxMetrics` | ✅ |
| `sandbox.createSnapshot` | `Sandbox::create_snapshot` → `SnapshotInfo` | ✅ |
| `Sandbox.listSnapshots` / `SnapshotPaginator` | `Sandbox::list_snapshots` + `SnapshotPaginator` | ✅ |
| `SandboxApi.deleteSnapshot` | `Sandbox::delete_snapshot` | ✅ |
| `sandbox.updateNetwork` | `Sandbox::update_network` (atomic) | ✅ |
| `sandbox.uploadUrl` / `downloadUrl` | `Sandbox::upload_url` / `download_url` | ✅ |
| `sandbox.getMcpUrl` / `getMcpToken` / `create({mcp})` | _(deferred: needs files.read + hand-authored McpServer)_ | ⬜ |
```

- [ ] **Step 3: Update README**

Add a short bullet/snippet under the README's usage section noting pause/metrics/snapshots/network/signed-URLs are available. Only stage `README.md` if it actually changed.

- [ ] **Step 4: Full release gate**

Run each and confirm it passes:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` (report counts; 0 failures)
- `cargo test --doc -p e2b-rs` (new doctests compile under `no_run`)
- `cargo doc --no-deps -p e2b-rs` (denies broken intra-doc links — fix any `[Type]` link that doesn't resolve by using `crate::Type`)
- `cargo xtask codegen && git status --porcelain` → empty (codegen idempotent; do NOT commit regenerated files)

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(sandbox): document control-plane extras and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 3a-extras is complete when:
- `Sandbox::pause`, `get_metrics`, `create_snapshot`, `delete_snapshot`, `list_snapshots` (+ `SnapshotPaginator`), `update_network`, `upload_url`, `download_url` are implemented and wiremock/unit-tested.
- All new public types (`SandboxMetrics`, `SnapshotInfo`, `SnapshotListOpts`, `SandboxNetworkUpdate`, `NetworkRule`, `SandboxUrlOpts`, `SnapshotPaginator`) are re-exported at the crate root; NO generated `api::schema` type leaks into the public API.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects the extras; MCP explicitly marked deferred.

**Carry-forwards (out of scope, documented):** MCP (getMcpUrl/getMcpToken/`create({mcp})`) deferred until Plan 3b provides `files.read` and the `McpServer` config type is hand-authored; `pause` exposes only `keep_memory=true` (add a `memory=false` variant if needed); metrics omit the soft `<0.2.4` disk-unsupported warning; `get_metrics` time-range params (`start`/`end`) wired in the API layer but not yet surfaced on the public method.

**Next:** Plan 3b (Filesystem) — `sandbox.files` (read/write/list/watch) on `ConnectClient`+`EnvdApiClient`+`envd::proto`, watch streaming via `tokio::sync::mpsc`.
