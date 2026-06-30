# Template Foundation (Plan 5a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Lay the pure (no-I/O) foundation of the `template` subsystem for the `e2b-rs` SDK — the `ReadyCmd` helpers, the build-log types, and the hand-written public data types that wrap the generated `api::schema` template types. The build pipeline, Dockerfile parser, file upload, and builder methods come in Plans 5b–5d.

**Architecture:** A new `pub mod template`. This sub-plan is all hand-written pure types + pure command-string generators — NO HTTP, NO state machine, NO file I/O. Generated wire types (`api::schema::{BuildLogEntry, LogLevel, TemplateBuildStatus, TemplateTag, BuildStatusReason, TemplateRequestResponseV3, TemplateBuildInfo}`) stay `pub(crate)` and are wrapped behind hand-written public types with `from_wire` mappers. Build logs will (in Plan 5c) be delivered over a `tokio::sync::mpsc` channel as `LogEntry` values — this plan defines that `LogEntry` type.

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), `chrono` (timestamps), `uuid` (TemplateTag build_id), `crate::utils::shell_quote`. No new transport.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` only in `#[cfg(test)]`. Prefer `try_from().unwrap_or()` over `as`. `[crate::Type]` cross-module intra-doc links.
- **Do NOT expose generated types.** Wrap `api::schema::*` behind hand-written public types.
- **Honest fixtures (the 3a/3b lesson):** the generated `LogLevel` and `TemplateBuildStatus` enums serialize LOWERCASE (`"debug"/"info"/"warn"/"error"`, `"building"/"waiting"/"ready"/"error"`). Any test that round-trips wire JSON must use the lowercase form; map via the generated enum, never invent casing.
- **Deferred (user decision):** no `addMcpServer`, no devcontainer-beta, no CLI animated logger anywhere in Plan 5. (Logs are consumed via the Plan-5c mpsc channel.)
- Every public item + field documented. Every task: `cargo fmt --all` before commit; `cargo doc --no-deps -p e2b-rs` in the gate. Commit trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference: JS `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/template/{readycmd.ts,logger.ts,types.ts,consts.ts}`.

### Pre-verified facts (confirmed at `main` = b306d3e)
- `crate::utils::shell_quote(s: &str) -> String` exists (utils.rs:33).
- Generated (`crate::api::schema`): `BuildLogEntry { level: LogLevel, message: String, step: Option<String>, timestamp: chrono::DateTime<Utc> }`; `LogLevel` enum renames lowercase `debug/info/warn/error`; `TemplateBuildStatus` enum renames lowercase `building/waiting/ready/error`; `TemplateTag { build_id: uuid::Uuid (rename "buildID"), created_at: DateTime<Utc> (rename "createdAt"), tag: String }`; `BuildStatusReason { message, step, log_entries }` (verify exact fields); `TemplateRequestResponseV3 { template_id (rename templateID), build_id (rename buildID), tags, names }`; `TemplateBuildInfo { build_id, log_entries, logs, reason, status, template_id }`.
- `readycmd.ts` command strings (port verbatim): `wait_for_port(port)` → ``[ -n "$(ss -Htuln sport = :{port})" ]``; `wait_for_url(url, status=200)` → ``curl -s -o /dev/null -w "%{{http_code}}" {shell_quote(url)} | grep -q "{status}"``; `wait_for_process(name)` → ``pgrep {shell_quote(name)} > /dev/null``; `wait_for_file(path)` → ``[ -f {shell_quote(path)} ]``; `wait_for_timeout(ms)` → ``sleep {max(1, ms/1000)}`` (integer seconds, floor, min 1).
- `LogEntry` (logger.ts): `{ timestamp: Date, level, message }`; ANSI escape codes are STRIPPED from `message` on construction (logger.ts:20-22). `toString()` → `"[<ISO timestamp>] [<level>] <message>"`.

---

## File Structure

- `crates/e2b-rs/src/template/mod.rs` — CREATE: module wiring + public re-exports.
- `crates/e2b-rs/src/template/readycmd.rs` — CREATE: `ReadyCmd` + 5 helper fns.
- `crates/e2b-rs/src/template/log.rs` — CREATE: `LogEntry`, `LogEntryLevel`, ANSI strip, `from_wire`.
- `crates/e2b-rs/src/template/types.rs` — CREATE: public data wrappers (`BuildStatus`, `BuildStatusReason`, `TemplateTag`, `BuildInfo`, `TemplateBuildStatusResponse`, `InstructionType`, `Instruction`, `CopyItem`) + `from_wire` mappers.
- `crates/e2b-rs/src/lib.rs` — MODIFY: `pub mod template;` (the generated `api::schema` stays where it is) + crate-root re-exports.
- `docs/parity-checklist.md` — MODIFY (Task 3).

---

### Task 1: Module wiring + `ReadyCmd` and the wait-for helpers

**Files:** Create `template/mod.rs`, `template/readycmd.rs`; modify `lib.rs`.

**Interfaces:**
- Produces (public): `struct ReadyCmd` (opaque wrapper over a command `String`) with `pub fn cmd(&self) -> &str` and a `pub(crate) fn into_cmd(self) -> String` (used by 5c to serialize into the build request). `#[derive(Debug, Clone)]`.
- Free fns (public): `wait_for_port(port: u16) -> ReadyCmd`, `wait_for_url(url: &str, status_code: u16) -> ReadyCmd` (default 200 — provide `wait_for_url` taking `status_code` and document that 200 is the JS default; callers pass 200 explicitly, OR add a `wait_for_url_default(url)` convenience — KEEP it simple: one fn `wait_for_url(url, status_code)`), `wait_for_process(name: &str) -> ReadyCmd`, `wait_for_file(path: &str) -> ReadyCmd`, `wait_for_timeout(timeout_ms: u64) -> ReadyCmd`.

- [ ] **Step 1: Write the failing tests** (`readycmd.rs` `#[cfg(test)] mod tests`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_cmd_strings_match_js() {
        assert_eq!(wait_for_port(8080).cmd(), r#"[ -n "$(ss -Htuln sport = :8080)" ]"#);
        assert_eq!(wait_for_process("node").cmd(), "pgrep node > /dev/null");
        assert_eq!(wait_for_file("/tmp/ready").cmd(), "[ -f /tmp/ready ]");
        assert_eq!(wait_for_timeout(5000).cmd(), "sleep 5");
        assert_eq!(wait_for_timeout(500).cmd(), "sleep 1");   // min 1s
        let u = wait_for_url("http://localhost:3000", 200);
        assert_eq!(u.cmd(), r#"curl -s -o /dev/null -w "%{http_code}" http://localhost:3000 | grep -q "200""#);
    }
    #[test]
    fn ready_cmd_shell_quotes_args() {
        // a path with a space must be shell-quoted by wait_for_file
        assert_eq!(wait_for_file("/tmp/a b").cmd(), "[ -f '/tmp/a b' ]");
    }
}
```
In `template/mod.rs`: `pub mod readycmd;` + `pub use readycmd::{ReadyCmd, wait_for_port, wait_for_url, wait_for_process, wait_for_file, wait_for_timeout};`. In `lib.rs`: `pub mod template;` + re-export those at the crate root.

- [ ] **Step 2: Run to verify failure** — `cargo test -p e2b-rs template::readycmd` → FAIL.
- [ ] **Step 3: Implement** — port the 5 command strings verbatim (use `crate::utils::shell_quote` for url/process/file args; `wait_for_port`/`wait_for_timeout` interpolate the validated number — `wait_for_timeout` uses `let seconds = (timeout_ms / 1000).max(1);`). `ReadyCmd { cmd: String }` with `cmd()`/`into_cmd()`. Document every item.
- [ ] **Step 4: Run tests green; verify & commit** — clippy `-D warnings`, `cargo doc` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add ReadyCmd and wait-for helpers" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Build-log types + build-status/tag public wrappers

**Files:** Create `template/log.rs`, `template/types.rs`; modify `template/mod.rs`, `lib.rs`.

**Interfaces:**
- Produces (`log.rs`, public): `enum LogEntryLevel { Debug, Info, Warn, Error }` (derive Debug/Clone/Copy/PartialEq/Eq); `struct LogEntry { timestamp: chrono::DateTime<Utc>, level: LogEntryLevel, message: String }` (derive Debug/Clone) with `pub fn timestamp/level/message` getters and a `Display` impl producing `"[<rfc3339>] [<level>] <message>"`. `pub(crate) fn LogEntry::from_wire(w: crate::api::schema::BuildLogEntry) -> LogEntry` — map `level` via the generated `LogLevel` → `LogEntryLevel`, and STRIP ANSI escape sequences from `message` (port logger.ts:20-22 — a small `strip_ansi(&str) -> String` helper removing `\x1b[...m`-style sequences; cover with a unit test).
- Produces (`types.rs`, public): `enum BuildStatus { Building, Waiting, Ready, Error }` (+ `from_wire` from generated `TemplateBuildStatus`); `struct BuildStatusReason { message: String, step: Option<String>, log_entries: Vec<LogEntry> }` (+ from_wire); `struct TemplateTag { tag: String, build_id: String, created_at: chrono::DateTime<Utc> }` (+ from_wire — generated `build_id` is a `uuid::Uuid`; expose as `String` via `.to_string()`); `struct TemplateBuildStatusResponse { template_id: String, build_id: String, status: BuildStatus, logs: Vec<String>, log_entries: Vec<LogEntry>, reason: Option<BuildStatusReason> }` (+ from_wire from generated `TemplateBuildInfo`).

- [ ] **Step 1: Write failing tests** (HONEST lowercase wire):
  - `log.rs`: `from_wire_maps_level_and_strips_ansi` — build a `BuildLogEntry { level: LogLevel::Warn, message: "\x1b[31mboom\x1b[0m".into(), step: None, timestamp: <fixed> }`, assert `LogEntry::from_wire(..).level == LogEntryLevel::Warn` and `message == "boom"`. `display_format` — assert `format!("{entry}")` starts with `[` and contains `[warn] boom`.
  - `types.rs`: `build_status_from_wire` — map each `TemplateBuildStatus` variant. `tag_from_wire` — generated `TemplateTag` (build_id uuid, createdAt) → public with `build_id` as the uuid string. A serde round-trip test deserializing an HONEST `TemplateBuildInfo` JSON (lowercase `"status":"building"`, `"logEntries":[{"level":"info",...}]`, camelCase `templateID`/`buildID`) → `TemplateBuildStatusResponse::from_wire` maps it.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** the types + `from_wire` mappers + `strip_ansi`. Verify the generated `BuildStatusReason`/`TemplateBuildInfo` field names by reading `api/schema.rs` before mapping.
- [ ] **Step 4: Re-export** `LogEntry`/`LogEntryLevel`/`BuildStatus`/`BuildStatusReason`/`TemplateTag`/`TemplateBuildStatusResponse` (mod.rs → lib.rs). Tests green.
- [ ] **Step 5: Verify & commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add build-log types and build-status/tag wrappers" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Builder data types (`InstructionType`/`Instruction`/`CopyItem`/`BuildInfo`) + parity + gate

**Files:** Modify `template/types.rs`, `template/mod.rs`, `lib.rs`, `docs/parity-checklist.md`.

**Interfaces:**
- Produces (public): `enum InstructionType { Copy, Env, Run, Workdir, User }` (mirror JS `InstructionType`, types.ts:180); `struct Instruction { instruction_type: InstructionType, args: Vec<String>, force: bool, force_upload: Option<bool>, files_hash: Option<String>, resolve_symlinks: bool }` (the internal-but-public step representation; Plan 5c/5d build these and serialize to the generated `TemplateStep`); `struct CopyItem { src: Vec<String>, dest: String, force_upload: Option<bool>, user: Option<String>, mode: Option<u32>, resolve_symlinks: bool }` (`#[derive(Default)]` where sensible); `struct BuildInfo { template_id: String, build_id: String, name: Option<String>, tags: Vec<String> }` (the build-trigger result; map from generated `TemplateRequestResponseV3` — a `from_wire`).

- [ ] **Step 1: Write failing tests** — `instruction_type_roundtrip`/defaults; `build_info_from_wire` mapping a `TemplateRequestResponseV3` (honest camelCase `templateID`/`buildID`). Keep tests focused — these are plain data types.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** the data types + `BuildInfo::from_wire`. Document every item + field.
- [ ] **Step 4: Re-export + parity** — re-export the new types at the crate root. Add a `## Template (Plan 5)` section to `docs/parity-checklist.md` with a `### 5a foundation` subsection listing ReadyCmd helpers + the log/status/tag/instruction types as ✅, and a note that the build pipeline/Dockerfile parser/builder methods are Plans 5b–5d (and that addMcpServer/devcontainer-beta/CLI-logger are deferred).
- [ ] **Step 5: Full gate** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; `cargo test --doc -p e2b-rs`; `cargo doc --no-deps -p e2b-rs`; `cargo xtask codegen && git status --porcelain` → empty.
- [ ] **Step 6: Commit**
```bash
cargo fmt --all
git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs docs/parity-checklist.md
git commit -m "feat(template): add builder data types + parity checklist" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 5a is complete when:
- `crate::template` exposes `ReadyCmd` + `wait_for_port/url/process/file/timeout` (command strings byte-match the JS), `LogEntry`/`LogEntryLevel` (ANSI-stripped, `Display`), and the public wrappers `BuildStatus`/`BuildStatusReason`/`TemplateTag`/`TemplateBuildStatusResponse`/`BuildInfo`/`InstructionType`/`Instruction`/`CopyItem` — all with `from_wire` mappers, no generated type leaked.
- All re-exported at the crate root; `cargo fmt --check`, clippy `-D warnings`, `cargo test`, `cargo test --doc`, `cargo doc --no-deps` pass; codegen idempotent.
- `docs/parity-checklist.md` has the `## Template (Plan 5)` section with 5a marked done.

**Next:** Plan 5b — Dockerfile parser + file discovery/hashing + S3 presigned upload (the file-context machinery).
