# xtask — e2b-rs codegen

Regenerates the vendored wire types from the E2B specs. Run from the workspace root:

```
cargo xtask codegen
```

Reads specs from `../E2B/spec` by default (override with `E2B_SPEC_DIR`). Writes
committed, `pub(crate)`, lint-exempt modules into `crates/e2b-rs/src`:

- `envd/proto/{filesystem,process}.rs` — protobuf messages with proto3-JSON serde
  (`protox` → `prost-build` → `pbjson-build`; no system `protoc` required).
- `api/gen.rs`, `volume/gen.rs`, `envd/rest_gen.rs` — OpenAPI schema types (`typify`).
- `sandbox/mcp_gen.rs` — MCP server types (hand-written stub — DEFERRED; typify produced no useful types from the catalog schema).

Generation is idempotent: re-running produces no diff. Generated files carry a
`@generated … DO NOT EDIT` header. Consumers of `e2b-rs` never run this — the
output is committed.
