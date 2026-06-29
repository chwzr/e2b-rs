# E2B Rust SDK (`e2b-rs`) — Design

**Status:** Approved design, pre-implementation
**Date:** 2026-06-28
**Reference:** `../E2B/packages/js-sdk` (the `e2b` npm package, v2.31.0, ~20.6K LOC TypeScript)

---

## 1. Goal & scope

Port the E2B JavaScript SDK to Rust as the crate **`e2b-rs`** (library name `e2b_rs`), targeting **1:1 feature parity**. Every public capability of the JS SDK has an equivalent in Rust, and the API is shaped so that a developer coming from the JS SDK recognizes the structure immediately (`sandbox.files.read`, `sandbox.commands.run`, `sandbox.git.clone`, `Template::build`, …), while the code reads as idiomatic async Rust.

### Non-goals
- Browser / WASM runtime support (the JS SDK supports Node/Deno/Bun/browser; `e2b-rs` targets native async Rust on tokio).
- Exposing the raw generated OpenAPI/protobuf types as public API (the JS SDK re-exports `components`/`paths`; in Rust these stay internal).
- A blocking/sync API surface in v1 (async-only; a blocking facade may be added later).

---

## 2. Background: what the JS SDK is

The SDK talks to **two backends**:

1. **Control-plane REST API** (`https://api.${domain}`) — sandbox lifecycle, snapshots, templates, volumes, tags, auth. Typed via OpenAPI (`openapi-fetch`).
2. **The `envd` daemon** inside every sandbox (port `49983`) — two protocols:
   - **ConnectRPC** for filesystem metadata ops + process/command + pty (server-streaming for run/connect/watch).
   - **A small REST surface** for file read/write (multipart & octet-stream upload, gzip) and `/health`.

**Critical transport fact:** the envd ConnectRPC transport is created with `useBinaryFormat: false` (`sandbox/index.ts:166`), i.e. the **Connect protocol over JSON** (proto3 JSON mapping), *not* gRPC and *not* binary protobuf. Unary calls are `POST {service}/{method}` with `Content-Type: application/json`; server-streaming uses `Content-Type: application/connect+json` with Connect's 5-byte envelope framing. This is confirmed by E2B's own hand-rolled Python `e2b_connect` client (`>BI` envelope header = 1 flag byte + 4-byte big-endian length; end-stream flag `0x02`).

**Consequence for Rust:** no `tonic`, no `h2`, no `protoc` at runtime. The envd RPC layer is a ~150-line Connect-over-JSON client on top of `reqwest` + `serde_json`.

### Feature surface (the parity target)

| Subsystem | Public surface |
|---|---|
| **Sandbox** | `create`, `connect`, `list` (paginated), `kill`, `pause`/`betaPause`, resume (via connect), `set_timeout`, `update_network`, `get_info`, `get_metrics`, `is_running`, `create_snapshot`/`list_snapshots`/`delete_snapshot`, `get_host`, `upload_url`/`download_url` (signed), MCP (`get_mcp_url`/`get_mcp_token`) |
| **Filesystem** | `read` (text/bytes/stream), `write`/`write_files` (multipart + octet-stream, gzip, metadata), `list`, `make_dir`, `rename`, `exists`, `get_info`, `remove`, `watch_dir` (streaming) |
| **Commands** | `run` (fg), `run_background` (bg), `connect`, `send_stdin`, `close_stdin`, `kill`, `list`; `CommandHandle` (`wait`, `disconnect`, `kill`, `send_stdin`, stdout/stderr, event stream) |
| **Pty** | `create`, `connect`, `send_input`, `resize`, `kill` (output via `CommandHandle::output()` → `CommandOutput::Pty`) |
| **Git** | `clone`, `init`, `add`, `commit`, `push`, `pull`, `remote_add`/`remote_get`, `reset`, `restore`, `create_branch`/`checkout_branch`/`delete_branch`/`branches`, `status`, `set_config`/`get_config`/`configure_user`, `dangerously_authenticate` (executes via Commands in-sandbox) |
| **Volume** | `create`/`connect`/`get_info`/`list`/`destroy` (static); `list`, `make_dir`, `get_info`, `exists`, `update_metadata`, `read_file` (text/bytes/stream), `write_file`, `remove` (instance) — separate volume-content REST API |
| **Template** | fluent builder (`from_*_image`/`from_dockerfile`/`from_template`/`from_{aws,gcp}_registry`, `copy`, `run_cmd`, `set_workdir`/`set_user`/`set_envs`, `pip_install`/`npm_install`/`bun_install`/`apt_install`, `git_clone`, `make_dir`/`make_symlink`/`remove`/`rename`, `add_mcp_server`, `set_ready_cmd`/`set_start_cmd`) + build pipeline + tags + `ReadyCmd` helpers |
| **Foundation** | `ConnectionConfig` + env resolution, `Error` hierarchy, `Logger`, `Paginator`, signed-URL SHA-256 signatures |

---

## 3. Locked decisions

| # | Decision | Choice | Rationale |
|---|---|---|---|
| D1 | Concurrency model | **Async-only** (tokio) | Matches JS Promise model; idiomatic for a streaming/network SDK; smallest surface. Blocking facade deferrable. |
| D2 | Parity scope | **Full surface in one pass** | Goal is 1:1; build order sequences risk (§12) but the target is the whole SDK. |
| D3 | Type generation | **Generate & vendor from `E2B/spec`** | Stays synced with spec changes; output committed so consumers need no toolchain. |
| D4 | API style | **Idiomatic Rust, familiar shape** | Same structure/names, snake_case, option-builders, typed reads, closures + Streams. |
| D5 | Crate name | package `e2b-rs`, lib `e2b_rs` | Per request. |
| D6 | Lints | **`deny` `unwrap_used` + `expect_used`** (lib); allowed in tests | Per request; `clippy.toml` `allow-{unwrap,expect}-in-tests = true`. Implies panic-free lib code (§11). |
| D7 | Builder finish | **Direct `.await`** (builder `impl IntoFuture`) | Closest to the JS feel: `Sandbox::create().template("x").await?`. |
| D8 | Generated-code lints | **Scoped `allow`** in vendored modules | The rule polices hand-written code; generated code gets a header `allow`. |
| D9 | Docs | **Inline `no_run` doctests on every public item; `#![deny(missing_docs)]`** (hand-written modules) | "As user-friendly as possible"; examples are compile-checked so they can't rot. |
| D10 | Repo / workspace | `e2b-rs` is the git repo root + Cargo workspace; **all crates (incl. `xtask`) under `crates/`** | Per request. Session runs in the parent dir for `E2B` reference access; the SDK repo is self-contained. |
| D11 | Streaming consumption | **`tokio::sync::mpsc` channels, not callbacks** | Per request: callbacks are unidiomatic in Rust. Event/output feeds become receivers; byte-body reads stay `Stream`. |
| D12 | Toolchain | **edition 2024, MSRV `rust-version = "1.95.0"`** | Per request; 1.95.0 is installed locally and supports edition 2024. |

---

## 4. Architecture

### 4.1 Workspace

The `e2b-rs` folder is the git repo root and the Cargo workspace root. **Every crate lives under `crates/`** (the published SDK and the dev-only codegen driver):

```
e2b-rs/                       # git repo root + Cargo workspace root
├── Cargo.toml                # [workspace] members = ["crates/*"]; shared [workspace.dependencies]/[workspace.lints]
├── clippy.toml               # allow-unwrap-in-tests / allow-expect-in-tests
├── rust-toolchain.toml       # pins stable 1.95.0
└── crates/
    ├── e2b-rs/               # the published SDK crate (package e2b-rs, lib e2b_rs)
    │   ├── Cargo.toml        # rust-version = "1.95.0", edition = "2024"; [lints] workspace = true
    │   └── src/...
    └── xtask/                # codegen driver (not published)
        └── src/main.rs
```

(Single SDK crate under `crates/` keeps room for future internal splits without a breaking move. The session's working dir is the *parent* of this repo so the `E2B` reference checkout stays reachable for codegen via `--spec-dir`.)

### 4.2 Module tree — mirrors `js-sdk/src/` for auditable parity

```
crates/e2b-rs/src/
├── lib.rs                    # re-exports (mirrors index.ts); crate-level //! quickstart
├── connection_config.rs      # ConnectionConfig + env resolution
├── errors.rs                 # Error enum + From/status mapping
├── logs.rs                   # Logger trait + middleware
├── paginator.rs              # Paginator
├── utils.rs                  # sha256, shell_quote, timeout_to_seconds, version/runtime
├── api/
│   ├── mod.rs                # ApiClient: auth, key validation, error map, inflight, logging
│   └── gen.rs                # VENDORED (progenitor ← openapi.yml)
├── connect/                  # Connect-over-JSON client (hand-written)
│   ├── mod.rs                # client, service/method table
│   ├── envelope.rs           # 5-byte framing codec
│   ├── unary.rs
│   ├── streaming.rs          # server-streaming → Stream
│   └── error.rs              # Connect Code → Error
├── envd/
│   ├── mod.rs
│   ├── rest.rs               # EnvdApiClient: /health, /files
│   ├── rest_gen.rs           # VENDORED (progenitor ← envd.yaml)
│   ├── versions.rs           # ENVD_* version gates (semver)
│   └── proto/                # VENDORED (prost + pbjson)
│       ├── filesystem.rs
│       └── process.rs
├── sandbox/
│   ├── mod.rs                # Sandbox struct + ctor wiring
│   ├── api.rs                # lifecycle (control-plane)
│   ├── network.rs            # ALL_TRAFFIC, selectors, rules, transforms
│   ├── signature.rs          # signed-URL SHA-256
│   ├── mcp.rs                # McpServer + mcp_gen.rs (VENDORED typify ← mcp-server.json)
│   ├── filesystem/
│   │   ├── mod.rs
│   │   └── watch_handle.rs
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── command_handle.rs
│   │   └── pty.rs
│   └── git/
│       ├── mod.rs
│       └── utils.rs
├── volume/
│   ├── mod.rs                # Volume API
│   ├── client.rs             # VolumeApiClient
│   ├── gen.rs                # VENDORED (progenitor ← openapi-volumecontent.yml)
│   └── types.rs
└── template/
    ├── mod.rs                # Template builder
    ├── build_api.rs          # build pipeline
    ├── dockerfile.rs         # Dockerfile parsing
    ├── readycmd.rs           # ReadyCmd helpers
    ├── logger.rs             # LogEntry, default_build_logger
    ├── types.rs
    └── utils.rs              # tar/gzip context, ignore/glob, hash
```

### 4.3 Transports (3, sharing one `reqwest::Client`)

1. **Control-plane REST** — `api::ApiClient` wraps `api/gen.rs`. Injects `X-API-KEY` and optional `Authorization: Bearer`; validates the `^e2b_[0-9a-f]+$` key pattern (`validate_api_key`, default on); maps HTTP status → `Error` (401→Authentication, 429→RateLimit, 502/unavailable→Timeout, 507→NotEnoughSpace, …).
2. **envd REST** — `EnvdApiClient` for `/health` and `/files` (GET read; POST write as multipart/form-data or `application/octet-stream`, optional gzip, `X-Metadata-*` headers). Sends `E2b-Sandbox-Id`/`E2b-Sandbox-Port` and `X-Access-Token`.
3. **envd Connect** — `connect::Client` (§8).

Cross-cutting concerns (the inflight concurrency semaphore, request logging via `Logger`, default headers/User-Agent) live in a `reqwest-middleware` stack shared by all three.

### 4.4 Dependency stack

| Concern | Crates |
|---|---|
| Runtime / HTTP | `tokio`, `reqwest` (json, stream, multipart, rustls-tls), `reqwest-middleware` |
| Serialization | `serde`, `serde_json` |
| Proto types (JSON) | `prost`, `pbjson`, `pbjson-types` |
| Streaming | `futures`, `bytes`, `async-stream`, `tokio-util`, `pin-project-lite` |
| Compression / archive | `async-compression` (gzip) / `flate2`, `tar`, `tempfile` |
| Build context | `ignore`, `globset`, `sha2`, `base64` |
| Errors / time / misc | `thiserror`, `time`, `url`, `semver`, `uuid`, `once_cell` |
| Dockerfile | `dockerfile-parser` (evaluate; fall back to a minimal hand parser) |
| Codegen (xtask only) | `prost-build`, `pbjson-build`, `progenitor`, `typify` |

**Toolchain:** edition 2024, `rust-version = "1.95.0"` (MSRV), pinned via `rust-toolchain.toml` and verified in CI. Channels/streams use `tokio` + `futures` types already in the stack.

---

## 5. Codegen & vendoring (`cargo xtask codegen`)

Mirrors the JS `generate:*` scripts. `xtask` reads `--spec-dir` (default `../E2B/spec`), runs the generators, prepends the scoped-`allow` header, `rustfmt`s, and writes the output into the crate. **All generated output is committed.** Consumers never run `xtask`.

| JS script | Source spec | Rust tool | Vendored output |
|---|---|---|---|
| `generate:api` | `openapi.yml` (tag-filtered to sandboxes/snapshots/templates/tags/auth/volumes) | progenitor | `src/api/gen.rs` |
| `generate:envd` | `envd/filesystem/*.proto`, `envd/process/*.proto` | prost + pbjson (**messages only**) | `src/envd/proto/{filesystem,process}.rs` |
| `generate:envd-api` | `envd/envd.yaml` | progenitor | `src/envd/rest_gen.rs` |
| `generate:volume-api` | `openapi-volumecontent.yml` | progenitor | `src/volume/gen.rs` |
| `generate:mcp` | `mcp-server.json` | typify | `src/sandbox/mcp_gen.rs` |

Notes:
- **Tag filter:** the JS build runs `spec/remove_extra_tags.py` to strip admin/node endpoints before generating. `xtask` reproduces this (port the small YAML transform, or shell out to the script when a Python interpreter is available); on failure it can generate from the full spec (extra endpoints are harmless dead code).
- **No gRPC stubs:** we generate proto *messages* only. The 13 envd service methods (6 filesystem: `stat`, `makeDir`, `move`, `listDir`, `remove`, `watchDir`; 7 process: `list`, `start`, `connect`, `update`, `sendInput`, `sendSignal`, `closeStdin`) and their streaming kinds are hand-mapped in `connect::mod` as a static table.
- **Generated-client wrapping:** the hand-written SDK wraps the progenitor clients, supplying the shared `reqwest::Client` (with auth/User-Agent baked in) and mapping progenitor's error into `Error` at the boundary.
- **Robustness fallback:** if progenitor can't digest a surface (complex `oneOf`, missing `operationId`, etc.), fall back to **typify (types only) + hand-written reqwest calls** for that surface. The SDK call logic is hand-written regardless, so this is low-risk and localized.

---

## 6. Public API design

### 6.1 Builders & `IntoFuture`

Configurable async entry points return an option-builder struct that `impl IntoFuture`, so the no-arg case and the configured case both read naturally:

```rust
let s = Sandbox::create().await?;                              // defaults
let s = Sandbox::create().template("base").timeout(Duration::from_secs(300)).await?;
```

Builders are hand-written `Option`-field structs with chainable setters and an `IntoFuture` impl that produces a `BoxFuture<'static, Result<T>>` (a small internal declarative macro reduces boilerplate). This avoids depending on a builder macro's IntoFuture support and keeps full control for the panic-free requirement.

Where one method would need two return types (JS `run({background})`), Rust uses two methods: `run` (→ `CommandResult`) and `run_background` (→ `CommandHandle`).

### 6.2 Error model

A single `thiserror` enum mirroring `errors.ts`; `matches!` replaces `instanceof`:

```rust
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")] Sandbox(String),
    #[error("{0}")] Timeout(String),
    #[error("{0}")] InvalidArgument(String),
    #[error("{0}")] NotEnoughSpace(String),
    #[error("{0}")] NotFound(String),
    #[error("{0}")] FileNotFound(String),
    #[error("{0}")] SandboxNotFound(String),
    #[error("{0}")] Authentication(String),
    #[error("{0}")] GitAuth(String),
    #[error("{0}")] GitUpstream(String),
    #[error("{0}")] Template(String),
    #[error("{0}")] RateLimit(String),
    #[error("{0}")] Build(String),
    #[error("{0}")] FileUpload(String),
    #[error("{0}")] Volume(String),
    #[error("command exited with code {exit_code}")]
    CommandExit { exit_code: i32, stdout: String, stderr: String, error: Option<String> },
    #[error(transparent)] Transport(#[from] reqwest::Error),
    #[error("internal error: {0}")] Internal(String),   // panic-free fallback for "impossible" cases (§11)
}
pub type Result<T> = std::result::Result<T, Error>;
```

`NotFound` is the deprecated base of `FileNotFound`/`SandboxNotFound` in JS; in Rust they are sibling variants, and helper predicates (e.g. `Error::is_not_found`) recover the "base class" grouping where the JS code relied on `instanceof NotFoundError`.

### 6.3 Per-subsystem shapes (representative)

```rust
// Sandbox
let sandbox = Sandbox::create().template("base").metadata([("k","v")]).await?;
let sandbox = Sandbox::connect(&id).await?;
let mut page = Sandbox::list();
while page.has_next() { for info in page.next_items().await? { /* ... */ } }
sandbox.set_timeout(Duration::from_secs(60)).await?;
let host = sandbox.get_host(3000);
sandbox.kill().await?;

// Filesystem
sandbox.files.write("hello.txt", "world").await?;
let text:  String  = sandbox.files.read("hello.txt").await?;       // default text
let bytes: Vec<u8> = sandbox.files.read_bytes("hello.txt").await?;
let stream         = sandbox.files.read_stream("big.bin").await?;   // impl Stream<Item=Result<Bytes>>
let entries = sandbox.files.list("/home/user").await?;
let watch = sandbox.files.watch_dir("/tmp").await?;                // WatchHandle
let mut events = watch.events();                                  // mpsc::Receiver<FilesystemEvent>
while let Some(ev) = events.recv().await { println!("{ev:?}"); }   // closes when watch ends
watch.stop().await?;

// Commands — foreground is fully buffered; background gives a channel + wait()
let r = sandbox.commands.run("echo hi").cwd("/app").await?;       // CommandResult
let mut h = sandbox.commands.run_background("npm test").await?;   // CommandHandle
let mut out = h.output();                                         // mpsc::Receiver<CommandOutput>
let pump = tokio::spawn(async move {
    while let Some(o) = out.recv().await {
        match o {
            CommandOutput::Stdout(s) => print!("{s}"),
            CommandOutput::Stderr(s) => eprint!("{s}"),
            CommandOutput::Pty(_)    => {}
        }
    }
});
h.send_stdin("data\n").await?;
let r = h.wait().await?;                                          // final CommandResult; channel now closed
pump.await.ok();

// Pty — output arrives as CommandOutput::Pty(Bytes) on the same channel
let pty = sandbox.pty.create().size(80, 24).await?;              // CommandHandle
let mut data = pty.output();                                     // mpsc::Receiver<CommandOutput>
sandbox.pty.send_input(pty.pid, b"ls\n").await?;
sandbox.pty.resize(pty.pid, 120, 40).await?;

// Git
sandbox.git.clone("https://github.com/x/y").path("/repo").await?;
let status = sandbox.git.status("/repo").await?;
sandbox.git.commit("/repo", "msg").author_name("A").author_email("a@b.c").await?;

// Volume
let vol = Volume::create("data").await?;
vol.write_file("/d/x.txt", "hi").await?;
let t = vol.read_file("/d/x.txt").await?;

// Template — grab the log channel before awaiting the build
let build = Template::build(
    Template::new().from_python_image("3.12").run_cmd("pip install numpy").set_workdir("/app"),
    "my-template",
);
let mut logs = build.logs();                                     // mpsc::Receiver<LogEntry>
tokio::spawn(async move { while let Some(e) = logs.recv().await { println!("{e}"); } });
let info = build.await?;                                          // BuildInfo
```

### 6.4 Streaming consumption — channels, not callbacks

The JS SDK uses optionally-async callbacks (`onStdout`, `onStderr`, `onData`, `onEvent`, `onBuildLogs`). `e2b-rs` replaces all of them with **`tokio::sync::mpsc` receivers**: the consumer holds a channel handle and `recv().await`s messages — no inversion of control, no `Send + 'static` closure constraints, composable with `tokio::select!`.

- **Command & Pty output:** `CommandHandle::output() -> mpsc::Receiver<CommandOutput>`, where
  ```rust
  pub enum CommandOutput { Stdout(String), Stderr(String), Pty(Bytes) }
  ```
  Commands emit `Stdout`/`Stderr`; PTYs emit `Pty` (raw bytes). The channel is created with the handle and **closes when the process ends** (the loop terminates naturally). `wait()` still returns the aggregated `CommandResult` — stdout/stderr are accumulated for parity whether or not the channel is drained.
- **Filesystem watch:** `WatchHandle::events() -> mpsc::Receiver<FilesystemEvent>`. The channel closes when the watch ends; a terminal error (the JS `onExit(err)`) is observable on the handle (`WatchHandle::stop()` returns the terminal `Result`, and the handle records any spontaneous error).
- **Build logs:** the build builder's `logs() -> mpsc::Receiver<LogEntry>`, called before `.await`. A `default_build_logger` helper consumes such a receiver and pretty-prints (TTY spinner + elapsed time) for parity with JS's default.

**Mechanism:** a background driver task (`tokio::spawn`) owns each underlying RPC/poll stream, parses it, and forwards messages into the channel. It is aborted on `stop()`/`disconnect()` or when the handle is dropped (the handle stores an `AbortHandle`). Handle control methods (`wait`/`kill`/`send_stdin`/`close_stdin`) take `&self`/`&mut self`, not `self`, so output can be drained on one task while another awaits `wait()` or issues `kill()`. Output channels are **unbounded** — matching JS's always-accumulate semantics — so a consumer that never drains cannot stall the driver.

**Byte-body reads are not channels.** `files.read_stream` and `volume.read_file` (stream mode) return `impl Stream<Item = Result<Bytes>>` directly — the canonical async-Rust shape for an HTTP response body: lazy, back-pressured by polling, and requiring no driver task.

**Config closures are unaffected.** The network selector (`SandboxNetworkSelector::Fn`) is a pure synchronous config function, not an event stream, and stays a closure (see §9).

### 6.5 Multi-format reads

JS `read(path, {format})` returns `string | Uint8Array | Blob | ReadableStream`. Rust splits into `read` (→ `String`, the default), `read_bytes` (→ `Vec<u8>`), `read_stream` (→ `impl Stream<Item = Result<Bytes>>`). `blob` is dropped (a Web type). Volume reads mirror this.

---

## 7. Connection config & env resolution

`ConnectionConfig` reproduces `connectionConfig.ts` exactly, including env precedence (explicit opt → env var → default):

- `E2B_API_KEY` → `api_key`
- `E2B_VALIDATE_API_KEY` → default `true` (`"false"` disables)
- `E2B_DOMAIN` → default `e2b.app`
- `E2B_API_URL` → default `https://api.${domain}` (debug: `http://localhost:3000`)
- `E2B_SANDBOX_URL` → default `https://sandbox.${domain}` or constructed host
- `E2B_DEBUG` → default `false`
- `E2B_ACCESS_TOKEN` → deprecated legacy auth
- Connection tuning: `E2B_API_CONNECTIONS`, `E2B_API_INFLIGHT_REQUESTS`, `E2B_ENVD_RPC_CONNECTIONS`, `E2B_ENVD_RPC_INFLIGHT_REQUESTS`, `E2B_ENVD_INFLIGHT_REQUESTS` (parsed positive-int, mirroring `api/metadata.ts`).

Constants: `REQUEST_TIMEOUT_MS = 60_000`, `DEFAULT_SANDBOX_TIMEOUT_MS = 300_000`, `KEEPALIVE_PING_INTERVAL_SEC = 50`, `envd_port = 49983`, `mcp_port = 50005`. Supported stable-host domains: `e2b.app`, `e2b.dev`, `e2b.pro`, `e2b-staging.dev`. Host format: `${port}-${sandbox_id}.${domain}` (debug: `localhost:${port}`).

**Signed URLs** (`signature.rs`): `signature_raw = "${path}:${operation}:${user}:${envd_access_token}[:${expiration}]"`, `sha256` → base64, strip trailing `=`, prefix `v1_`. Expiration is `floor(now_unix) + expiration_seconds`. Errors if no access token.

---

## 8. The Connect-over-JSON client

A self-contained module reproducing the subset of the Connect protocol the SDK uses (matching E2B's `e2b_connect`).

- **Endpoint:** `POST {envd_base_url}/{package}.{Service}/{Method}`.
- **Unary** (`stat`, `makeDir`, `move`, `listDir`, `remove`, `list`, `update`, `sendInput`, `sendSignal`, `closeStdin`): `Content-Type: application/json`; body = JSON request; 200 → JSON response; non-200 → Connect error JSON `{ "code", "message", "details" }` → `Error` via a `Code → Error` map (`invalid_argument`→InvalidArgument, `unauthenticated`→Authentication, `not_found`→NotFound, `resource_exhausted`→RateLimit, `unavailable`→Timeout-formatted, `canceled`/`deadline_exceeded`→Timeout).
- **Server-streaming** (`watchDir`, `start`, `connect`): `Content-Type: application/connect+json`; request body = one enveloped message; response = a sequence of envelopes. **Envelope** = 5-byte header (`flags: u8`, `len: u32` big-endian) + `len` bytes. Flag `0x01` = compressed; flag `0x02` = end-of-stream (payload is trailers/error JSON; empty/`{}` = clean end). Each non-end payload is a JSON message decoded into the prost+pbjson type and surfaced via `async_stream::stream!` as `impl Stream<Item = Result<T>>`.
- **Headers:** `Authorization: Basic base64(user:)` (version-gated by `ENVD_DEFAULT_USER`), `X-Access-Token`, `E2b-Sandbox-Id`/`E2b-Sandbox-Port`, `Keepalive-Ping-Interval`. Redirects followed.
- **Health-aware errors:** on a dropped connection mid-stream, probe `/health`; if the sandbox is dead, surface `Timeout` ("sandbox terminated"), else a transient transport error — mirroring `rpc.ts` `handleRpcErrorWithHealthCheck`.

Unit-tested at the byte level (envelope round-trip, multi-frame streams, end-stream trailers, error mapping) with `wiremock`.

---

## 9. Subsystem behavior notes (parity-critical details)

- **Filesystem.write** picks multipart/form-data by default; switches to `application/octet-stream` (streamed body) when given a stream or `use_octet_stream`, gated by `ENVD_OCTET_STREAM_UPLOAD` (0.5.7). gzip sets `Content-Encoding: gzip`. Metadata → `X-Metadata-*` (0.6.2). Streamed uploads bypass the request timeout.
- **Filesystem.read(stream)** holds a pooled connection; an idle-timeout (`stream_idle_timeout_ms`, default = request timeout, `0` disables) aborts unconsumed streams.
- **watch_dir** gates: recursive (`ENVD_VERSION_RECURSIVE_WATCH` 0.1.4), `include_entry` (0.6.3), `allow_network_mounts` (0.6.4). Events flow to the `events()` channel; the channel closes once when watching ends, and the terminal `Result` (clean end vs. error — JS `onExit`) is surfaced on the handle.
- **Commands.run** wraps `/bin/bash -l -c <cmd>`; non-zero exit → `Err(CommandExit{..})`. `stdin` gated by 0.3.0; `close_stdin` by `ENVD_ENVD_CLOSE` 0.5.2. stdout/stderr decoded with streaming UTF-8 (incremental, flush on close), accumulated for `wait()` and mirrored to the `output()` channel. Foreground `run` consumes the stream internally and returns a fully-buffered `CommandResult`; live output uses `run_background` + `output()`.
- **Pty.create** runs `/bin/bash -i -l`, injects `TERM`/`LANG`/`LC_ALL` if absent; raw output is delivered as `CommandOutput::Pty(Bytes)` on the handle's `output()` channel.
- **Git** builds `git [-C path] …` via `shell_quote` (shlex-equivalent), with `GIT_TERMINAL_PROMPT=0`. Credentials are inlined into the remote URL transiently for clone/push/pull then restored; auth/upstream failures detected by stderr regex → `GitAuth`/`GitUpstream`. `status` parses `--porcelain=1 -b`; `branches` parses branch listing.
- **Volume** read/write mirror Filesystem format handling; separate `Authorization: Bearer ${token}` API; `FILE_TIMEOUT_MS = 3_600_000` for streamed transfers.
- **Template build pipeline:** (1) `POST /v3/templates` → `{templateID, buildID}`; (2) per COPY layer, `GET …/files/{hash}` presence check, then stream a **gzip tar** of the matched context (spooled to a temp file via `tempfile`, deterministic file ordering, hash over relative path + mode + size + content + symlink target) to the presigned S3 URL via `PUT` with `Content-Length`; (3) `POST /v2/templates/{id}/builds/{buildID}` with steps; (4) poll `…/builds/{buildID}/status` with `logsOffset`, forwarding `LogEntry`s to the build's `logs()` channel until `ready`/`error`. `FILE_UPLOAD_TIMEOUT_MS = 3_600_000`. Dockerfile parsing extracts FROM/RUN/COPY/WORKDIR/USER/ENV/ARG/CMD/ENTRYPOINT (multi-stage → error). `ReadyCmd` helpers generate shell snippets (`wait_for_port` → `ss`, `wait_for_url` → `curl`, `wait_for_process` → `pgrep`, `wait_for_file` → `[ -f ]`, `wait_for_timeout` → `sleep`).
- **Network rules:** `SandboxNetworkSelector` is `enum { List(Vec<String>), Fn(Box<dyn Fn(SelectorContext) -> Vec<String> + Send + Sync>) }`; `ALL_TRAFFIC = "0.0.0.0/0"`. Rules accept map/ordered-map of `transform { headers }`. `update_network` is an atomic replace (`PUT …/network`).
- **Pagination:** cursor via `x-next-token` response header; `has_next` true iff token present; `next_items` errors when exhausted.
- **MCP:** `get_mcp_url` = `https://{host(mcp_port)}/mcp`; `get_mcp_token` reads `/etc/mcp-gateway/.token` or a cached `uuid`.

---

## 10. Documentation standard

- **Every public item** carries a `///` doc comment with a `# Example` `no_run` doctest using the hidden-async-wrapper pattern (`# async fn run() -> e2b_rs::Result<()> { … # Ok(()) # }`). `no_run` ⇒ `cargo test --doc` compiles (type-checks against the real API) but does not execute (no key/network needed).
- **Crate-level `//!`** and **module-level `//!`** docs provide a Quickstart mirroring the README.
- **`examples/`** holds runnable programs (port of `example.mts`), checked by `cargo build --examples`.
- Enforcement: `#![deny(missing_docs)]` on hand-written modules (generated modules exempt via scoped `allow`); `rustdoc::broken_intra_doc_links = "deny"`.

---

## 11. Lints & code quality

- `crates/e2b-rs/Cargo.toml`:
  ```toml
  [lints.clippy]
  unwrap_used = "deny"
  expect_used = "deny"
  [lints.rust]
  missing_docs = "deny"
  ```
- `clippy.toml` (workspace root):
  ```toml
  allow-unwrap-in-tests = true
  allow-expect-in-tests = true
  ```
- **Panic-free lib consequence:** no `unwrap`/`expect`/`panic!`/indexing-that-can-panic in hand-written lib code. Static regexes use `once_cell`/`OnceLock` and surface a construction failure as an internal `Error` variant rather than `expect`. Locks avoid poisoning concerns (`parking_lot` or careful handling). Slicing uses `get(..)`/`bytes`. The envelope codec and porcelain parsers return `Result` on malformed input instead of panicking.
- **Generated modules** carry a header `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::all, missing_docs)]`.

---

## 12. Build order (dependency-sequenced)

Each phase compiles and is testable before the next; within each, implementation is test-driven.

1. **Foundation** — crate/workspace skeleton + lints, `Error`, `ConnectionConfig` + env resolution, `Logger`, `utils`, `Paginator`, `xtask` skeleton.
2. **Codegen** — wire `xtask`; generate & vendor all five surfaces; confirm compilation.
3. **Transports** — `ApiClient` (auth/validation/error-map/inflight/logging), `EnvdApiClient`, `connect::Client` (envelope + unary + streaming + error map + version gates).
4. **Sandbox core** — full lifecycle, URL/host construction, signatures, network selectors, MCP, signed URLs; `Sandbox` wiring.
5. **envd I/O** — Filesystem (+`WatchHandle`), Commands (+`CommandHandle` streaming), Pty.
6. **Git** — all ops via Commands + porcelain parsing.
7. **Volume** — content client + Volume API + `volume_mounts`.
8. **Template** — builder + build pipeline + Dockerfile parser + `ReadyCmd` + logger + tags.
9. **Polish** — examples, README, full test port, parity checklist, CI.

---

## 13. Testing & parity verification

- **Unit (CI default, no network):** `wiremock` mocks the three HTTP surfaces. Cover env resolution, signature SHA-256, error mapping, paginator, network-selector resolution, `shell_quote`, dockerignore+hash determinism, `ReadyCmd` strings, git porcelain parsing, Connect envelope codec.
- **Integration (gated on `E2B_API_KEY`):** mirror the JS `tests/` tree case-for-case (`tests/sandbox`, `tests/filesystem`, `tests/commands`, `tests/template`, `tests/volume`, …). Skipped without a key. The JS tests are the behavioral contract.
- **Doctests:** `cargo test --doc` compiles every public example.
- **Parity checklist:** a committed table mapping every `index.ts` export → its `e2b-rs` equivalent + status.
- **CI gates** (mirror JS format/lint/typecheck/test): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (enforces no-unwrap/no-expect + missing_docs), `cargo test`, `cargo test --doc`, `cargo doc`.

---

## 14. Risks & mitigations

| Risk | Mitigation |
|---|---|
| progenitor fails on a spec surface | Per-surface fallback to typify (types) + hand-written calls. |
| proto3 JSON ↔ pbjson field-name/timestamp/bytes mismatch | Byte-level conformance tests against recorded envd responses; pbjson follows the canonical proto3 JSON mapping that connect-web emits. |
| `dockerfile-parser` crate gaps vs. `dockerfile-ast` | Evaluate early; fall back to a minimal hand parser for the supported instruction subset. |
| Driver-task lifecycle leaks (channels) | Each handle stores an `AbortHandle`; the driver is aborted on `stop`/`disconnect`/drop. Tested for no orphaned tasks. |
| Spec drift (separate repo) | `xtask --spec-dir` re-syncs from an E2B checkout; vendored output committed and reviewed. |
| Streamed request-body timeouts | Match JS: streamed uploads bypass the handshake timeout; rely on idle/overall caps. |

---

## 15. Open questions (non-blocking)

- Publish cadence / crates.io name reservation for `e2b-rs`.
- Whether to expose a `tracing` feature that bridges `Logger` to the `tracing` ecosystem (nice-to-have, not parity).
