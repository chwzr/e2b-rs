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

## Codegen & wire types (Plan 2a)

| Source (`../E2B/spec`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `envd/*.proto` | `envd::proto::{filesystem,process}` (protox+prost+pbjson) | ✅ |
| `openapi.yml` schemas | `api::gen` (typify) | ✅ |
| `openapi-volumecontent.yml` schemas | `volume::gen` (typify) | ✅ |
| `envd/envd.yaml` | `envd::rest_gen` (typify) | ✅ |
| `mcp-server.json` | `sandbox::mcp_gen` (hand-written stub — DEFERRED) | ⬜ |

Note: typify produced no useful types from the `mcp-server.json` catalog schema (a bare object-of-servers catalog with no `$defs`; wrong target anyway). The `McpServer` config type will be hand-written when MCP is wired in Plan 3/5.

Transports (ApiClient/EnvdApiClient/Connect client) consume these in Plan 2b.

## REST transports (Plan 2b-i)

| JS (`src/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `api/index.ts` `ApiClient` + `validateApiKey` | `api::client::{ApiClient, validate_api_key}` | ✅ |
| `api/inflight.ts` `limitConcurrency` | `http::inflight::ConcurrencyLimiter` | ✅ |
| `envd/api.ts` client + `checkSandboxHealth` | `envd::rest::EnvdApiClient` (+ `check_health`) | ✅ |
| `api/index.ts` per-endpoint calls (createSandbox, …) | _(Plan 3+, built on `ApiClient::request`)_ | ⬜ |
| `envd/api.ts` `/files` read/write | _(Plan 3 Filesystem — multipart/octet-stream/gzip)_ | ⬜ |

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

## Sandbox & I/O (Plan 3, remaining) · Git & Volume (Plan 4) · Template (Plan 5)

_Rows added as each milestone lands._
