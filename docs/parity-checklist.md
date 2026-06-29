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

## Transports (Plan 2) · Sandbox & I/O (Plan 3) · Git & Volume (Plan 4) · Template (Plan 5)

_Rows added as each milestone lands._
