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

## Transports (Plan 2b) · Sandbox & I/O (Plan 3) · Git & Volume (Plan 4) · Template (Plan 5)

_Rows added as each milestone lands._
