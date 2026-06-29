# E2B Rust SDK — Connect-over-JSON Client (Plan 2b-ii) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the hand-rolled Connect-protocol-over-JSON RPC client the SDK uses to talk to the in-sandbox `envd` daemon — envelope codec, `Code`→`Error` mapping, version gates, unary calls, and server-streaming calls — so Plan 3 can build Filesystem/Commands/Pty on top.

**Architecture:** The envd RPC is the Connect protocol with JSON encoding (`useBinaryFormat: false`), NOT gRPC/binary — so this is plain HTTP POST + `serde_json` + a ~5-byte envelope codec on top of `reqwest`, with no `tonic`/`h2`/`protoc`. Unary calls `POST {base}/{pkg.Service}/{Method}` with `application/json`; server-streaming uses `application/connect+json` with length-prefixed envelopes decoded into an `impl Stream`. Generic `unary`/`server_stream` methods take the vendored `envd::proto` message types (which already serde-serialize via pbjson's proto3-JSON mapping). The protocol details are confirmed against E2B's own hand-rolled `e2b_connect` Python client.

**Tech Stack:** `reqwest` (json/stream), `serde_json`, `futures` + `async-stream` (streaming), `base64` (Basic auth), `semver` (version gates). All already present except `async-stream` and `semver`.

**Reference spec:** `.super/specs/2026-06-28-e2b-rust-sdk-design.md` §8 (Connect client). **JS source:** `../E2B/packages/js-sdk/src/envd/{rpc.ts,versions.ts}`. **Protocol reference:** `../E2B/packages/python-sdk/e2b_connect/client.py`.

## Milestone roadmap (context — Plans 1, 2a, 2b-i merged to `main`)

| Plan | Deliverable |
|---|---|
| 1, 2a, 2b-i | DONE, merged (foundation, codegen, REST transports) |
| **2b-ii — Connect client (this plan)** | envelope codec + `Code`→`Error` + version gates + unary + server-streaming; wiremock + byte-level tests |
| 3 — Sandbox & envd I/O | Filesystem/Commands/Pty (consume this client + `EnvdApiClient`) and the public `Sandbox` API |
| 4 — Git & Volume · 5 — Template & Polish | … |

## Global Constraints

- **Repo/workspace:** package `e2b-rs`, lib `e2b_rs`, crates under `crates/`. Edition 2024, MSRV 1.95.0.
- **Lints (panic-free lib):** `clippy::unwrap_used`/`expect_used`/`missing_docs` denied; allowed in tests. No `.unwrap()`/`.expect()`/`panic!` in non-test code. Use the project's idioms (let-chains, `?`, `.map_err`). Generated modules stay exempt.
- **Transport is internal:** the Connect client is `pub(crate)`; it gets real callers in Plan 3. Forward-looking `pub(crate)` items get a SCOPED `#[allow(dead_code)] // used by Plan 3` (item-scoped, never a blanket module allow) — matching the established pattern.
- **Generated types:** the vendored envd messages are at `crate::envd::proto::filesystem::*` and `crate::envd::proto::process::*` and serialize via pbjson (proto3 JSON). Generated control-plane/volume types are `api::schema` / `volume::schema`. Import generated `Error` types under qualified aliases; never `use ...::Error` bare.
- **Protocol facts (confirmed against `e2b_connect`):** 5-byte envelope header = `>BI` (1 byte flags + 4 bytes big-endian u32 length). Flags: `compressed = 0b01`, `end_stream = 0b10`. Unary: `Content-Type: application/json`, `connect-protocol-version: 1`, raw JSON body. Server-stream: `Content-Type: application/connect+json`, one enveloped request (flags 0), response = envelope stream; the `end_stream` envelope's payload is `{ "error"?: {code, message}, "metadata"?: {...} }`. Error JSON is `{code, message}`: an **integer** `code` maps via the HTTP-status table; a **string** `code` is the Connect code name directly.
- **Async model:** async-only on `tokio`; streaming returns `impl Stream<Item = Result<T>>`.
- **Commits:** conventional messages ending with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Run `cargo fmt --all` before every commit.

### File structure (this plan)

```
crates/e2b-rs/src/
├── envd/
│   ├── mod.rs            # MODIFY: + pub(crate) mod versions;
│   └── versions.rs       # ENVD_* version constants + version_gte (semver)
└── connect/
    ├── mod.rs            # pub(crate): re-exports + the 13 service/method PATHS
    ├── error.rs          # Code enum, code_from_http_status, parse_connect_error, map_code_to_error
    ├── envelope.rs       # EnvelopeFlags, encode_envelope, EnvelopeDecoder
    └── client.rs         # connect::Client (new, auth_header, unary, server_stream) + handle_rpc_error
```

---

### Task 1: envd version gates (`envd/versions.rs`)

Port `envd/versions.ts`: the feature-gate version constants + a semver `>=` check. Consumed by the Connect auth header (Task 4) and by Plan 3's filesystem/commands feature gating.

**Files:**
- Create: `crates/e2b-rs/src/envd/versions.rs`
- Modify: `crates/e2b-rs/src/envd/mod.rs`, `Cargo.toml` (+ `semver`), `crates/e2b-rs/Cargo.toml`

**Interfaces:**
- Produces: `pub(crate) const ENVD_VERSION_RECURSIVE_WATCH/COMMANDS_STDIN/DEFAULT_USER/ENVD_CLOSE/OCTET_STREAM_UPLOAD/FILE_METADATA/FS_EVENT_ENTRY_INFO/WATCH_NETWORK_MOUNTS: &str`; `pub(crate) fn version_gte(actual: &str, required: &str) -> bool` (true if `actual >= required`; lenient `false` on unparseable actual).

- [ ] **Step 1: Add the `semver` dependency**

In workspace `Cargo.toml` `[workspace.dependencies]`: `semver = "1"`. In `crates/e2b-rs/Cargo.toml` `[dependencies]`: `semver = { workspace = true }`.

- [ ] **Step 2: Write the failing test**

Create `crates/e2b-rs/src/envd/versions.rs`:

```rust
//! envd feature-gate versions (port of `envd/versions.ts`).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_constants_match_js() {
        assert_eq!(ENVD_DEFAULT_USER, "0.4.0");
        assert_eq!(ENVD_ENVD_CLOSE, "0.5.2");
        assert_eq!(ENVD_OCTET_STREAM_UPLOAD, "0.5.7");
    }

    #[test]
    fn version_gte_compares_semver() {
        assert!(version_gte("0.4.0", "0.4.0"));
        assert!(version_gte("0.5.2", "0.4.0"));
        assert!(version_gte("1.0.0", "0.6.4"));
        assert!(!version_gte("0.3.9", "0.4.0"));
        // Unparseable actual is treated as "too old" (false), never panics.
        assert!(!version_gte("not-a-version", "0.4.0"));
    }
}
```

Add to `crates/e2b-rs/src/envd/mod.rs`: `pub(crate) mod versions;`.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs envd::versions`
Expected: FAIL — symbols not found.

- [ ] **Step 4: Implement**

Insert above the test module:

```rust
/// Recursive directory watch (`recursive` on watchDir).
pub(crate) const ENVD_VERSION_RECURSIVE_WATCH: &str = "0.1.4";
/// `stdin` option on command start.
pub(crate) const ENVD_COMMANDS_STDIN: &str = "0.3.0";
/// Default-user (Basic auth) support.
pub(crate) const ENVD_DEFAULT_USER: &str = "0.4.0";
/// `closeStdin` RPC.
pub(crate) const ENVD_ENVD_CLOSE: &str = "0.5.2";
/// octet-stream uploads + gzip.
pub(crate) const ENVD_OCTET_STREAM_UPLOAD: &str = "0.5.7";
/// File metadata (xattr) on write.
pub(crate) const ENVD_FILE_METADATA: &str = "0.6.2";
/// `includeEntry` in filesystem watch events.
pub(crate) const ENVD_VERSION_FS_EVENT_ENTRY_INFO: &str = "0.6.3";
/// `allowNetworkMounts` on watch.
pub(crate) const ENVD_VERSION_WATCH_NETWORK_MOUNTS: &str = "0.6.4";

/// Return `true` if `actual >= required` (both semver). An unparseable `actual`
/// returns `false` (treated as too old) rather than panicking; `required` is
/// one of the constants above and is always valid.
pub(crate) fn version_gte(actual: &str, required: &str) -> bool {
    // Strip a leading `v` if present, then parse.
    let parse = |s: &str| semver::Version::parse(s.trim_start_matches('v'));
    match (parse(actual), parse(required)) {
        (Ok(a), Ok(r)) => a >= r,
        _ => false,
    }
}
```

- [ ] **Step 5: Verify & commit**

Run: `cargo test -p e2b-rs envd::versions` → PASS (2 tests). `cargo clippy --workspace --all-targets -- -D warnings` → clean.

```bash
cargo fmt --all
git add crates/e2b-rs/src/envd/versions.rs crates/e2b-rs/src/envd/mod.rs Cargo.toml crates/e2b-rs/Cargo.toml Cargo.lock
git commit -m "feat(envd): add version-gate constants and semver comparison" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Connect error mapping (`connect/error.rs`)

Port the error half of `e2b_connect` + `rpc.ts`'s `DEFAULT_ERROR_MAP`: the Connect `Code` enum, the HTTP-status→`Code` table, parsing a Connect error JSON `{code, message}`, and mapping a `Code` to the SDK's `errors::Error`.

**Files:**
- Create: `crates/e2b-rs/src/connect/mod.rs`, `crates/e2b-rs/src/connect/error.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (`pub(crate) mod connect;`)

**Interfaces:**
- Consumes: `crate::errors::{Error, format_sandbox_timeout_error}`.
- Produces:
  - `pub(crate) enum Code { Canceled, Unknown, InvalidArgument, DeadlineExceeded, NotFound, AlreadyExists, PermissionDenied, ResourceExhausted, FailedPrecondition, Aborted, OutOfRange, Unimplemented, Internal, Unavailable, DataLoss, Unauthenticated }` with `Code::from_name(&str) -> Code` and `Code::from_http_status(u16) -> Code`.
  - `pub(crate) fn parse_connect_error(status: u16, body: &[u8]) -> (Code, String)` — body is the response/end-stream JSON; an integer `code` uses the HTTP table, a string `code` uses the name, non-JSON falls back to `(from_http_status(status), <body-as-utf8>)`.
  - `pub(crate) fn map_code_to_error(code: Code, message: String) -> Error`.

- [ ] **Step 1: Write the failing tests**

Create `crates/e2b-rs/src/connect/mod.rs`:

```rust
//! Hand-rolled Connect-protocol-over-JSON client for the envd daemon.

pub(crate) mod error;
```

Create `crates/e2b-rs/src/connect/error.rs`:

```rust
//! Connect protocol error codes and mapping to the SDK error type.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::Error;

    #[test]
    fn code_from_name_and_http() {
        assert_eq!(Code::from_name("not_found"), Code::NotFound);
        assert_eq!(Code::from_name("resource_exhausted"), Code::ResourceExhausted);
        assert_eq!(Code::from_name("totally_unknown"), Code::Unknown);
        assert_eq!(Code::from_http_status(404), Code::NotFound);
        assert_eq!(Code::from_http_status(429), Code::ResourceExhausted);
        assert_eq!(Code::from_http_status(502), Code::Unavailable);
        assert_eq!(Code::from_http_status(418), Code::Unknown);
    }

    #[test]
    fn parse_connect_error_string_int_and_nonjson() {
        // String code → used directly.
        let (c, m) = parse_connect_error(500, br#"{"code":"not_found","message":"nope"}"#);
        assert_eq!(c, Code::NotFound);
        assert_eq!(m, "nope");
        // Integer code → mapped via HTTP table.
        let (c, m) = parse_connect_error(200, br#"{"code":429,"message":"slow down"}"#);
        assert_eq!(c, Code::ResourceExhausted);
        assert_eq!(m, "slow down");
        // Non-JSON body → fall back to (from_http_status, body text).
        let (c, m) = parse_connect_error(404, b"plain text error");
        assert_eq!(c, Code::NotFound);
        assert_eq!(m, "plain text error");
    }

    #[test]
    fn map_code_to_error_matches_js_default_map() {
        assert!(matches!(map_code_to_error(Code::InvalidArgument, "x".into()), Error::InvalidArgument(_)));
        assert!(matches!(map_code_to_error(Code::Unauthenticated, "x".into()), Error::Authentication(_)));
        assert!(matches!(map_code_to_error(Code::NotFound, "x".into()), Error::NotFound(_)));
        assert!(matches!(map_code_to_error(Code::ResourceExhausted, "x".into()), Error::RateLimit(_)));
        // Unavailable → sandbox-timeout-formatted Timeout.
        match map_code_to_error(Code::Unavailable, "boom".into()) {
            Error::Timeout(msg) => assert!(msg.contains("sandbox timeout")),
            other => panic!("expected Timeout, got {other:?}"),
        }
        assert!(matches!(map_code_to_error(Code::Canceled, "x".into()), Error::Timeout(_)));
        assert!(matches!(map_code_to_error(Code::DeadlineExceeded, "x".into()), Error::Timeout(_)));
        // Anything else → generic Sandbox error.
        assert!(matches!(map_code_to_error(Code::Internal, "x".into()), Error::Sandbox(_)));
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`: `pub(crate) mod connect;`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs connect::error`
Expected: FAIL — symbols not found.

- [ ] **Step 3: Implement**

Insert above the test module in `connect/error.rs`:

```rust
use crate::errors::{format_sandbox_timeout_error, Error};

/// Connect protocol status code (mirrors `e2b_connect.Code`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Code {
    Canceled,
    Unknown,
    InvalidArgument,
    DeadlineExceeded,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    ResourceExhausted,
    FailedPrecondition,
    Aborted,
    OutOfRange,
    Unimplemented,
    Internal,
    Unavailable,
    DataLoss,
    Unauthenticated,
}

impl Code {
    /// Parse a Connect code name (e.g. `"not_found"`); unknown names → [`Code::Unknown`].
    pub(crate) fn from_name(name: &str) -> Code {
        match name {
            "canceled" => Code::Canceled,
            "invalid_argument" => Code::InvalidArgument,
            "deadline_exceeded" => Code::DeadlineExceeded,
            "not_found" => Code::NotFound,
            "already_exists" => Code::AlreadyExists,
            "permission_denied" => Code::PermissionDenied,
            "resource_exhausted" => Code::ResourceExhausted,
            "failed_precondition" => Code::FailedPrecondition,
            "aborted" => Code::Aborted,
            "out_of_range" => Code::OutOfRange,
            "unimplemented" => Code::Unimplemented,
            "internal" => Code::Internal,
            "unavailable" => Code::Unavailable,
            "data_loss" => Code::DataLoss,
            "unauthenticated" => Code::Unauthenticated,
            _ => Code::Unknown,
        }
    }

    /// Map an HTTP status to a Connect code (mirrors `make_error_from_http_code`).
    pub(crate) fn from_http_status(status: u16) -> Code {
        match status {
            400 => Code::InvalidArgument,
            401 => Code::Unauthenticated,
            403 => Code::PermissionDenied,
            404 => Code::NotFound,
            409 => Code::AlreadyExists,
            413 | 429 => Code::ResourceExhausted,
            499 => Code::Canceled,
            500 => Code::Internal,
            501 | 505 => Code::Unimplemented,
            502 | 503 => Code::Unavailable,
            504 => Code::DeadlineExceeded,
            _ => Code::Unknown,
        }
    }
}

/// Parse a Connect error from a response/end-stream body. A JSON `{code, message}`
/// where `code` is an integer uses the HTTP table; a string `code` is the name;
/// a non-JSON body falls back to `(from_http_status(status), <body text>)`.
pub(crate) fn parse_connect_error(status: u16, body: &[u8]) -> (Code, String) {
    #[derive(serde::Deserialize)]
    struct Raw {
        code: Option<serde_json::Value>,
        #[serde(default)]
        message: String,
    }
    match serde_json::from_slice::<Raw>(body) {
        Ok(raw) => {
            let code = match raw.code {
                Some(serde_json::Value::String(s)) => Code::from_name(&s),
                Some(serde_json::Value::Number(n)) => {
                    Code::from_http_status(n.as_u64().unwrap_or(0) as u16)
                }
                _ => Code::from_http_status(status),
            };
            (code, raw.message)
        }
        Err(_) => (
            Code::from_http_status(status),
            String::from_utf8_lossy(body).into_owned(),
        ),
    }
}

/// Map a Connect [`Code`] + message to the SDK [`Error`]. Mirrors `rpc.ts`'s
/// `DEFAULT_ERROR_MAP`; codes not in that map become a generic [`Error::Sandbox`].
pub(crate) fn map_code_to_error(code: Code, message: String) -> Error {
    match code {
        Code::InvalidArgument => Error::InvalidArgument(message),
        Code::Unauthenticated => Error::Authentication(message),
        Code::NotFound => Error::NotFound(message),
        Code::ResourceExhausted => Error::RateLimit(message),
        Code::Unavailable => format_sandbox_timeout_error(message),
        Code::Canceled | Code::DeadlineExceeded => Error::Timeout(message),
        _ => Error::Sandbox(message),
    }
}
```

- [ ] **Step 4: Verify & commit**

Run: `cargo test -p e2b-rs connect::error` → PASS (3 tests). `cargo clippy --workspace --all-targets -- -D warnings` → clean (the `_ as u16` cast on a bounded `n.as_u64().unwrap_or(0)` may trip `cast_possible_truncation`; if so, replace with `u16::try_from(n.as_u64().unwrap_or(0)).unwrap_or(0)` and re-run).

```bash
cargo fmt --all
git add crates/e2b-rs/src/connect crates/e2b-rs/src/lib.rs
git commit -m "feat(connect): add Connect Code enum, error parsing, and Error mapping" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Envelope codec (`connect/envelope.rs`)

The 5-byte length-prefixed framing used by Connect streaming. Pure, byte-level, fully unit-testable.

**Files:**
- Create: `crates/e2b-rs/src/connect/envelope.rs`
- Modify: `crates/e2b-rs/src/connect/mod.rs` (`pub(crate) mod envelope;`)

**Interfaces:**
- Produces:
  - `pub(crate) const FLAG_COMPRESSED: u8 = 0b0000_0001;` `pub(crate) const FLAG_END_STREAM: u8 = 0b0000_0010;`
  - `pub(crate) fn encode_envelope(flags: u8, data: &[u8]) -> Vec<u8>` (5-byte `>BI` header + data).
  - `pub(crate) struct Frame { pub flags: u8, pub data: Vec<u8> }` with `pub(crate) fn is_end_stream(&self) -> bool`.
  - `pub(crate) struct EnvelopeDecoder` with `new()`, `push(&mut self, chunk: &[u8])`, `next_frame(&mut self) -> Option<Frame>` (returns a complete frame or `None` if more bytes are needed).

- [ ] **Step 1: Write the failing tests**

Create `crates/e2b-rs/src/connect/envelope.rs`:

```rust
//! Connect protocol message framing (5-byte length-prefixed envelopes).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_has_5_byte_be_header() {
        let env = encode_envelope(0, b"hi");
        // flags=0, len=2 (big-endian u32), then payload.
        assert_eq!(env, vec![0x00, 0x00, 0x00, 0x00, 0x02, b'h', b'i']);
        let end = encode_envelope(FLAG_END_STREAM, b"{}");
        assert_eq!(end[0], FLAG_END_STREAM);
    }

    #[test]
    fn decoder_yields_complete_frames_and_buffers_partials() {
        let mut dec = EnvelopeDecoder::new();
        let f1 = encode_envelope(0, b"one");
        let f2 = encode_envelope(FLAG_END_STREAM, b"{}");
        // Feed f1 split across two chunks + the start of f2.
        dec.push(&f1[..3]);
        assert!(dec.next_frame().is_none()); // header incomplete
        dec.push(&f1[3..]);
        let frame = dec.next_frame().expect("frame 1");
        assert_eq!(frame.data, b"one");
        assert!(!frame.is_end_stream());
        assert!(dec.next_frame().is_none()); // nothing buffered yet
        dec.push(&f2);
        let frame = dec.next_frame().expect("frame 2");
        assert_eq!(frame.data, b"{}");
        assert!(frame.is_end_stream());
        assert!(dec.next_frame().is_none());
    }

    #[test]
    fn decoder_handles_two_frames_in_one_chunk() {
        let mut dec = EnvelopeDecoder::new();
        let mut buf = encode_envelope(0, b"a");
        buf.extend(encode_envelope(0, b"bb"));
        dec.push(&buf);
        assert_eq!(dec.next_frame().expect("f1").data, b"a");
        assert_eq!(dec.next_frame().expect("f2").data, b"bb");
        assert!(dec.next_frame().is_none());
    }
}
```

Add to `crates/e2b-rs/src/connect/mod.rs`: `pub(crate) mod envelope;`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs connect::envelope`
Expected: FAIL — symbols not found.

- [ ] **Step 3: Implement**

Insert above the test module:

```rust
/// Envelope flag: payload is compressed.
pub(crate) const FLAG_COMPRESSED: u8 = 0b0000_0001;
/// Envelope flag: end-of-stream frame (payload is trailers/error JSON).
pub(crate) const FLAG_END_STREAM: u8 = 0b0000_0010;

const HEADER_LEN: usize = 5;

/// Encode one envelope: 5-byte header (`flags: u8` + `len: u32` big-endian) + `data`.
pub(crate) fn encode_envelope(flags: u8, data: &[u8]) -> Vec<u8> {
    let len = data.len() as u32;
    let mut out = Vec::with_capacity(HEADER_LEN + data.len());
    out.push(flags);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// A decoded Connect frame.
pub(crate) struct Frame {
    /// Envelope flags byte.
    pub flags: u8,
    /// Frame payload.
    pub data: Vec<u8>,
}

impl Frame {
    /// Whether this is the end-of-stream frame.
    pub(crate) fn is_end_stream(&self) -> bool {
        self.flags & FLAG_END_STREAM != 0
    }
}

/// Incrementally decodes envelopes from a byte stream. Push response chunks,
/// then pull complete [`Frame`]s; partial frames stay buffered.
pub(crate) struct EnvelopeDecoder {
    buf: Vec<u8>,
}

impl EnvelopeDecoder {
    /// Create an empty decoder.
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Append received bytes.
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Return the next complete frame, or `None` if more bytes are needed.
    pub(crate) fn next_frame(&mut self) -> Option<Frame> {
        if self.buf.len() < HEADER_LEN {
            return None;
        }
        let flags = self.buf[0];
        let len = u32::from_be_bytes([self.buf[1], self.buf[2], self.buf[3], self.buf[4]]) as usize;
        if self.buf.len() < HEADER_LEN + len {
            return None;
        }
        let data = self.buf[HEADER_LEN..HEADER_LEN + len].to_vec();
        self.buf.drain(..HEADER_LEN + len);
        Some(Frame { flags, data })
    }
}
```

- [ ] **Step 4: Verify & commit**

Run: `cargo test -p e2b-rs connect::envelope` → PASS (3 tests). `cargo clippy --workspace --all-targets -- -D warnings` → clean (the `data.len() as u32` cast is bounded by frame size; if clippy flags it, use `u32::try_from(data.len()).unwrap_or(u32::MAX)`).

```bash
cargo fmt --all
git add crates/e2b-rs/src/connect
git commit -m "feat(connect): add 5-byte envelope codec with streaming decoder" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Connect client + unary calls (`connect/client.rs`)

The client: a configured `reqwest::Client`, the version-gated `Authorization: Basic` user header, the service/method paths, and the `unary` call. Server-streaming is Task 5.

**Files:**
- Create: `crates/e2b-rs/src/connect/client.rs`
- Modify: `crates/e2b-rs/src/connect/mod.rs` (`pub(crate) mod client;` + the path constants)

**Interfaces:**
- Consumes: `crate::errors::{Error, Result}`, `crate::logs::Logger`, `crate::envd::versions::{ENVD_DEFAULT_USER, version_gte}`, `connect::error::{parse_connect_error, map_code_to_error}`.
- Produces:
  - In `connect/mod.rs`: `pub(crate)` path consts, e.g. `FS_STAT: &str = "/filesystem.Filesystem/Stat"`, `FS_MAKE_DIR`, `FS_MOVE`, `FS_LIST_DIR`, `FS_REMOVE`, `FS_WATCH_DIR`, `PROC_LIST`, `PROC_UPDATE`, `PROC_SEND_INPUT`, `PROC_SEND_SIGNAL`, `PROC_CLOSE_STDIN`, `PROC_START`, `PROC_CONNECT`.
  - `pub(crate) struct ConnectClient` with `pub(crate) fn new(opts: ConnectClientOpts) -> Result<Self>` and `pub(crate) async fn unary<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(&self, path: &str, req: &Req, user: Option<&str>) -> Result<Resp>`.
  - `pub(crate) struct ConnectClientOpts { base_url, access_token: Option<String>, sandbox_id, envd_port: u16, user_agent, envd_version: String, request_timeout_ms: u64, logger: Option<Arc<dyn Logger>>, proxy: Option<String> }`.
  - `pub(crate) fn auth_header(envd_version: &str, user: Option<&str>) -> Option<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>`.

- [ ] **Step 1: Add the path constants**

In `crates/e2b-rs/src/connect/mod.rs`, add (and `pub(crate) mod client;`):

```rust
pub(crate) mod client;

/// Connect service/method paths (`/{package}.{Service}/{Method}`).
pub(crate) const FS_STAT: &str = "/filesystem.Filesystem/Stat";
pub(crate) const FS_MAKE_DIR: &str = "/filesystem.Filesystem/MakeDir";
pub(crate) const FS_MOVE: &str = "/filesystem.Filesystem/Move";
pub(crate) const FS_LIST_DIR: &str = "/filesystem.Filesystem/ListDir";
pub(crate) const FS_REMOVE: &str = "/filesystem.Filesystem/Remove";
pub(crate) const FS_WATCH_DIR: &str = "/filesystem.Filesystem/WatchDir";
pub(crate) const PROC_LIST: &str = "/process.Process/List";
pub(crate) const PROC_UPDATE: &str = "/process.Process/Update";
pub(crate) const PROC_SEND_INPUT: &str = "/process.Process/SendInput";
pub(crate) const PROC_SEND_SIGNAL: &str = "/process.Process/SendSignal";
pub(crate) const PROC_CLOSE_STDIN: &str = "/process.Process/CloseStdin";
pub(crate) const PROC_START: &str = "/process.Process/Start";
pub(crate) const PROC_CONNECT: &str = "/process.Process/Connect";
```

- [ ] **Step 2: Write the failing tests**

Create `crates/e2b-rs/src/connect/client.rs`:

```rust
//! Connect-over-JSON client: unary + (Task 5) server-streaming calls.

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn opts_for(server: &MockServer) -> ConnectClientOpts {
        ConnectClientOpts {
            base_url: server.uri(),
            access_token: Some("tok".to_string()),
            sandbox_id: "sbx".to_string(),
            envd_port: 49983,
            user_agent: "e2b-rs/test".to_string(),
            envd_version: "0.6.0".to_string(),
            request_timeout_ms: 5_000,
            logger: None,
            proxy: None,
        }
    }

    #[test]
    fn auth_header_is_basic_for_modern_envd() {
        // 0.6.0 >= 0.4.0 → Basic base64("user:") for the default user.
        let (name, value) = auth_header("0.6.0", None).expect("auth header");
        assert_eq!(name.as_str(), "authorization");
        // base64("user:") = "dXNlcjo="
        assert_eq!(value.to_str().unwrap_or(""), "Basic dXNlcjo=");
        // Explicit user.
        let (_, v2) = auth_header("0.6.0", Some("root")).expect("auth header");
        assert_eq!(v2.to_str().unwrap_or(""), format!("Basic {}", base64_user("root")));
    }

    // Helper mirroring the impl for the test's expectation.
    fn base64_user(u: &str) -> String {
        use base64::prelude::{Engine as _, BASE64_STANDARD};
        BASE64_STANDARD.encode(format!("{u}:"))
    }

    #[tokio::test]
    async fn unary_posts_json_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/process.Process/List"))
            .and(header("content-type", "application/json"))
            .and(header("connect-protocol-version", "1"))
            .and(header("X-Access-Token", "tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"echoed": true})))
            .mount(&server)
            .await;

        let client = ConnectClient::new(opts_for(&server)).expect("client");
        let req = serde_json::json!({"ping": 1});
        let resp: serde_json::Value = client.unary(super::super::PROC_LIST, &req, None).await.expect("unary ok");
        assert_eq!(resp["echoed"], serde_json::Value::Bool(true));
    }

    #[tokio::test]
    async fn unary_maps_connect_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/Stat"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "code": "not_found", "message": "no such file"
            })))
            .mount(&server)
            .await;
        let client = ConnectClient::new(opts_for(&server)).expect("client");
        let err = client
            .unary::<_, serde_json::Value>(super::super::FS_STAT, &serde_json::json!({}), None)
            .await
            .unwrap_err();
        match err {
            crate::errors::Error::NotFound(m) => assert!(m.contains("no such file")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs connect::client`
Expected: FAIL — symbols not found.

- [ ] **Step 4: Implement**

Insert above the test module:

```rust
use std::sync::Arc;
use std::time::Duration;

use base64::prelude::{Engine as _, BASE64_STANDARD};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::connect::error::{map_code_to_error, parse_connect_error};
use crate::envd::versions::{version_gte, ENVD_DEFAULT_USER};
use crate::errors::{Error, Result};
use crate::logs::Logger;

/// Default sandbox user for Basic auth (matches `defaultUsername`).
const DEFAULT_USER: &str = "user";

/// Options for constructing a [`ConnectClient`].
pub(crate) struct ConnectClientOpts {
    /// Base URL of the sandbox envd RPC surface.
    pub base_url: String,
    /// envd access token (`X-Access-Token`).
    pub access_token: Option<String>,
    /// Sandbox id (`E2b-Sandbox-Id`).
    pub sandbox_id: String,
    /// envd port (`E2b-Sandbox-Port`).
    pub envd_port: u16,
    /// `User-Agent` header.
    pub user_agent: String,
    /// envd version (gates the auth header).
    pub envd_version: String,
    /// Per-request timeout (ms).
    pub request_timeout_ms: u64,
    /// Optional logger.
    pub logger: Option<Arc<dyn Logger>>,
    /// Optional proxy URL.
    pub proxy: Option<String>,
}

/// The `Authorization: Basic base64("{user}:")` header, version-gated by
/// `ENVD_DEFAULT_USER`. For envd < 0.4.0 (no default-user support) returns
/// `None` unless an explicit user is given. Mirrors `authenticationHeader`.
pub(crate) fn auth_header(
    envd_version: &str,
    user: Option<&str>,
) -> Option<(HeaderName, HeaderValue)> {
    let username = match (user, version_gte(envd_version, ENVD_DEFAULT_USER)) {
        (Some(u), _) => u,
        (None, true) => DEFAULT_USER,
        (None, false) => return None,
    };
    let value = format!("Basic {}", BASE64_STANDARD.encode(format!("{username}:")));
    let header = HeaderValue::from_str(&value).ok()?;
    Some((reqwest::header::AUTHORIZATION, header))
}

/// Connect-over-JSON RPC client for the envd daemon.
pub(crate) struct ConnectClient {
    http: reqwest::Client,
    base_url: String,
    envd_version: String,
    request_timeout: Duration,
    logger: Option<Arc<dyn Logger>>,
}

impl ConnectClient {
    /// Build the client, baking the sandbox/access/user-agent headers into the
    /// underlying `reqwest::Client` as default headers.
    pub(crate) fn new(opts: ConnectClientOpts) -> Result<Self> {
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
            let p = reqwest::Proxy::all(proxy)
                .map_err(|e| Error::InvalidArgument(format!("invalid proxy URL {proxy:?}: {e}")))?;
            builder = builder.proxy(p);
        }
        let http = builder.build()?;

        Ok(Self {
            http,
            base_url: opts.base_url.trim_end_matches('/').to_string(),
            envd_version: opts.envd_version,
            request_timeout: Duration::from_millis(opts.request_timeout_ms),
            logger: opts.logger,
        })
    }

    /// Make a unary Connect call: `POST {base}{path}` with `application/json`,
    /// returning the decoded `Resp`. Maps a non-2xx response to a typed error.
    pub(crate) async fn unary<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        path: &str,
        req: &Req,
        user: Option<&str>,
    ) -> Result<Resp> {
        let url = format!("{}{path}", self.base_url);
        if let Some(logger) = &self.logger {
            logger.debug(&format!("POST {url}"));
        }
        let body = serde_json::to_vec(req)
            .map_err(|e| Error::Internal(format!("failed to encode request for {path}: {e}")))?;

        let mut rb = self
            .http
            .post(&url)
            .timeout(self.request_timeout)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("connect-protocol-version", "1")
            .body(body);
        if let Some((name, value)) = auth_header(&self.envd_version, user) {
            rb = rb.header(name, value);
        }

        let resp = rb.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let (code, message) = parse_connect_error(status.as_u16(), &bytes);
            return Err(map_code_to_error(code, message));
        }
        serde_json::from_slice::<Resp>(&bytes)
            .map_err(|e| Error::Internal(format!("failed to decode response from {path}: {e}")))
    }
}
```

- [ ] **Step 5: Verify & commit**

Run: `cargo test -p e2b-rs connect::client` → PASS (3 tests). `cargo clippy --workspace --all-targets -- -D warnings` → clean (if the `put` closure trips a borrow lint, mirror `envd/rest.rs`'s direct `if let` form). `cargo fmt --all --check`.

```bash
cargo fmt --all
git add crates/e2b-rs/src/connect
git commit -m "feat(connect): add ConnectClient with version-gated Basic auth and unary calls" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Server-streaming calls

Add `server_stream` to `ConnectClient`: POST `application/connect+json` with one enveloped request, decode the response envelope stream into an `impl Stream<Item = Result<Resp>>`, handling the end-stream frame (clean end vs. error).

**Files:**
- Modify: `crates/e2b-rs/src/connect/client.rs`
- Modify: `Cargo.toml` (+ `async-stream`), `crates/e2b-rs/Cargo.toml`

**Interfaces:**
- Consumes: `connect::envelope::{encode_envelope, EnvelopeDecoder, FLAG_END_STREAM}`, `futures::StreamExt`.
- Produces: `pub(crate) async fn ConnectClient::server_stream<Req: Serialize, Resp: DeserializeOwned + 'static>(&self, path: &str, req: &Req, user: Option<&str>) -> Result<impl futures::Stream<Item = Result<Resp>>>`.

- [ ] **Step 1: Add `async-stream`**

In workspace `Cargo.toml` `[workspace.dependencies]`: `async-stream = "0.3"`. In `crates/e2b-rs/Cargo.toml` `[dependencies]`: `async-stream = { workspace = true }`.

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `connect/client.rs`:

```rust
    #[tokio::test]
    async fn server_stream_decodes_enveloped_messages_until_end() {
        use futures::StreamExt as _;
        use crate::connect::envelope::{encode_envelope, FLAG_END_STREAM};

        // Build a Connect streaming body: two message frames + a clean end-stream frame.
        let mut body = encode_envelope(0, br#"{"n":1}"#);
        body.extend(encode_envelope(0, br#"{"n":2}"#));
        body.extend(encode_envelope(FLAG_END_STREAM, b"{}"));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/process.Process/Start"))
            .and(header("content-type", "application/connect+json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;

        let client = ConnectClient::new(opts_for(&server)).expect("client");
        let stream = client
            .server_stream::<_, serde_json::Value>(super::super::PROC_START, &serde_json::json!({}), None)
            .await
            .expect("stream opened");
        futures::pin_mut!(stream);
        let mut ns = Vec::new();
        while let Some(item) = stream.next().await {
            ns.push(item.expect("frame ok")["n"].as_i64().unwrap_or(0));
        }
        assert_eq!(ns, vec![1, 2]);
    }

    #[tokio::test]
    async fn server_stream_surfaces_end_stream_error() {
        use futures::StreamExt as _;
        use crate::connect::envelope::{encode_envelope, FLAG_END_STREAM};

        let mut body = encode_envelope(0, br#"{"n":1}"#);
        body.extend(encode_envelope(
            FLAG_END_STREAM,
            br#"{"error":{"code":"not_found","message":"gone"}}"#,
        ));
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/process.Process/Start"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;
        let client = ConnectClient::new(opts_for(&server)).expect("client");
        let stream = client
            .server_stream::<_, serde_json::Value>(super::super::PROC_START, &serde_json::json!({}), None)
            .await
            .expect("stream opened");
        futures::pin_mut!(stream);
        // First item: the data frame {"n":1} (Ok).
        let first = stream.next().await.expect("first item").expect("first ok");
        assert_eq!(first["n"].as_i64().unwrap_or(0), 1);
        // Second item: the end-stream error → Err(NotFound).
        let second = stream.next().await.expect("second item");
        assert!(matches!(second, Err(crate::errors::Error::NotFound(_))));
        // Stream ends after the error frame.
        assert!(stream.next().await.is_none());
    }
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p e2b-rs connect::client::tests::server_stream`
Expected: FAIL — `server_stream` not found.

- [ ] **Step 4: Implement**

Add to `impl ConnectClient`:

```rust
    /// Make a server-streaming Connect call: `POST {base}{path}` with
    /// `application/connect+json` and a single enveloped request; decode the
    /// response envelope stream into messages. The end-stream frame ends the
    /// stream (or yields a final `Err` if it carries an error).
    pub(crate) async fn server_stream<Req: Serialize, Resp: DeserializeOwned + 'static>(
        &self,
        path: &str,
        req: &Req,
        user: Option<&str>,
    ) -> Result<impl futures::Stream<Item = Result<Resp>>> {
        use crate::connect::envelope::{encode_envelope, EnvelopeDecoder};
        use futures::StreamExt as _;

        let url = format!("{}{path}", self.base_url);
        if let Some(logger) = &self.logger {
            logger.debug(&format!("POST {url} (stream)"));
        }
        let encoded = serde_json::to_vec(req)
            .map_err(|e| Error::Internal(format!("failed to encode request for {path}: {e}")))?;
        let body = encode_envelope(0, &encoded);

        let mut rb = self
            .http
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/connect+json")
            .header("connect-protocol-version", "1")
            .body(body);
        if let Some((name, value)) = auth_header(&self.envd_version, user) {
            rb = rb.header(name, value);
        }

        let resp = rb.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let bytes = resp.bytes().await?;
            let (code, message) = parse_connect_error(status.as_u16(), &bytes);
            return Err(map_code_to_error(code, message));
        }

        let mut bytes_stream = resp.bytes_stream();
        let stream = async_stream::try_stream! {
            let mut decoder = EnvelopeDecoder::new();
            while let Some(chunk) = bytes_stream.next().await {
                let chunk = chunk?; // reqwest::Error -> Error::Transport
                decoder.push(&chunk);
                while let Some(frame) = decoder.next_frame() {
                    if frame.is_end_stream() {
                        // End-of-stream: payload may carry `{ "error": {code, message} }`.
                        if let Some(err) = end_stream_error(&frame.data) {
                            Err(err)?;
                        }
                        return;
                    }
                    let msg: Resp = serde_json::from_slice(&frame.data)
                        .map_err(|e| Error::Internal(format!("failed to decode stream frame from {path}: {e}")))?;
                    yield msg;
                }
            }
        };
        Ok(stream)
    }
```

And add this free function below `impl ConnectClient` (parses the end-stream frame's optional error):

```rust
/// Parse a Connect end-of-stream frame; returns an [`Error`] if it carries one.
fn end_stream_error(data: &[u8]) -> Option<Error> {
    #[derive(serde::Deserialize)]
    struct EndStream {
        error: Option<serde_json::Value>,
    }
    let parsed = serde_json::from_slice::<EndStream>(data).ok()?;
    let err = parsed.error?;
    // The nested error is `{code, message}`; reuse the unary error parser.
    let bytes = serde_json::to_vec(&err).ok()?;
    let (code, message) = parse_connect_error(200, &bytes);
    Some(map_code_to_error(code, message))
}
```

- [ ] **Step 5: Verify & commit**

Run: `cargo test -p e2b-rs connect::client` → PASS (5 tests). `cargo clippy --workspace --all-targets -- -D warnings` → clean. `cargo fmt --all --check`.

```bash
cargo fmt --all
git add crates/e2b-rs/src/connect Cargo.toml crates/e2b-rs/Cargo.toml Cargo.lock
git commit -m "feat(connect): add server-streaming calls decoding the envelope stream" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Health-aware error helper, parity checklist & full gate

Add `handle_rpc_error` (port of `handleRpcErrorWithHealthCheck`): when an RPC fails with a connection-terminated/transport error, probe sandbox health to distinguish a killed sandbox (→ `Timeout`) from a transient failure. Then update the parity checklist and run the gate. (Plan 3 applies this helper around its filesystem/commands calls.)

**Files:**
- Modify: `crates/e2b-rs/src/connect/client.rs` (or a small `connect/health.rs`)
- Modify: `docs/parity-checklist.md`

**Interfaces:**
- Consumes: `crate::errors::Error`.
- Produces: `pub(crate) async fn handle_rpc_error<F, Fut>(err: Error, check_health: F) -> Error where F: FnOnce() -> Fut, Fut: std::future::Future<Output = Option<bool>>` — if `err` is a transport error and the health probe returns `Some(false)` (sandbox responded unhealthy → killed), returns a sandbox-timeout `Error::Timeout`; otherwise returns `err` unchanged.

- [ ] **Step 1: Write the failing test**

Add to `connect/client.rs` tests:

```rust
    #[tokio::test]
    async fn handle_rpc_error_converts_transport_error_when_sandbox_dead() {
        // A transport error + health probe says "responded unhealthy" (Some(false)) → Timeout.
        let transport_err = {
            let e = reqwest::get("http://127.0.0.1:1/x").await.unwrap_err();
            crate::errors::Error::Transport(e)
        };
        let out = handle_rpc_error(transport_err, || async { Some(false) }).await;
        assert!(matches!(out, crate::errors::Error::Timeout(_)));

        // Same transport error but health unknown (None) → unchanged.
        let transport_err2 = {
            let e = reqwest::get("http://127.0.0.1:1/x").await.unwrap_err();
            crate::errors::Error::Transport(e)
        };
        let out2 = handle_rpc_error(transport_err2, || async { None }).await;
        assert!(matches!(out2, crate::errors::Error::Transport(_)));

        // A non-transport error is returned unchanged without probing.
        let out3 = handle_rpc_error(crate::errors::Error::NotFound("x".into()), || async {
            panic!("should not probe");
        })
        .await;
        assert!(matches!(out3, crate::errors::Error::NotFound(_)));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p e2b-rs connect::client::tests::handle_rpc_error`
Expected: FAIL — `handle_rpc_error` not found.

- [ ] **Step 3: Implement**

Add below `impl ConnectClient` in `connect/client.rs`:

```rust
use crate::errors::format_sandbox_timeout_error;

/// Refine an RPC error using a sandbox health probe. If `err` is a transport
/// error and the probe reports the sandbox responded unhealthy (`Some(false)`,
/// i.e. confirmed killed/timed-out), convert it to a sandbox-timeout
/// [`Error::Timeout`]; otherwise (transient `None`, or a non-transport error)
/// return `err` unchanged. Mirrors `handleRpcErrorWithHealthCheck`.
pub(crate) async fn handle_rpc_error<F, Fut>(err: Error, check_health: F) -> Error
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<bool>>,
{
    if matches!(err, Error::Transport(_)) {
        if let Some(false) = check_health().await {
            return format_sandbox_timeout_error("the sandbox is no longer running");
        }
    }
    err
}
```

- [ ] **Step 4: Verify the helper test**

Run: `cargo test -p e2b-rs connect::client::tests::handle_rpc_error` → PASS. (The `panic!` in the no-probe arm is test-only and only fires if the helper wrongly probes.)

- [ ] **Step 5: Update the parity checklist**

In `docs/parity-checklist.md`, add:

```markdown
## Connect-over-JSON RPC client (Plan 2b-ii)

| JS (`src/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `envd/versions.ts` | `envd::versions` (constants + `version_gte`) | ✅ |
| `envd/rpc.ts` `Code`/`DEFAULT_ERROR_MAP` | `connect::error` (`Code`, `parse_connect_error`, `map_code_to_error`) | ✅ |
| Connect envelope framing | `connect::envelope` (`encode_envelope`, `EnvelopeDecoder`) | ✅ |
| `createConnectTransport` unary | `connect::client::ConnectClient::unary` | ✅ |
| `createConnectTransport` server-streaming | `ConnectClient::server_stream` (→ `impl Stream`) | ✅ |
| `authenticationHeader` | `connect::client::auth_header` (Basic, version-gated) | ✅ |
| `handleRpcErrorWithHealthCheck` | `connect::client::handle_rpc_error` | ✅ |
| Filesystem/Process/Pty RPC wrappers | _(Plan 3 — built on `ConnectClient` + the proto types)_ | ⬜ |

> Deferred parity detail: the JS transport sets a `Keepalive-Ping-Interval` header on streaming requests (`KEEPALIVE_PING_HEADER`/`KEEPALIVE_PING_INTERVAL_SEC`, already defined in `connection_config`). The stream decoder works without it (it's a server-side connection-liveness hint); add it to `server_stream`'s request headers in Plan 3 once confirmed against `sandbox/index.ts`.
```

- [ ] **Step 6: Full release gate**

Run each and confirm it passes:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` (report counts: prior 44 + the new versions/connect tests, 0 failures)
- `cargo test --doc -p e2b-rs`
- `cargo doc --no-deps -p e2b-rs`
- `cargo xtask codegen && git status --porcelain` → empty (codegen still idempotent)

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/connect docs/parity-checklist.md
git commit -m "feat(connect): add health-aware RPC error handling; document Connect parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 2b-ii is complete when:
- `connect::{error, envelope, client}` and `envd::versions` are implemented and tested (byte-level codec, `Code`/error mapping, version gates, plus wiremock unary + server-streaming + error-mapping tests).
- `ConnectClient` exposes generic `unary` and `server_stream` over the `envd::proto` message types, with the version-gated Basic auth header and the 13 service/method paths.
- `handle_rpc_error` converts a transport error to a sandbox-timeout when the health probe confirms the sandbox is gone.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects the Connect client.

**Next:** Plan 3 (Sandbox & envd I/O) — the headline milestone. It wires the control-plane `ApiClient` into the `Sandbox` lifecycle (create/connect/list/kill/pause/resume/setTimeout/getInfo/getMetrics) and builds Filesystem (read/write/list/watch), Commands (run/handle/streaming via `tokio::sync::mpsc`), and Pty on top of `ConnectClient` + `EnvdApiClient` + the `envd::proto` types — applying `handle_rpc_error` around RPC calls and resolving the carried-forward 2b-i items (array-query comma-join, per-call timeouts, error-message decoration, the public-API boundary / wrapping generated types into the public `Sandbox`/`SandboxInfo` types).
```
