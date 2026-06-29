# E2B Rust SDK — Codegen & Vendored Types (Plan 2a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the `crates/xtask` codegen driver and generate-and-vendor all the wire types the SDK needs — envd protobuf messages (as proto3-JSON-serializable structs) and the three OpenAPI surfaces' schema types (plus MCP) — committed into `crates/e2b-rs/src`, so consumers build with no codegen toolchain.

**Architecture:** A new non-published `crates/xtask` binary runs the generators ONCE (`cargo xtask codegen`) and writes committed `.rs` files into the SDK crate. envd messages use `protox` (pure-Rust protoc) → `prost-build` → `pbjson-build` (proto3-JSON serde). OpenAPI/MCP schema types use `typify`. The published `e2b-rs` crate gains only the *runtime* deps the generated code references (`prost`, `pbjson-types`, `chrono`, `uuid`, `regress`, `serde`); the heavy codegen deps live solely in `xtask`.

**Tech Stack:** `protox 0.9`, `prost`/`prost-build 0.14`, `pbjson`/`pbjson-build`/`pbjson-types 0.9`, `typify 0.4`, `schemars 0.8`, `serde_yaml 0.9` (xtask); `prost`/`pbjson-types`/`chrono`/`uuid`/`regress`/`serde` (e2b-rs runtime). This recipe was validated end-to-end in a throwaway spike (round-trips pass, large openapi.yml compiles).

**Reference spec:** `.super/specs/2026-06-28-e2b-rust-sdk-design.md` (§3 D3/D8, §4.4, §5). **Spec source-of-truth:** `../E2B/spec` (the sibling E2B checkout).

## Milestone roadmap (context — this is Plan 2a; Plan 1 Foundation is merged to `main`)

| Plan | Deliverable |
|---|---|
| 1 — Foundation | DONE, merged: errors, config+env, logs, utils, paginator, signature |
| **2a — Codegen & vendored types (this plan)** | `crates/xtask` + committed generated types (envd proto, 3 OpenAPI surfaces, MCP) that compile |
| 2b — Transports | `ApiClient`, `EnvdApiClient`, Connect-over-JSON client (wiremock + byte-level tests) — written against 2a's real type names |
| 3 — Sandbox & envd I/O · 4 — Git & Volume · 5 — Template & Polish | … |

## Global Constraints

These apply to **every** task; each task's requirements implicitly include them.

- **Repo/workspace:** `e2b-rs` is the workspace root; published package `e2b-rs`, lib `e2b_rs`; ALL crates under `crates/`. This plan adds `crates/xtask` (a binary, `publish = false`).
- **Toolchain:** edition 2024, MSRV 1.95.0 (pinned via `rust-toolchain.toml`). `cargo`/`protoc`-free codegen via pure-Rust `protox`. crates.io network is available.
- **Lints (hand-written code):** `clippy::unwrap_used`/`expect_used` and `missing_docs` are denied via `[workspace.lints]`; allowed in tests via `clippy.toml`. **No `.unwrap()`/`.expect()`/`panic!` in non-test hand-written code** — use `?`/`match`/`.map_err(..)`. The `xtask` binary is also hand-written code and MUST obey this (codegen errors propagate via `Result`, not `unwrap`).
- **Generated (vendored) code is exempt** via a header `xtask` prepends to every file it writes (see Task 1) AND by being `pub(crate)` (so `missing_docs` never fires on it).
- **Vendoring, not build.rs:** generation writes committed `.rs` files into `crates/e2b-rs/src/...`. Consumers never run `xtask`. `cargo xtask codegen` must be idempotent (re-running yields no diff).
- **Codegen deps live ONLY in `crates/xtask`.** The `e2b-rs` crate gains only the runtime deps the generated code references.
- **Docs:** every *hand-written* public item gets a `///` doc with a runnable-or-`no_run` example where it has behavior; generated `pub(crate)` modules are exempt.
- **TDD where it applies:** generated types are verified by *serde round-trip tests* against representative JSON (these are real, TDD-able tests). Pure codegen wiring is verified by "compiles + idempotent".
- **Commits:** conventional messages ending with the trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`. Run `cargo fmt --all` before every commit (Plan 1 lesson). Commit `Cargo.lock` changes.
- **Spike provenance:** the exact recipe, versions, and gotchas below are validated facts from a throwaway spike, not guesses — follow them precisely.

### Generated file layout (what this plan produces)

```
crates/e2b-rs/src/
├── envd/
│   ├── mod.rs                 # pub(crate) mod proto; pub(crate) mod rest_gen; (hand-written)
│   ├── proto/
│   │   ├── mod.rs             # pub(crate) mod filesystem; pub(crate) mod process;
│   │   ├── filesystem.rs      # VENDORED (protox+prost+pbjson; struct defs + serde, concatenated)
│   │   └── process.rs         # VENDORED
│   └── rest_gen.rs            # VENDORED (typify ← envd.yaml)
├── api/
│   ├── mod.rs                 # pub(crate) mod gen; (hand-written)
│   └── gen.rs                 # VENDORED (typify ← openapi.yml schemas)
├── volume/
│   ├── mod.rs                 # pub(crate) mod gen; (hand-written)
│   └── gen.rs                 # VENDORED (typify ← openapi-volumecontent.yml schemas)
└── sandbox/
    └── mcp_gen.rs             # VENDORED (typify ← mcp-server.json); declared from sandbox/mod.rs
crates/xtask/
├── Cargo.toml
└── src/
    ├── main.rs                # arg parse: `codegen` subcommand
    ├── proto.rs               # proto → vendored envd/proto/*
    ├── openapi.rs             # reusable: OpenAPI schemas → typify → vendored file
    └── vendor.rs              # shared: header + rustfmt + write-into-src helper
```

---

### Task 1: `crates/xtask` skeleton, runtime deps, and the vendoring helper

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Create: `crates/xtask/Cargo.toml`
- Create: `crates/xtask/src/main.rs`
- Create: `crates/xtask/src/vendor.rs`
- Modify: `crates/e2b-rs/Cargo.toml` (add runtime deps generated code will reference)
- Create: `crates/e2b-rs/src/codegen_smoke.rs` (temporary compile-smoke module, removed in Task 7)
- Modify: `crates/e2b-rs/src/lib.rs` (temporary `mod codegen_smoke;`)

**Interfaces:**
- Consumes: nothing from Plan 1 beyond the existing workspace.
- Produces:
  - `crates/xtask` binary runnable as `cargo run -p xtask -- codegen` (alias: `cargo xtask` if a `.cargo/config.toml` alias is added — this task adds it).
  - `xtask::vendor::write_generated(out_path: &Path, body: &str) -> anyhow::Result<()>` — prepends the standard generated-file header, runs `rustfmt` on the result, and writes it to `out_path` (creating parent dirs).
  - `xtask::vendor::GENERATED_HEADER: &str` — the inner-attribute + "do not edit" banner.

- [ ] **Step 1: Add a cargo alias and the xtask manifest**

Create `.cargo/config.toml` (workspace root):

```toml
[alias]
xtask = "run --package xtask --"
```

Create `crates/xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
publish = false

[lints]
workspace = true

[dependencies]
anyhow = "1"
protox = "0.9"
prost = "0.14"
prost-build = "0.14"
pbjson-build = "0.9"
typify = "0.4"
schemars = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
proc-macro2 = "1"
```

(Formatting uses the `rustfmt` binary via `vendor.rs`, so no `prettyplease`/`syn` deps are needed.)

Add to workspace `Cargo.toml` `[workspace.dependencies]` (so e2b-rs and future crates share versions):

```toml
prost = "0.14"
pbjson = "0.9"
pbjson-types = "0.9"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["serde", "v4"] }
regress = "0.10"
serde_json = "1"
serde = { version = "1", features = ["derive"] }
```

(Plan 1's `[workspace.dependencies]` had only `thiserror`/`sha2`/`base64`, so `serde`/`serde_json` are new here. `anyhow` is intentionally NOT a workspace dep — it is used only by `xtask` (declared directly in its manifest), never by the published `e2b-rs` crate, which uses its own `Error`/`Result`.)

- [ ] **Step 2: Add runtime deps the generated code will reference to `e2b-rs`**

In `crates/e2b-rs/Cargo.toml` `[dependencies]`, add:

```toml
serde = { workspace = true }
serde_json = { workspace = true }
prost = { workspace = true }
pbjson = { workspace = true }
pbjson-types = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
regress = { workspace = true }
```

(`serde` needs the `derive` feature — ensure the workspace dep carries it. `prost` provides the `Message` derive the generated structs use; `pbjson-types` provides `Timestamp`; `chrono`/`uuid`/`regress` are referenced by typify output.)

- [ ] **Step 3: Write the vendoring helper (failing compile first)**

Create `crates/xtask/src/vendor.rs`:

```rust
//! Shared helpers for writing generated code into the SDK source tree.

use std::path::Path;

/// Header prepended to every vendored file: marks it generated and exempts it
/// from the workspace lints (it is `pub(crate)`, so `missing_docs` never fires;
/// these allows cover clippy + dead code for code with no caller yet).
pub const GENERATED_HEADER: &str = "\
// @generated by `cargo xtask codegen` from ../E2B/spec — DO NOT EDIT BY HAND.
#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]
#![allow(dead_code, unused_imports, missing_docs)]
";

/// Prepend [`GENERATED_HEADER`], format with rustfmt, and write to `out_path`
/// (creating parent directories). Returns an error rather than panicking.
pub fn write_generated(out_path: &Path, body: &str) -> anyhow::Result<()> {
    let with_header = format!("{GENERATED_HEADER}\n{body}\n");
    let formatted = rustfmt(&with_header)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, formatted)?;
    Ok(())
}

/// Format Rust source via the `rustfmt` binary; falls back to the unformatted
/// source if rustfmt is unavailable (the file still compiles).
fn rustfmt(src: &str) -> anyhow::Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Spawn rustfmt; if it isn't installed, return the source unformatted.
    let mut child = match Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Ok(src.to_string()),
    };

    // Write the source to rustfmt's stdin (drop the handle to signal EOF).
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(src.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(String::from_utf8(output.stdout)?)
    } else {
        Ok(src.to_string())
    }
}
```

Create `crates/xtask/src/main.rs`:

```rust
//! Codegen driver for e2b-rs. Run with `cargo xtask codegen`.
//!
//! Generation modules are added in later tasks; this skeleton dispatches the
//! `codegen` subcommand and fails loudly on unknown input.

mod vendor;

fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("codegen") => {
            println!("xtask codegen: no generators wired yet");
            Ok(())
        }
        other => Err(anyhow::anyhow!(
            "unknown xtask command: {other:?} (expected `codegen`)"
        )),
    }
}
```

- [ ] **Step 4: Add a temporary compile-smoke module to prove the runtime deps wire up**

Create `crates/e2b-rs/src/codegen_smoke.rs`:

```rust
//! TEMPORARY: proves the codegen-output runtime deps (prost, pbjson-types,
//! chrono, uuid, regress, serde) link. Removed in Task 7 once real generated
//! modules exist.

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_codegen_deps_link() {
        // pbjson-types Timestamp (referenced by generated proto structs)
        let ts = pbjson_types::Timestamp { seconds: 0, nanos: 0 };
        assert_eq!(ts.seconds, 0);
        // chrono (referenced by typify date-time fields)
        let _now: chrono::DateTime<chrono::Utc> = chrono::DateTime::UNIX_EPOCH;
        // uuid (referenced by typify uuid fields)
        let _id = uuid::Uuid::nil();
        // serde_json round-trips
        let v: serde_json::Value = serde_json::json!({"ok": true});
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
    }
}
```

Add to `crates/e2b-rs/src/lib.rs` (temporary — Task 7 removes it):

```rust
mod codegen_smoke;
```

- [ ] **Step 5: Verify the workspace builds with xtask and the new deps**

Run: `cargo build --workspace`
Expected: both `e2b-rs` and `xtask` compile (deps fetched from crates.io on first run).

Run: `cargo xtask codegen`
Expected: prints `xtask codegen: no generators wired yet`, exit 0.

Run: `cargo test -p e2b-rs codegen_smoke`
Expected: PASS (1 test) — confirms runtime deps link.

Run: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`
Expected: clean (the `xtask` binary obeys the no-unwrap lint; `vendor.rs` uses `?`/`match`, no unwrap).

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add .cargo Cargo.toml Cargo.lock crates/xtask crates/e2b-rs/Cargo.toml crates/e2b-rs/src/codegen_smoke.rs crates/e2b-rs/src/lib.rs
git commit -m "build(xtask): scaffold codegen driver, vendoring helper, runtime deps" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: envd protobuf messages → vendored `envd/proto`

Generate proto3-JSON-serializable structs for the two envd `.proto` files via `protox` → `prost-build` → `pbjson-build`, and vendor them. This is the footgun-heavy task — follow the gotchas exactly.

**Files:**
- Create: `crates/xtask/src/proto.rs`
- Modify: `crates/xtask/src/main.rs` (call proto codegen)
- Create (VENDORED by xtask): `crates/e2b-rs/src/envd/proto/filesystem.rs`, `crates/e2b-rs/src/envd/proto/process.rs`
- Create: `crates/e2b-rs/src/envd/proto/mod.rs`, `crates/e2b-rs/src/envd/mod.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (`pub(crate) mod envd;`)
- Test: `crates/e2b-rs/src/envd/proto/mod.rs` (`#[cfg(test)]` round-trip)

**Interfaces:**
- Consumes: `xtask::vendor::write_generated`.
- Produces:
  - `xtask::proto::generate(spec_dir: &Path, sdk_src: &Path) -> anyhow::Result<()>`.
  - Vendored modules `e2b_rs::envd::proto::filesystem` and `::process` (`pub(crate)`), containing prost structs + pbjson serde impls. Sample types: `process::ProcessConfig`, `process::ProcessEvent`, `filesystem::EntryInfo`, `filesystem::FileType` (enum, variants `Unspecified`/`File`/`Directory`), `process::Signal` (`Unspecified`/`Sigterm`/`Sigkill`).

- [ ] **Step 1: Write the proto generation module**

Create `crates/xtask/src/proto.rs`:

```rust
//! Generate envd protobuf message types (proto3-JSON serde) and vendor them.

use crate::vendor::write_generated;
use prost::Message;
use std::path::Path;

/// Compile `filesystem.proto` and `process.proto` from `spec_dir/envd` and
/// vendor the generated message structs (+ pbjson serde impls) into
/// `sdk_src/envd/proto/{filesystem,process}.rs`.
pub fn generate(spec_dir: &Path, sdk_src: &Path) -> anyhow::Result<()> {
    let proto_root = spec_dir.join("envd");

    // 1. protox: pure-Rust compile to a FileDescriptorSet (bundles google WKTs).
    //    Pass proto paths RELATIVE to the include root (spike-verified form).
    let fds = protox::compile(
        ["filesystem/filesystem.proto", "process/process.proto"],
        [&proto_root],
    )?;
    let fds_bytes = fds.encode_to_vec();

    // 2. Generate into a temp dir so prost/pbjson keep their per-package files.
    let tmp = std::env::temp_dir().join("e2b_rs_proto_gen");
    std::fs::create_dir_all(&tmp)?;

    // prost-build: struct defs. CRITICAL: prost_types_path REPLACES the default
    // `.google.protobuf -> ::prost_types` mapping. Do NOT use extern_path here —
    // it adds a duplicate key and panics ("duplicate extern Protobuf path").
    let mut cfg = prost_build::Config::new();
    cfg.prost_types_path("::pbjson_types");
    cfg.out_dir(&tmp);
    cfg.compile_fds(fds)?;

    // pbjson-build: proto3-JSON serde impls. Writes `{package}.serde.rs` to OUT_DIR.
    // SAFETY: single-threaded codegen; set OUT_DIR for pbjson-build's writer.
    unsafe { std::env::set_var("OUT_DIR", &tmp) };
    pbjson_build::Builder::new()
        .register_descriptors(&fds_bytes)?
        .build(&[".filesystem", ".process"])?;

    // 3. Concatenate each package's struct file + serde file into one vendored
    //    module file, then write with the generated header + rustfmt.
    for pkg in ["filesystem", "process"] {
        let defs = std::fs::read_to_string(tmp.join(format!("{pkg}.rs")))?;
        let serde = std::fs::read_to_string(tmp.join(format!("{pkg}.serde.rs")))?;
        let body = format!("{defs}\n{serde}");
        write_generated(&sdk_src.join(format!("envd/proto/{pkg}.rs")), &body)?;
    }
    Ok(())
}
```

(Note: `std::env::set_var` is `unsafe` in edition 2024 — wrap as shown. xtask is single-threaded so this is sound. This is xtask-only code; the `unsafe` does not appear in the SDK.)

- [ ] **Step 2: Wire it into main and create the module scaffolding**

In `crates/xtask/src/main.rs`, add `mod proto;` and, in the `"codegen"` arm, replace the placeholder with:

```rust
Some("codegen") => {
    let spec_dir = std::path::PathBuf::from(
        std::env::var("E2B_SPEC_DIR").unwrap_or_else(|_| "../E2B/spec".to_string()),
    );
    let sdk_src = std::path::PathBuf::from("crates/e2b-rs/src");
    proto::generate(&spec_dir, &sdk_src)?;
    println!("xtask codegen: wrote envd proto modules");
    Ok(())
}
```

(`unwrap_or_else` is allowed — it is not `unwrap`/`expect`. The default path assumes `cargo xtask` runs from the workspace root, where `../E2B/spec` resolves.)

Create `crates/e2b-rs/src/envd/mod.rs`:

```rust
//! envd daemon clients and generated wire types. Client wiring lands in Plan 2b.

pub(crate) mod proto;
```

Create `crates/e2b-rs/src/envd/proto/mod.rs`:

```rust
//! Generated envd protobuf message types (proto3-JSON serde). Generated by
//! `cargo xtask codegen`; see the per-module files.

pub(crate) mod filesystem;
pub(crate) mod process;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_info_round_trips_proto3_json() {
        // proto3 JSON: int64 -> quoted string, enum -> full proto name,
        // snake_case -> camelCase, optional None omitted.
        let json = r#"{
            "name": "hello.txt",
            "type": "FILE_TYPE_FILE",
            "path": "/tmp/hello.txt",
            "size": "42",
            "mode": 420,
            "permissions": "rw-r--r--",
            "owner": "root",
            "group": "root"
        }"#;
        let info: filesystem::EntryInfo =
            serde_json::from_str(json).expect("deserialize EntryInfo");
        assert_eq!(info.name, "hello.txt");
        assert_eq!(info.path, "/tmp/hello.txt");
        assert_eq!(info.size, 42); // int64 parsed from "42"
        // FileType::File == 1 (prost strips the FILE_TYPE_ prefix on the variant)
        assert_eq!(info.r#type, filesystem::FileType::File as i32);

        // Re-serialize: size becomes a quoted string again, type the full name.
        let back = serde_json::to_value(&info).expect("serialize EntryInfo");
        assert_eq!(back["size"], serde_json::json!("42"));
        assert_eq!(back["type"], serde_json::json!("FILE_TYPE_FILE"));
    }

    #[test]
    fn process_config_constructs() {
        let cfg = process::ProcessConfig {
            cmd: "/bin/bash".to_string(),
            args: vec!["-l".to_string(), "-c".to_string()],
            envs: std::collections::HashMap::new(),
            cwd: Some("/home/user".to_string()),
        };
        let json = serde_json::to_value(&cfg).expect("serialize ProcessConfig");
        assert_eq!(json["cmd"], serde_json::json!("/bin/bash"));
    }
}
```

(Tests use `.expect(..)` — allowed in `#[cfg(test)]`. Field names/types — `r#type: i32`, `size: i64`, `cwd: Option<String>` — are the spike-verified shapes. If a field name differs in the actual generated output, fix the TEST to match the generated struct, not the struct.)

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub(crate) mod envd;
```

- [ ] **Step 3: Generate and verify the vendored output compiles**

Run: `cargo xtask codegen`
Expected: prints `wrote envd proto modules`; `crates/e2b-rs/src/envd/proto/filesystem.rs` and `process.rs` now exist, each starting with the generated header.

Run: `cargo build -p e2b-rs`
Expected: compiles (generated structs derive `prost::Message` + pbjson serde).

- [ ] **Step 4: Run the round-trip tests**

Run: `cargo test -p e2b-rs envd::proto`
Expected: PASS (2 tests). If a test fails on a field name/type, adjust the test to the generated reality (do NOT hand-edit the generated file).

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean (generated files exempt via header; xtask code panic-free).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/xtask/src/proto.rs crates/xtask/src/main.rs crates/e2b-rs/src/envd Cargo.lock
git commit -m "feat(codegen): vendor envd protobuf message types (proto3-JSON)" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: OpenAPI→types helper + volume-content types

Write the reusable typify-based OpenAPI-schema generator (used again in Tasks 4–5) and apply it to the small `openapi-volumecontent.yml` to prove it end-to-end.

**Files:**
- Create: `crates/xtask/src/openapi.rs`
- Modify: `crates/xtask/src/main.rs`
- Create (VENDORED): `crates/e2b-rs/src/volume/gen.rs`
- Create: `crates/e2b-rs/src/volume/mod.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (`pub(crate) mod volume;`)

**Interfaces:**
- Consumes: `xtask::vendor::write_generated`.
- Produces:
  - `xtask::openapi::generate_schema_types(spec_path: &Path, out_path: &Path) -> anyhow::Result<()>` — reads an OpenAPI YAML, extracts `components.schemas`, rewrites `$ref`s, runs typify, vendors the result. Reused by Tasks 4–5.
  - Vendored `e2b_rs::volume::gen` (`pub(crate)`). Sample types: `gen::VolumeEntryStat { atime, ctime, mtime: chrono::DateTime<Utc>, gid, mode, uid: u32, name, path: String, size: i64, target: Option<String>, type_: VolumeEntryStatType }`, `gen::VolumeEntryStatType` (enum Unknown/File/Directory/Symlink), `gen::Error { code: String, message: String }`. typify preserves original field names (no camelCase); `type` → `type_` with serde rename.

- [ ] **Step 1: Write the reusable OpenAPI schema generator**

Create `crates/xtask/src/openapi.rs`:

```rust
//! Generate Rust types from an OpenAPI document's `components.schemas` via
//! typify, and vendor the result. typify consumes JSON Schema, so we lift the
//! schema section into a draft-07 root document and rewrite the `$ref` base.

use crate::vendor::write_generated;
use std::path::Path;
use typify::{TypeSpace, TypeSpaceSettings};

/// Extract `components.schemas` from the OpenAPI doc at `spec_path`, generate
/// Rust types, and vendor them to `out_path`.
pub fn generate_schema_types(spec_path: &Path, out_path: &Path) -> anyhow::Result<()> {
    // 1. Parse OpenAPI YAML into JSON.
    let raw = std::fs::read_to_string(spec_path)?;
    let spec: serde_json::Value = serde_yaml::from_str(&raw)?;

    // 2. Lift components.schemas; OpenAPI `$ref: #/components/schemas/X`
    //    becomes JSON-Schema `$ref: #/definitions/X`.
    let schemas = spec
        .get("components")
        .and_then(|c| c.get("schemas"))
        .ok_or_else(|| anyhow::anyhow!("no components.schemas in {}", spec_path.display()))?;
    let schemas_str =
        serde_json::to_string(schemas)?.replace("#/components/schemas/", "#/definitions/");
    let definitions: serde_json::Value = serde_json::from_str(&schemas_str)?;

    // 3. Build a draft-07 root schema and hand its definitions to typify.
    let root: schemars::schema::RootSchema = serde_json::from_value(serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "definitions": definitions,
    }))?;

    let mut type_space = TypeSpace::new(&TypeSpaceSettings::default());
    type_space.add_ref_types(root.definitions)?;

    // 4. Vendor.
    write_generated(out_path, &type_space.to_stream().to_string())?;
    Ok(())
}
```

- [ ] **Step 2: Wire volume generation into main and create the module**

In `crates/xtask/src/main.rs`, add `mod openapi;` and append to the `"codegen"` arm (after the proto call, before the final `println!`):

```rust
    openapi::generate_schema_types(
        &spec_dir.join("openapi-volumecontent.yml"),
        &sdk_src.join("volume/gen.rs"),
    )?;
```

Create `crates/e2b-rs/src/volume/mod.rs`:

```rust
//! Volume content client and generated wire types. Client wiring lands in a
//! later milestone.

pub(crate) mod gen;

#[cfg(test)]
mod tests {
    use super::gen;

    #[test]
    fn volume_entry_stat_round_trips() {
        let json = r#"{
            "path": "/d/x.txt",
            "name": "x.txt",
            "size": 42,
            "mode": 420,
            "uid": 0,
            "gid": 0,
            "type": "file",
            "atime": "2023-11-14T22:13:20Z",
            "mtime": "2023-11-14T22:13:20Z",
            "ctime": "2023-11-14T22:13:20Z"
        }"#;
        let stat: gen::VolumeEntryStat =
            serde_json::from_str(json).expect("deserialize VolumeEntryStat");
        assert_eq!(stat.name, "x.txt");
        assert_eq!(stat.size, 42);
        assert!(matches!(stat.type_, gen::VolumeEntryStatType::File));
        // `type` field round-trips through the serde rename.
        let back = serde_json::to_value(&stat).expect("serialize");
        assert_eq!(back["type"], serde_json::json!("file"));
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub(crate) mod volume;
```

- [ ] **Step 3: Generate, build, and round-trip**

Run: `cargo xtask codegen`
Expected: writes `crates/e2b-rs/src/volume/gen.rs` (with header).

Run: `cargo build -p e2b-rs`
Expected: compiles.

Run: `cargo test -p e2b-rs volume`
Expected: PASS. If a field name/enum-variant differs from the generated reality, adjust the TEST. (typify enum variants for `unknown/file/directory/symlink` are `Unknown`/`File`/`Directory`/`Symlink` with `#[serde(rename_all=...)]` or per-variant renames — confirm against generated `gen.rs` and fix the `matches!`/asserts if needed.)

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/xtask/src/openapi.rs crates/xtask/src/main.rs crates/e2b-rs/src/volume Cargo.lock
git commit -m "feat(codegen): add OpenAPI->types helper, vendor volume content types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: control-plane API types (`openapi.yml`)

Apply the Task 3 helper to the large `openapi.yml` (93 schemas; allOf/oneOf/discriminator). The spike confirmed typify digests it (compiles). No tag filtering is needed — typify reads `components.schemas`, not paths.

**Files:**
- Modify: `crates/xtask/src/main.rs`
- Create (VENDORED): `crates/e2b-rs/src/api/gen.rs`
- Create: `crates/e2b-rs/src/api/mod.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (`pub(crate) mod api;`)

**Interfaces:**
- Consumes: `xtask::openapi::generate_schema_types`.
- Produces: vendored `e2b_rs::api::gen` (`pub(crate)`). Contains the control-plane schema types (e.g. `Sandbox`, `SandboxDetail`, `SandboxState`, `SandboxMetric`, `Template`, `Node`, registry oneOf as a Rust enum, `Error { code: i64, message: String }` — note `code` is integer here, unlike volume's string `code`).

- [ ] **Step 1: Wire main API generation into main**

In `crates/xtask/src/main.rs`, append to the `"codegen"` arm:

```rust
    openapi::generate_schema_types(
        &spec_dir.join("openapi.yml"),
        &sdk_src.join("api/gen.rs"),
    )?;
```

Create `crates/e2b-rs/src/api/mod.rs`:

```rust
//! Control-plane REST API client and generated schema types. The `ApiClient`
//! and per-endpoint calls land in Plan 2b.

pub(crate) mod gen;

#[cfg(test)]
mod tests {
    use super::gen;

    #[test]
    fn control_plane_error_round_trips() {
        // The control-plane Error uses an integer `code` (distinct from the
        // volume content Error which uses a string code).
        let json = r#"{"code": 404, "message": "sandbox not found"}"#;
        let err: gen::Error = serde_json::from_str(json).expect("deserialize Error");
        assert_eq!(err.code, 404);
        assert_eq!(err.message, "sandbox not found");
    }
}
```

Add to `crates/e2b-rs/src/lib.rs`:

```rust
pub(crate) mod api;
```

- [ ] **Step 2: Generate and verify it compiles**

Run: `cargo xtask codegen`
Expected: writes `crates/e2b-rs/src/api/gen.rs` (large — ~190KB per the spike).

Run: `cargo build -p e2b-rs`
Expected: compiles. If typify emits a type referencing a runtime crate not yet present (the spike found only `chrono`/`uuid`/`regress`, all added in Task 1), add the missing workspace dep and note it.

- [ ] **Step 3: Round-trip and lint**

Run: `cargo test -p e2b-rs api`
Expected: PASS (1 test). If the generated `Error` field is named differently or `code` is a different integer width, adjust the test to match `gen.rs`.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add crates/xtask/src/main.rs crates/e2b-rs/src/api Cargo.lock
git commit -m "feat(codegen): vendor control-plane API schema types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: envd REST types (`envd.yaml`)

Apply the Task 3 helper to `envd/envd.yaml` (the small envd HTTP surface: `/health`, `/files`).

**Files:**
- Modify: `crates/xtask/src/main.rs`
- Create (VENDORED): `crates/e2b-rs/src/envd/rest_gen.rs`
- Modify: `crates/e2b-rs/src/envd/mod.rs` (`pub(crate) mod rest_gen;`)

**Interfaces:**
- Consumes: `xtask::openapi::generate_schema_types`.
- Produces: vendored `e2b_rs::envd::rest_gen` (`pub(crate)`) with the envd REST schema types.

- [ ] **Step 1: Wire envd REST generation into main**

In `crates/xtask/src/main.rs`, append to the `"codegen"` arm:

```rust
    openapi::generate_schema_types(
        &spec_dir.join("envd/envd.yaml"),
        &sdk_src.join("envd/rest_gen.rs"),
    )?;
```

In `crates/e2b-rs/src/envd/mod.rs`, add below the `proto` declaration:

```rust
pub(crate) mod rest_gen;
```

- [ ] **Step 2: Handle the no-schemas case if it arises**

If `cargo xtask codegen` errors with `no components.schemas in .../envd.yaml` (the envd OpenAPI may define only paths/inline bodies with no named schemas), do NOT force it. Instead make `rest_gen.rs` a hand-written stub:

```rust
//! envd REST surface (`/health`, `/files`). The spec defines no named
//! component schemas, so there are no generated types; request/response
//! handling is hand-written in the `EnvdApiClient` (Plan 2b).
```

and remove the `envd.yaml` call from `main.rs` (record this in the report). Otherwise keep the generated module.

- [ ] **Step 3: Build, lint, commit**

Run: `cargo xtask codegen` then `cargo build -p e2b-rs`
Expected: compiles (either generated types or the stub).

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

```bash
cargo fmt --all
git add crates/xtask/src/main.rs crates/e2b-rs/src/envd Cargo.lock
git commit -m "feat(codegen): vendor envd REST types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: MCP server types (`mcp-server.json`)

Generate types for the MCP server config from `mcp-server.json`. The JS SDK uses `json2ts` (it is JSON Schema). Feed it to typify directly (it is already a schema document, not OpenAPI — no `components.schemas` extraction).

**Files:**
- Modify: `crates/xtask/src/openapi.rs` (add a JSON-Schema-document entry point)
- Modify: `crates/xtask/src/main.rs`
- Create (VENDORED): `crates/e2b-rs/src/sandbox/mcp_gen.rs`
- Modify: `crates/e2b-rs/src/sandbox/mod.rs` (`pub(crate) mod mcp_gen;`)

**Interfaces:**
- Consumes: `xtask::vendor::write_generated`.
- Produces:
  - `xtask::openapi::generate_json_schema_types(schema_path: &Path, out_path: &Path) -> anyhow::Result<()>` — for a standalone JSON Schema document.
  - Vendored `e2b_rs::sandbox::mcp_gen` (`pub(crate)`).

- [ ] **Step 1: Add the JSON-Schema-document generator**

In `crates/xtask/src/openapi.rs`, add:

```rust
/// Generate Rust types from a standalone JSON Schema document (e.g. the MCP
/// server schema) — no OpenAPI `components` wrapper.
pub fn generate_json_schema_types(schema_path: &Path, out_path: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(schema_path)?;
    let root: schemars::schema::RootSchema = serde_json::from_str(&raw)?;

    let mut type_space = TypeSpace::new(&TypeSpaceSettings::default());
    // Root schema may carry a top-level type plus `$defs`/`definitions`.
    type_space.add_root_schema(root)?;

    write_generated(out_path, &type_space.to_stream().to_string())?;
    Ok(())
}
```

- [ ] **Step 2: Wire MCP generation into main**

In `crates/xtask/src/main.rs`, append to the `"codegen"` arm:

```rust
    openapi::generate_json_schema_types(
        &spec_dir.join("mcp-server.json"),
        &sdk_src.join("sandbox/mcp_gen.rs"),
    )?;
```

In `crates/e2b-rs/src/sandbox/mod.rs`, add (below the existing `pub mod signature;`):

```rust
pub(crate) mod mcp_gen;
```

- [ ] **Step 3: Generate; adapt if the schema shape needs it**

Run: `cargo xtask codegen`
Expected: writes `crates/e2b-rs/src/sandbox/mcp_gen.rs`.

If typify errors (e.g. the document is a bare schema without `$schema`/`definitions` that `add_root_schema` accepts, or uses an unsupported construct), capture the error and try: parse as `serde_json::Value`, wrap as `{"$schema":"http://json-schema.org/draft-07/schema#", ...document...}` if missing `$schema`, then retry. If it still fails, vendor a minimal hand-written stub module documenting that MCP types are deferred, remove the call, and report it — do NOT block the milestone on MCP (it is only consumed by the optional MCP feature in a later plan).

Run: `cargo build -p e2b-rs`
Expected: compiles.

- [ ] **Step 4: Lint and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

```bash
cargo fmt --all
git add crates/xtask/src/openapi.rs crates/xtask/src/main.rs crates/e2b-rs/src/sandbox Cargo.lock
git commit -m "feat(codegen): vendor MCP server types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: idempotency, cleanup, parity checklist & full gate

Finalize: remove the temporary smoke module, prove `cargo xtask codegen` is idempotent (re-running produces no diff), update docs, and run the full release gate.

**Files:**
- Delete: `crates/e2b-rs/src/codegen_smoke.rs`
- Modify: `crates/e2b-rs/src/lib.rs` (remove `mod codegen_smoke;`)
- Modify: `docs/parity-checklist.md`
- Create: `crates/xtask/README.md`

**Interfaces:**
- Consumes: everything generated in Tasks 2–6.
- Produces: a clean, idempotent codegen milestone.

- [ ] **Step 1: Remove the temporary smoke module**

Delete `crates/e2b-rs/src/codegen_smoke.rs` and remove the `mod codegen_smoke;` line from `lib.rs`. (Its job — proving the runtime deps link — is now covered by the real generated modules and their round-trip tests.)

Run: `cargo test -p e2b-rs` — Expected: all tests pass without the smoke module (the proto + volume + api round-trips remain).

- [ ] **Step 2: Prove idempotency**

Run: `cargo xtask codegen && git status --porcelain`
Expected: NO output from `git status` (re-generating over committed files yields byte-identical results — header + rustfmt are deterministic). If there IS a diff, inspect it: a non-deterministic generator ordering would be a real problem to fix now (e.g. sort inputs). Resolve until re-running is a no-op.

- [ ] **Step 3: Document the codegen workflow**

Create `crates/xtask/README.md`:

```markdown
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
- `sandbox/mcp_gen.rs` — MCP server types (`typify`).

Generation is idempotent: re-running produces no diff. Generated files carry a
`@generated … DO NOT EDIT` header. Consumers of `e2b-rs` never run this — the
output is committed.
```

Update `docs/parity-checklist.md` — add a section:

```markdown
## Codegen & wire types (Plan 2a)

| Source (`../E2B/spec`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `envd/*.proto` | `envd::proto::{filesystem,process}` (protox+prost+pbjson) | ✅ |
| `openapi.yml` schemas | `api::gen` (typify) | ✅ |
| `openapi-volumecontent.yml` schemas | `volume::gen` (typify) | ✅ |
| `envd/envd.yaml` | `envd::rest_gen` (typify) | ✅ |
| `mcp-server.json` | `sandbox::mcp_gen` (typify) | ✅ |

Transports (ApiClient/EnvdApiClient/Connect client) consume these in Plan 2b.
```

- [ ] **Step 4: Full release gate**

Run each and confirm it passes:
- `cargo fmt --all --check` (exit 0)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` (clean)
- `cargo test --workspace --all-features` (Plan 1's 28 tests minus the 1 removed smoke test, plus the new round-trip tests; confirm count and 0 failures)
- `cargo test --doc -p e2b-rs` (2 doctests still pass)
- `cargo doc --no-deps -p e2b-rs` (builds; generated `pub(crate)` modules don't trip `missing_docs`)

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md crates/xtask/README.md
git rm crates/e2b-rs/src/codegen_smoke.rs
git commit -m "chore(codegen): drop smoke module, document xtask, verify idempotency" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 2a is complete when:
- `cargo xtask codegen` regenerates all vendored modules idempotently (no git diff on re-run).
- The vendored modules (`envd::proto::{filesystem,process}`, `api::gen`, `volume::gen`, `envd::rest_gen`, `sandbox::mcp_gen`) compile and their round-trip tests pass.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, and `cargo doc --no-deps` all pass.
- Codegen deps live only in `crates/xtask`; `e2b-rs` carries only the runtime deps the generated code references.
- `docs/parity-checklist.md` reflects the codegen surface.

**Next:** Plan 2b (Transports) — written against the real generated type names produced here — adds `tokio`/`reqwest`, the `Error::Transport(#[from] reqwest::Error)` variant, `validate_api_key`, the `ApiClient` (auth/key-validation/error-map/inflight semaphore/logging middleware), the `EnvdApiClient` (`/health`, `/files`), and the hand-rolled Connect-over-JSON client (envelope codec + unary + server-streaming + `Code`→`Error` mapping + version gates), all tested with `wiremock` + byte-level codec tests.
```
