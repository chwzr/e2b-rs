# E2B Rust SDK — REST Transports (Plan 2b-i) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the async HTTP foundation and the two REST transport clients — `ApiClient` (E2B control-plane) and `EnvdApiClient` (the in-sandbox envd daemon's REST surface) — with auth, API-key validation, status→`Error` mapping, a concurrency limiter, and logging, all tested with `wiremock`.

**Architecture:** Async-only on `tokio` + `reqwest`. Each client is a hand-written struct owning a configured `reqwest::Client` (default headers incl. `User-Agent` + auth), a `tokio::sync::Semaphore` (in-flight cap, the Rust analogue of the JS `limitConcurrency` fetch wrapper), and an optional logger. A generic typed `request` method centralizes header injection, the timeout, status→`Error` mapping (extracting the server's error message), and logging; concrete endpoints (this plan ships `health()`) build on it. The per-endpoint sandbox/volume/template calls are added by the milestones that consume these clients (Plan 3+).

**Tech Stack:** `tokio` (sync/time), `reqwest` 0.13 (json, stream, rustls-tls), `serde`/`serde_json`; dev: `wiremock`, `tokio` (macros, rt-multi-thread).

**Reference spec:** `.super/specs/2026-06-28-e2b-rust-sdk-design.md` (§4.3 transports, §7 config). **JS source:** `../E2B/packages/js-sdk/src/{api/index.ts,api/inflight.ts,api/metadata.ts,envd/api.ts,connectionConfig.ts}`.

## Milestone roadmap (context — Plans 1 + 2a are merged to `main`)

| Plan | Deliverable |
|---|---|
| 1 — Foundation · 2a — Codegen | DONE, merged |
| **2b-i — REST transports (this plan)** | `ApiClient` + `EnvdApiClient` + inflight limiter + async deps; wiremock-tested |
| 2b-ii — Connect client | envelope codec + unary + server-streaming + `Code`→`Error` + version gates (against 2b-i + the proto types) |
| 3 — Sandbox & envd I/O · 4 — Git & Volume · 5 — Template & Polish | … |

## Global Constraints

- **Repo/workspace:** package `e2b-rs`, lib `e2b_rs`, all crates under `crates/`. Edition 2024, MSRV 1.95.0.
- **Lints (panic-free lib):** `clippy::unwrap_used`/`expect_used`/`missing_docs` denied (`[workspace.lints]`); allowed in tests via `clippy.toml`. NO `.unwrap()`/`.expect()`/`panic!` in non-test code — use `?`/`match`/`.map_err`. Generated modules stay exempt via their header + `pub(crate)`.
- **Generated module naming (DECIDED):** rename the typify `gen` modules to `schema` — `api::schema`, `volume::schema` (avoids the Rust-2024 `gen` keyword / `r#gen`). The proto modules (`envd::proto::*`) and `envd::rest_gen` keep their names.
- **Three `Error` types collide by name** — `api::schema::Error` (i32 `code`), `volume::schema::Error` (string `code`), and the SDK's own `errors::Error`. Always import the generated ones under qualified aliases (e.g. `use crate::api::schema::Error as ApiError;`); never `use ...::Error` bare.
- **Transport clients are internal** (`pub(crate)`) this plan — the public `Sandbox`/`Volume` API is Plan 3+. Do NOT expose raw generated types publicly (spec §1 non-goal).
- **Async model:** async-only; `tokio`. Library `tokio` features stay minimal (`sync`, `time`); `macros`/`rt-multi-thread` are dev-only (for `#[tokio::test]`).
- **Parity:** match the cited JS behavior (the `^e2b_[0-9a-f]+$` key pattern with lowercase hex; the status→error map; default headers; env-tuned connection/inflight limits).
- **Docs:** every public/`pub(crate)`-but-documented hand-written item gets a `///` doc; `no_run` doctests for anything that would touch the network.
- **Commits:** conventional messages ending with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Run `cargo fmt --all` before every commit.

### File structure (this plan)

```
crates/e2b-rs/src/
├── errors.rs                 # MODIFY: add Error::Transport(#[from] reqwest::Error)
├── http/
│   ├── mod.rs                # pub(crate): re-exports inflight + shared bits
│   └── inflight.rs           # Semaphore-based concurrency limiter (port of inflight.ts)
├── api/
│   ├── mod.rs                # MODIFY: mod schema (renamed); + mod client
│   ├── schema.rs             # RENAMED from gen.rs (vendored; via xtask)
│   └── client.rs             # ApiClient (control-plane REST)
├── volume/
│   ├── mod.rs                # MODIFY: mod schema (renamed)
│   └── schema.rs             # RENAMED from gen.rs (vendored; via xtask)
└── envd/
    ├── mod.rs                # MODIFY: + pub(crate) mod rest
    └── rest.rs               # EnvdApiClient (/health; the configured envd REST client)
crates/xtask/src/
├── openapi.rs                # (unchanged helper)
└── main.rs                   # MODIFY: output paths api/gen.rs→api/schema.rs, volume/gen.rs→volume/schema.rs
```

---

### Task 1: Rename `gen`→`schema`, add async deps & `Error::Transport`

**Files:**
- Modify: `crates/xtask/src/main.rs` (output paths)
- Rename (via codegen): `crates/e2b-rs/src/api/gen.rs` → `api/schema.rs`; `volume/gen.rs` → `volume/schema.rs`
- Modify: `crates/e2b-rs/src/api/mod.rs`, `crates/e2b-rs/src/volume/mod.rs`
- Modify: `Cargo.toml` (workspace deps), `crates/e2b-rs/Cargo.toml`
- Modify: `crates/e2b-rs/src/errors.rs` (+ test)

**Interfaces:**
- Produces: `api::schema` / `volume::schema` modules (renamed); `tokio`/`reqwest`/`wiremock` available; `Error::Transport(reqwest::Error)` variant + `#[from]`.

- [ ] **Step 1: Repoint codegen output to `schema.rs`**

In `crates/xtask/src/main.rs`, change the two OpenAPI output paths: `sdk_src.join("api/gen.rs")` → `sdk_src.join("api/schema.rs")`, and `sdk_src.join("volume/gen.rs")` → `sdk_src.join("volume/schema.rs")`. Leave the proto and `envd/rest_gen.rs` calls unchanged.

- [ ] **Step 2: Delete the old generated files and regenerate**

```bash
git rm crates/e2b-rs/src/api/gen.rs crates/e2b-rs/src/volume/gen.rs
cargo xtask codegen   # writes api/schema.rs and volume/schema.rs
```

- [ ] **Step 3: Update the module declarations and test aliases**

In `crates/e2b-rs/src/api/mod.rs`: change `pub(crate) mod r#gen;` → `pub(crate) mod schema;` and the test `use super::r#gen as api_gen;` → `use super::schema as api_gen;` (keep the rest of the round-trip test identical).

In `crates/e2b-rs/src/volume/mod.rs`: change `pub(crate) mod r#gen;` → `pub(crate) mod schema;` and `use super::r#gen as volume_gen;` → `use super::schema as volume_gen;`.

- [ ] **Step 4: Add the async dependencies**

In the workspace `Cargo.toml` `[workspace.dependencies]`, add:

```toml
tokio = { version = "1", default-features = false }
reqwest = { version = "0.13", default-features = false, features = ["json", "stream", "rustls-tls"] }
futures = "0.3"
wiremock = "0.6"
```

In `crates/e2b-rs/Cargo.toml`:

```toml
[dependencies]
# ... existing ...
tokio = { workspace = true, features = ["sync", "time"] }
reqwest = { workspace = true }
futures = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
wiremock = { workspace = true }
```

- [ ] **Step 5: Write the failing test for `Error::Transport`**

In `crates/e2b-rs/src/errors.rs`, add to the `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn reqwest_error_converts_via_from() {
    // A connection to an unroutable address yields a reqwest::Error,
    // which must convert into Error::Transport via `?`/`#[from]`.
    fn try_it(e: reqwest::Error) -> Error {
        Error::from(e)
    }
    let err = reqwest::get("http://127.0.0.1:1/nope").await.unwrap_err();
    assert!(matches!(try_it(err), Error::Transport(_)));
}
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p e2b-rs errors::tests::reqwest_error_converts_via_from`
Expected: FAIL — no `Error::Transport` variant.

- [ ] **Step 7: Add the variant**

In `crates/e2b-rs/src/errors.rs`, add to the `Error` enum (before `Internal`):

```rust
    /// Underlying HTTP transport error (connection, TLS, timeout at the wire level).
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
```

- [ ] **Step 8: Verify and gate**

Confirm the rename: `crates/e2b-rs/src/api/schema.rs` and `volume/schema.rs` exist; `api/gen.rs`/`volume/gen.rs` are gone (`ls crates/e2b-rs/src/api crates/e2b-rs/src/volume`).
Run: `cargo build -p e2b-rs`; `cargo test -p e2b-rs` (the api/volume round-trips still pass under the new `schema` module name, plus the new `errors` transport test); `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check`.
(The strict codegen-idempotency no-op check — `cargo xtask codegen && git status --porcelain` empty — is run after the commit, in Task 5's gate.)

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add -A
git commit -m "refactor(codegen): rename generated gen→schema modules; add async deps + Error::Transport" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

(`git add -A` is acceptable here because the only changes are the tracked rename + manifests + errors.rs; confirm `git status` shows nothing unexpected first.)

---

### Task 2: In-flight concurrency limiter (`http/inflight.rs`)

Port `api/inflight.ts`: a FIFO concurrency cap. The JS wraps `fetch` with a semaphore releasing a slot when headers resolve. In Rust we expose an owned-permit guard from a `tokio::sync::Semaphore`; callers hold the permit across a request and drop it when done.

**Files:**
- Create: `crates/e2b-rs/src/http/mod.rs`, `crates/e2b-rs/src/http/inflight.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (`pub(crate) mod http;`)

**Interfaces:**
- Produces: `http::inflight::ConcurrencyLimiter` with `pub(crate) fn new(max: usize) -> Self`, `pub(crate) async fn acquire(&self) -> Option<OwnedSemaphorePermit>` (returns `None` when the limiter is disabled, i.e. `max == 0`).

- [ ] **Step 1: Write the failing test**

Create `crates/e2b-rs/src/http/inflight.rs`:

```rust
//! FIFO concurrency limiter for outbound requests (port of `api/inflight.ts`).

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn caps_concurrent_holders() {
        let limiter = Arc::new(ConcurrencyLimiter::new(2));
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let (l, live, peak) = (limiter.clone(), live.clone(), peak.clone());
            handles.push(tokio::spawn(async move {
                let _permit = l.acquire().await;
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                live.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.expect("task");
        }
        assert!(peak.load(Ordering::SeqCst) <= 2, "never more than 2 in flight");
    }

    #[tokio::test]
    async fn disabled_limiter_returns_none_permit() {
        let limiter = ConcurrencyLimiter::new(0);
        assert!(limiter.acquire().await.is_none());
    }
}
```

Add `pub(crate) mod http;` to `crates/e2b-rs/src/lib.rs`, and create `crates/e2b-rs/src/http/mod.rs`:

```rust
//! Shared HTTP plumbing for the REST transport clients.

pub(crate) mod inflight;
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p e2b-rs http::inflight`
Expected: FAIL — `ConcurrencyLimiter` not found.

- [ ] **Step 3: Implement the limiter**

Insert above the test module in `inflight.rs`:

```rust
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// A FIFO cap on concurrent in-flight requests. `max == 0` disables the cap
/// (every `acquire` returns `None`, matching JS `limitConcurrency(max<=0)`).
#[derive(Clone)]
pub(crate) struct ConcurrencyLimiter {
    sem: Option<Arc<Semaphore>>,
}

impl ConcurrencyLimiter {
    /// Create a limiter allowing `max` concurrent holders (`0` = unlimited).
    pub(crate) fn new(max: usize) -> Self {
        let sem = if max == 0 {
            None
        } else {
            Some(Arc::new(Semaphore::new(max)))
        };
        Self { sem }
    }

    /// Acquire a slot, waiting (FIFO) if the cap is reached. Returns `None` when
    /// the limiter is disabled. Hold the returned permit for the request's
    /// lifetime; dropping it frees the slot.
    pub(crate) async fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        match &self.sem {
            None => None,
            Some(sem) => match sem.clone().acquire_owned().await {
                Ok(permit) => Some(permit),
                // Semaphore is never closed in our usage; treat closure as "no cap".
                Err(_) => None,
            },
        }
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo test -p e2b-rs http::inflight` → PASS (2 tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings` → clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/http crates/e2b-rs/src/lib.rs
git commit -m "feat(http): add FIFO concurrency limiter" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `ApiClient` (control-plane REST)

Port `api/index.ts`: a client over the control-plane API with API-key validation (`^e2b_[0-9a-f]+$`, lowercase hex), `X-API-KEY` / `Authorization: Bearer` headers, status→`Error` mapping (server message extraction), the in-flight cap, and logging. Ships a `health()` endpoint as the first concrete call; the generic `request` method underpins the sandbox/template calls added in Plan 3+.

**Files:**
- Create: `crates/e2b-rs/src/api/client.rs`
- Modify: `crates/e2b-rs/src/api/mod.rs` (`pub(crate) mod client;`)

**Interfaces:**
- Consumes: `crate::connection_config::ConnectionConfig`, `crate::http::inflight::ConcurrencyLimiter`, `crate::errors::{Error, Result}`, `crate::logs::Logger`, `api::schema::Error as ApiError`.
- Produces:
  - `pub(crate) fn validate_api_key(key: &str) -> Result<()>` (the `^e2b_[0-9a-f]+$` check; `Err(Error::Authentication)` with the `e2b_<40 hex>` example).
  - `pub(crate) struct ApiClient` with `pub(crate) fn new(config: &ConnectionConfig, require_api_key: bool) -> Result<Self>`, `pub(crate) async fn request<T: serde::de::DeserializeOwned>(&self, method: reqwest::Method, path: &str, query: &[(&str, String)], body: Option<&serde_json::Value>) -> Result<T>`, `pub(crate) async fn request_unit(&self, method, path, query, body) -> Result<()>` (no response body), and `pub(crate) async fn health(&self) -> Result<()>`.

- [ ] **Step 1: Write the failing tests**

Create `crates/e2b-rs/src/api/client.rs`:

```rust
//! Control-plane REST client (port of `api/index.ts`).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(server: &MockServer) -> ApiClient {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            api_url: Some(server.uri()),
            ..Default::default()
        });
        ApiClient::new(&config, true).expect("construct ApiClient")
    }

    #[test]
    fn validate_api_key_accepts_valid_and_rejects_invalid() {
        assert!(validate_api_key("e2b_0123456789abcdef").is_ok());
        assert!(matches!(
            validate_api_key("not-a-key"),
            Err(crate::errors::Error::Authentication(_))
        ));
        // Uppercase hex is NOT allowed (JS pattern is lowercase [0-9a-f]).
        assert!(validate_api_key("e2b_ABCDEF").is_err());
        assert!(validate_api_key("e2b_").is_err());
    }

    #[test]
    fn new_requires_api_key_when_asked() {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: None,
            api_url: Some("https://api.example".to_string()),
            ..Default::default()
        });
        assert!(matches!(
            ApiClient::new(&config, true),
            Err(crate::errors::Error::Authentication(_))
        ));
        // require_api_key=false allows construction without a key.
        assert!(ApiClient::new(&config, false).is_ok());
    }

    #[tokio::test]
    async fn health_sends_api_key_header_and_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("X-API-KEY", "e2b_0123456789abcdef"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        client_for(&server).health().await.expect("health ok");
    }

    #[tokio::test]
    async fn maps_status_codes_to_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "code": 401, "message": "bad key"
            })))
            .mount(&server)
            .await;
        let err = client_for(&server).health().await.unwrap_err();
        match err {
            crate::errors::Error::Authentication(msg) => assert!(msg.contains("bad key")),
            other => panic!("expected Authentication, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rate_limit_maps_to_rate_limit_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        assert!(matches!(
            client_for(&server).health().await,
            Err(crate::errors::Error::RateLimit(_))
        ));
    }
}
```

Add to `crates/e2b-rs/src/api/mod.rs`: `pub(crate) mod client;`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs api::client`
Expected: FAIL — `ApiClient`/`validate_api_key` not found.

- [ ] **Step 3: Implement the client**

Insert above the test module in `api/client.rs`:

```rust
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::de::DeserializeOwned;

use crate::api::schema::Error as ApiError;
use crate::connection_config::ConnectionConfig;
use crate::errors::{Error, Result};
use crate::http::inflight::ConcurrencyLimiter;
use crate::logs::Logger;
use std::sync::Arc;

/// Validate an E2B API key: `e2b_` followed by one or more lowercase hex chars.
/// Mirrors `API_KEY_PATTERN` in `api/index.ts`.
pub(crate) fn validate_api_key(key: &str) -> Result<()> {
    let valid = key
        .strip_prefix("e2b_")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    if valid {
        Ok(())
    } else {
        let example = format!("e2b_{}", "0".repeat(40));
        Err(Error::Authentication(format!(
            "Invalid API key format: expected \"e2b_\" followed by hex characters (e.g. \"{example}\")."
        )))
    }
}

/// Client for the E2B control-plane REST API.
pub(crate) struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    request_timeout: Duration,
    limiter: ConcurrencyLimiter,
    logger: Option<Arc<dyn Logger>>,
}

impl ApiClient {
    /// Build a client from a [`ConnectionConfig`]. Validates the API key format
    /// when present and `validate_api_key` is enabled; errors when
    /// `require_api_key` is set but no key is configured.
    pub(crate) fn new(config: &ConnectionConfig, require_api_key: bool) -> Result<Self> {
        if require_api_key && config.api_key.is_none() {
            return Err(Error::Authentication(
                "API key is required: set E2B_API_KEY or pass api_key in the options.".to_string(),
            ));
        }
        if let Some(key) = &config.api_key {
            if config.validate_api_key {
                validate_api_key(key)?;
            }
        }

        let mut headers = HeaderMap::new();
        for (name, value) in &config.headers {
            if let (Ok(n), Ok(v)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
                headers.insert(n, v);
            }
        }
        if let Some(key) = &config.api_key {
            if let Ok(v) = HeaderValue::from_str(key) {
                headers.insert(HeaderName::from_static("x-api-key"), v);
            }
        }
        if let Some(token) = &config.access_token {
            if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(reqwest::header::AUTHORIZATION, v);
            }
        }

        let mut builder = reqwest::Client::builder().default_headers(headers);
        if let Some(proxy) = &config.proxy {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }
        let http = builder.build()?;

        Ok(Self {
            http,
            base_url: config.api_url.trim_end_matches('/').to_string(),
            request_timeout: Duration::from_millis(config.request_timeout_ms),
            limiter: ConcurrencyLimiter::new(0), // cap wired from env in a later task; 0 = unlimited
            logger: config.logger.clone(),
        })
    }

    /// Make a request, deserializing a JSON response into `T`. Centralizes the
    /// in-flight cap, timeout, status→error mapping, and logging.
    pub(crate) async fn request<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<T> {
        let body = self.send(method, path, query, body).await?;
        serde_json::from_slice::<T>(&body)
            .map_err(|e| Error::Internal(format!("failed to decode response from {path}: {e}")))
    }

    /// Like [`ApiClient::request`] but discards the response body.
    pub(crate) async fn request_unit(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<()> {
        self.send(method, path, query, body).await.map(|_| ())
    }

    /// Health check: `GET /health`.
    pub(crate) async fn health(&self) -> Result<()> {
        self.request_unit(reqwest::Method::GET, "/health", &[], None).await
    }

    /// Shared request execution: build, send, log, map status to `Error`, and
    /// return the raw success body bytes.
    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
    ) -> Result<Vec<u8>> {
        let _permit = self.limiter.acquire().await;
        let url = format!("{}{path}", self.base_url);
        if let Some(logger) = &self.logger {
            logger.debug(&format!("{method} {url}"));
        }

        let mut req = self.http.request(method, &url).timeout(self.request_timeout);
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let body_bytes = resp.bytes().await?;

        if status.is_success() {
            return Ok(body_bytes.to_vec());
        }

        // Extract the server's error message (control-plane Error.message), else the status reason.
        let message = serde_json::from_slice::<ApiError>(&body_bytes)
            .map(|e| e.message)
            .unwrap_or_else(|_| status.canonical_reason().unwrap_or("request failed").to_string());
        if let Some(logger) = &self.logger {
            logger.error(&format!("{} {url} -> {}", status.as_u16(), message));
        }
        Err(Error::from_status(status.as_u16(), message))
    }
}
```

(No extra dependency is needed: `resp.bytes().await?` yields `reqwest::Bytes`, which `.to_vec()` turns into `Vec<u8>`; `serde_json::from_slice` reads `&[u8]` directly.)

- [ ] **Step 4: Verify**

Run: `cargo test -p e2b-rs api::client`
Expected: PASS (5 tests). `wiremock` spins a local mock server; the 401-with-body test confirms message extraction, the 429 test confirms `from_status` mapping.

Run: `cargo clippy --workspace --all-targets -- -D warnings` → clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/api
git commit -m "feat(api): add control-plane ApiClient with auth, key validation, error mapping" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `EnvdApiClient` (envd REST surface)

Port the client side of `envd/api.ts`: a configured client for the in-sandbox envd daemon's REST surface, carrying the `E2b-Sandbox-Id`/`E2b-Sandbox-Port` headers, the `X-Access-Token`, and the `User-Agent`. Ships `check_health()` (`GET /health` with a short timeout, mapping 502→sandbox-timeout). The `/files` read/write methods are deferred to Plan 3 (Filesystem), where the multipart/octet-stream/gzip body handling lives.

**Files:**
- Create: `crates/e2b-rs/src/envd/rest.rs`
- Modify: `crates/e2b-rs/src/envd/mod.rs` (`pub(crate) mod rest;`)

**Interfaces:**
- Consumes: `crate::errors::{Error, Result}`, `crate::logs::Logger`, `api::schema`-style status mapping via `Error::from_status`.
- Produces:
  - `pub(crate) struct EnvdApiClient` with `pub(crate) fn new(opts: EnvdApiClientOpts) -> Result<Self>` and `pub(crate) async fn check_health(&self) -> bool` (true iff `GET /health` returns 2xx within the health timeout; false on any error/timeout — mirrors `checkSandboxHealth`).
  - `pub(crate) struct EnvdApiClientOpts { pub base_url: String, pub access_token: Option<String>, pub sandbox_id: String, pub envd_port: u16, pub user_agent: String, pub request_timeout_ms: u64, pub logger: Option<Arc<dyn Logger>>, pub proxy: Option<String> }`.

- [ ] **Step 1: Write the failing tests**

Create `crates/e2b-rs/src/envd/rest.rs`:

```rust
//! envd daemon REST client (port of the client side of `envd/api.ts`).

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn opts_for(server: &MockServer) -> EnvdApiClientOpts {
        EnvdApiClientOpts {
            base_url: server.uri(),
            access_token: Some("tok-123".to_string()),
            sandbox_id: "sbx_test".to_string(),
            envd_port: 49983,
            user_agent: "e2b-rs/test".to_string(),
            request_timeout_ms: 5_000,
            logger: None,
            proxy: None,
        }
    }

    #[tokio::test]
    async fn check_health_true_on_200_with_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("X-Access-Token", "tok-123"))
            .and(header("E2b-Sandbox-Id", "sbx_test"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = EnvdApiClient::new(opts_for(&server)).expect("construct");
        assert!(client.check_health().await);
    }

    #[tokio::test]
    async fn check_health_false_on_502() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let client = EnvdApiClient::new(opts_for(&server)).expect("construct");
        assert!(!client.check_health().await);
    }
}
```

Add to `crates/e2b-rs/src/envd/mod.rs`: `pub(crate) mod rest;`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs envd::rest`
Expected: FAIL — `EnvdApiClient`/`EnvdApiClientOpts` not found.

- [ ] **Step 3: Implement the client**

Insert above the test module in `envd/rest.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::errors::Result;
use crate::logs::Logger;

/// Health-check timeout for envd, matching `checkSandboxHealth` (5s).
const HEALTH_TIMEOUT_MS: u64 = 5_000;

/// Options for constructing an [`EnvdApiClient`].
pub(crate) struct EnvdApiClientOpts {
    /// Base URL of the sandbox's envd REST surface.
    pub base_url: String,
    /// envd access token (sent as `X-Access-Token`).
    pub access_token: Option<String>,
    /// Sandbox id (sent as `E2b-Sandbox-Id`).
    pub sandbox_id: String,
    /// envd port (sent as `E2b-Sandbox-Port`).
    pub envd_port: u16,
    /// `User-Agent` header value.
    pub user_agent: String,
    /// Per-request timeout in milliseconds.
    pub request_timeout_ms: u64,
    /// Optional logger.
    pub logger: Option<Arc<dyn Logger>>,
    /// Optional proxy URL.
    pub proxy: Option<String>,
}

/// Client for the in-sandbox envd daemon's REST surface (`/health`, and — in a
/// later milestone — `/files`).
pub(crate) struct EnvdApiClient {
    http: reqwest::Client,
    base_url: String,
    request_timeout: Duration,
    logger: Option<Arc<dyn Logger>>,
}

impl EnvdApiClient {
    /// Build the client, baking the sandbox/access headers into the underlying
    /// `reqwest::Client` as default headers.
    pub(crate) fn new(opts: EnvdApiClientOpts) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let mut put = |name: &'static str, value: &str| {
            if let Ok(v) = HeaderValue::from_str(value) {
                headers.insert(HeaderName::from_static(name), v);
            }
        };
        put("user-agent", &opts.user_agent);
        put("e2b-sandbox-id", &opts.sandbox_id);
        put("e2b-sandbox-port", &opts.envd_port.to_string());
        if let Some(token) = &opts.access_token {
            put("x-access-token", token);
        }

        let mut builder = reqwest::Client::builder().default_headers(headers);
        if let Some(proxy) = &opts.proxy {
            if let Ok(p) = reqwest::Proxy::all(proxy) {
                builder = builder.proxy(p);
            }
        }
        let http = builder.build()?;

        Ok(Self {
            http,
            base_url: opts.base_url.trim_end_matches('/').to_string(),
            request_timeout: Duration::from_millis(opts.request_timeout_ms),
            logger: opts.logger,
        })
    }

    /// Return `true` iff `GET /health` responds 2xx within the health timeout.
    /// Any error, non-2xx, or timeout yields `false` (mirrors `checkSandboxHealth`).
    pub(crate) async fn check_health(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        let timeout = Duration::from_millis(HEALTH_TIMEOUT_MS.min(self.request_timeout.as_millis() as u64).max(1));
        if let Some(logger) = &self.logger {
            logger.debug(&format!("GET {url} (health)"));
        }
        match self.http.get(&url).timeout(timeout).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo test -p e2b-rs envd::rest` → PASS (2 tests).
Run: `cargo clippy --workspace --all-targets -- -D warnings` → clean.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/envd
git commit -m "feat(envd): add EnvdApiClient with sandbox headers and health check" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Connection-limit env tuning, parity checklist & full gate

Wire the env-tuned in-flight cap into `ApiClient` (port `parseInflightLimitEnv` / the `E2B_API_INFLIGHT_REQUESTS` default of 1000), update the parity checklist, and run the full release gate.

**Files:**
- Modify: `crates/e2b-rs/src/connection_config.rs` (add inflight-limit resolution + test)
- Modify: `crates/e2b-rs/src/api/client.rs` (use the configured cap)
- Modify: `docs/parity-checklist.md`

**Interfaces:**
- Consumes: `ConnectionConfig`.
- Produces: `ConnectionConfig.api_inflight_requests: usize` (resolved from `E2B_API_INFLIGHT_REQUESTS`, default 1000; `0` disables the cap).

- [ ] **Step 1: Write the failing test**

In `crates/e2b-rs/src/connection_config.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn resolves_api_inflight_limit() {
    let c = cfg(ConnectionConfigOpts::default(), &[]);
    assert_eq!(c.api_inflight_requests, 1000); // default

    let c2 = cfg(ConnectionConfigOpts::default(), &[("E2B_API_INFLIGHT_REQUESTS", "50")]);
    assert_eq!(c2.api_inflight_requests, 50);

    // 0 is allowed (disables the cap), per parseInflightLimitEnv.
    let c3 = cfg(ConnectionConfigOpts::default(), &[("E2B_API_INFLIGHT_REQUESTS", "0")]);
    assert_eq!(c3.api_inflight_requests, 0);

    // Non-integer falls back to the default rather than panicking.
    let c4 = cfg(ConnectionConfigOpts::default(), &[("E2B_API_INFLIGHT_REQUESTS", "nope")]);
    assert_eq!(c4.api_inflight_requests, 1000);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs connection_config::tests::resolves_api_inflight_limit`
Expected: FAIL — no `api_inflight_requests` field.

- [ ] **Step 3: Implement**

In `connection_config.rs`, add the field to `ConnectionConfig`:

```rust
    /// Max concurrent in-flight control-plane requests (`0` = unlimited).
    pub api_inflight_requests: usize,
```

In `from_env`, before the `Self { .. }` construction, add:

```rust
        // Inflight cap: allows 0 (disable); non-integer/negative falls back to default.
        let api_inflight_requests = env("E2B_API_INFLIGHT_REQUESTS")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1000);
```

and add `api_inflight_requests,` to the `Self { .. }` initializer.

In `crates/e2b-rs/src/api/client.rs` `ApiClient::new`, change the limiter line from `ConcurrencyLimiter::new(0)` to:

```rust
            limiter: ConcurrencyLimiter::new(config.api_inflight_requests),
```

- [ ] **Step 4: Verify the field + clients still pass**

Run: `cargo test -p e2b-rs connection_config` → PASS (10 tests now).
Run: `cargo test -p e2b-rs api::client` → still PASS (the default 1000 cap doesn't affect the single-request tests).

- [ ] **Step 5: Update the parity checklist**

In `docs/parity-checklist.md`, add a section:

```markdown
## REST transports (Plan 2b-i)

| JS (`src/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `api/index.ts` `ApiClient` + `validateApiKey` | `api::client::{ApiClient, validate_api_key}` | ✅ |
| `api/inflight.ts` `limitConcurrency` | `http::inflight::ConcurrencyLimiter` | ✅ |
| `envd/api.ts` client + `checkSandboxHealth` | `envd::rest::EnvdApiClient` (+ `check_health`) | ✅ |
| `api/index.ts` per-endpoint calls (createSandbox, …) | _(Plan 3+, built on `ApiClient::request`)_ | ⬜ |
| `envd/api.ts` `/files` read/write | _(Plan 3 Filesystem — multipart/octet-stream/gzip)_ | ⬜ |

Connect-over-JSON RPC client (filesystem/process/pty) is Plan 2b-ii.
```

- [ ] **Step 6: Full release gate**

Run each and confirm it passes:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` (report counts: foundation + codegen round-trips + the new inflight/api/envd/errors tests, 0 failures)
- `cargo test --doc -p e2b-rs`
- `cargo doc --no-deps -p e2b-rs`
- `cargo xtask codegen && git status --porcelain` → empty (codegen still idempotent under the `schema` names)

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/connection_config.rs crates/e2b-rs/src/api/client.rs docs/parity-checklist.md
git commit -m "feat(api): wire env-tuned inflight cap; document REST transport parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 2b-i is complete when:
- `gen` modules are renamed to `schema` and codegen remains idempotent.
- `ApiClient` (auth, `validate_api_key`, status→`Error` mapping with message extraction, env-tuned in-flight cap, logging, `health()`) and `EnvdApiClient` (sandbox headers, `check_health`) are implemented and `wiremock`-tested.
- `Error::Transport(#[from] reqwest::Error)` exists.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects the REST transports.

**Next:** Plan 2b-ii (Connect-over-JSON client) — the envelope codec (5-byte `>BI` framing, end-stream flag), unary calls (`application/json`), server-streaming (`application/connect+json` → `impl Stream`), the Connect `Code`→`Error` map, version gates (`envd/versions.rs`), the `Authorization: Basic` user header, and health-aware error handling — built against `ApiClient`/`EnvdApiClient` and the `envd::proto` types.
