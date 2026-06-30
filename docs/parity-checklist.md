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
| `pause`/`betaPause`/resume/`getMetrics`/snapshots/`updateNetwork`/MCP/signed-URLs | _(Plan 3a-extras — see below)_ | ✅ |
| `files`/`commands`/`pty` | _(Plans 3b/3c)_ | ⬜ |

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

## Sandbox commands & pty (Plan 3c)

| JS (`sandbox.commands.*` / `sandbox.pty.*`) | Rust (`sandbox.commands().*` / `sandbox.pty().*`) | Status |
|---|---|---|
| `commands.run` (foreground, waits for exit) | `Commands::run` → `Ok(CommandResult)` (non-zero exit is **data**, not an error; JS throws on non-zero) | ✅ |
| `commands.run` (background / streaming) | `Commands::start` → `CommandHandle` (live output via `next`; result via `wait`) | ✅ |
| `commands.list` | `Commands::list` → `Vec<ProcessInfo>` | ✅ |
| `commands.kill(pid)` | `Commands::kill(pid)` → `bool` (`false` = not found) | ✅ |
| `commands.sendStdin(pid, data)` | `Commands::send_stdin(pid, data)` | ✅ |
| `commands.closeStdin(pid)` | `Commands::close_stdin(pid)` (version-gated: envd >= `ENVD_ENVD_CLOSE`) | ✅ |
| `commands.connect(pid)` | `Commands::connect(pid, user)` → `CommandHandle` | ✅ |
| `pty.create(size, opts)` | `Pty::create(size, opts)` → `CommandHandle` (output as `CommandOutput::Pty`) | ✅ |
| `pty.sendInput(pid, data)` | `Pty::send_input(pid, data)` | ✅ |
| `pty.resize(pid, size)` | `Pty::resize(pid, size)` | ✅ |
| `pty.kill(pid)` | `Pty::kill(pid)` → `bool` (`false` = not found) | ✅ |
| `pty.connect(pid)` | `Pty::connect(pid, user)` → `CommandHandle` | ✅ |

> **Divergence — non-zero exit:** The JS SDK throws `CommandExitError` when a command exits with a non-zero code; `e2b-rs` returns `Ok(CommandResult)` with the exit code in `result.exit_code`. Callers must check `exit_code` themselves.
>
> **Carry-forwards (out of scope):** `StreamInput` client-streaming RPC (unused by JS); tag-based process selection (only pid used); per-stream `connect-timeout-ms` / `KEEPALIVE_PING_HEADER` on long-lived streams (Plan 2b carry-forward); deduplicating the `ConnectClient` across `Filesystem`/`Commands`/`Pty` (currently one client per subsystem).

## Sandbox & I/O (Plan 3, remaining) · Git & Volume (Plan 4) · Template (Plan 5)

_Rows added as each milestone lands._
