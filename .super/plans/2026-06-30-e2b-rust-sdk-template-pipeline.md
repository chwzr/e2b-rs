# Template Build Pipeline (Plan 5c) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Wire the public template BUILD pipeline on top of the Plan-5b file-context internals: the `Template` builder core, the 4-phase HTTP build flow (request → parallel file uploads → trigger → poll), build-log streaming over a `tokio::sync::mpsc` channel via a `BuildHandle`, and tag operations. The many builder convenience methods (copy/run/installers/gitClone/from-variants) are Plan 5d.

**Architecture:** A public `Template` builder accumulates a base-image config + an ordered `Vec<Instruction>` + start/ready cmds + flags (force/skip-cache, cpu/mem). `build(name, opts)` does the synchronous setup — `request_build` (POST `/v3/templates` → templateID/buildID), then for each COPY instruction compute its files-hash and upload the context in PARALLEL (only when the server reports it absent), then `trigger_build` (POST `/v2/templates/{id}/builds/{bid}` with the serialized `TemplateBuildStartV2`) — and then returns a **`BuildHandle`**: a background `tokio::spawn` task runs the `wait_for_build_finish` poll loop (GET `/.../status?logsOffset=N`, ≤100 logs/call, track offset, drain on terminal), forwarding each `LogEntry` into an `mpsc::Receiver` and the terminal `BuildInfo`/error into a `oneshot`. `BuildHandle::next()` drains logs; `wait()` awaits the final `Result<BuildInfo>` — exactly the Plan-3c `CommandHandle` pattern. `build_in_background(name, opts)` runs the same setup but returns `BuildInfo` directly with NO polling/streaming. All HTTP uses the existing `ApiClient` (X-API-KEY).

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), tokio (`sync::{mpsc, oneshot}`, `spawn`, `time::sleep`), the existing `ApiClient`, the Plan-5a public types (`LogEntry`/`BuildStatus`/`BuildInfo`/`Instruction`/...) and Plan-5b internals (`calculate_files_hash`/`get_file_upload_link`/`upload_file`/`tar_file_stream`/`parse_dockerfile`).

## Global Constraints

- Package `e2b-rs`/lib `e2b_rs`; crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` only in `#[cfg(test)]`. Prefer `try_from().unwrap_or()` over `as`. `[crate::Type]` cross-module links.
- **Do NOT expose generated types.** `TemplateBuildStartV2`/`TemplateStep`/`FromImageRegistry`/`TemplateRequestResponseV3`/`TemplateBuildInfo` stay `pub(crate)`; serialize into them from hand-written types; decode + map via `from_wire`.
- **Build logs stream via `tokio::sync::mpsc` (NOT callbacks).** `build()` returns a `BuildHandle` (USER DECISION) with an mpsc `Receiver<LogEntry>` + `wait() -> Result<BuildInfo>`. Mirror `crate::sandbox::commands::CommandHandle` (the spawn + mpsc + oneshot pattern) — read it.
- **Streaming-request test caveat carries over** only if any RPC streams; the template pipeline is unary HTTP (POST/GET) so wiremock `body_partial_json`/path matching works normally.
- Every task: `cargo fmt --all` as the LAST step before `git add`; `cargo doc --no-deps` + `cargo xtask codegen` idempotency in the final gate. Commit trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference: JS `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/template/{index.ts,buildApi.ts}`.

### Pre-verified facts (confirmed at `main` = 3bb7506)
- Endpoints: `POST /v3/templates` body `{name, tags?, cpuCount?, memoryMB?}` → `TemplateRequestResponseV3 {template_id (templateID), build_id (buildID), aliases, names, public, tags}`. `POST /v2/templates/{templateID}/builds/{buildID}` body `TemplateBuildStartV2` → 204. `GET /templates/{templateID}/builds/{buildID}/status?logsOffset=N` → `TemplateBuildInfo {build_id, log_entries, logs, reason, status, template_id}`.
- Generated (`api::schema`, verify field/rename names before use): `TemplateBuildStartV2 { force: bool, from_image (fromImage)?, from_template (fromTemplate)?, from_image_registry (fromImageRegistry)?, ready_cmd (readyCmd)?, start_cmd (startCmd)?, steps: Vec<TemplateStep> }`; `TemplateStep { type_ (type), args: Vec<String>, files_hash (filesHash)?, force: bool }`; `FromImageRegistry` = enum `AwsRegistry`/`GcpRegistry`/`GeneralRegistry` (discriminated by `type`); `AwsRegistry { type_, aws_access_key_id, aws_secret_access_key, aws_region }`; `GcpRegistry { type_, service_account_json }`; `GeneralRegistry { type_, username, password }`.
- `wait_for_build_finish` algorithm (port EXACTLY, buildApi.ts:296): `logs_offset=0`, `status=Building`; `poll = get_build_status(logs_offset)` → `logs_offset += resp.log_entries.len()`, forward each entry; `while status in {Building, Waiting}`: poll, `status = resp.status`; on `Ready`/`Error` → DRAIN (`while last.log_entries.len() > 0 { last = poll }`); `Ready` → return `Ok(BuildInfo)`; `Error` → `Err(Error::Build(reason.message or "Unknown error"))`; `Waiting` → continue; sleep `logs_refresh_frequency` (default 200ms) between iterations.
- `build()` flow (index.ts:152): `request_build` → private build (instruction hashes + PARALLEL uploads + `trigger_build`) → return `BuildInfo`; then `wait_for_build_finish`. For the Rust BuildHandle: the request/upload/trigger run synchronously inside `build()`; the poll loop runs in the spawned task feeding the channel + oneshot.
- Upload orchestration (index.ts ~1150): `instructions_with_hashes()` computes `files_hash` per COPY instruction (via `calculate_files_hash`); for each with a non-null `src`+`files_hash`: `get_file_upload_link(templateID, files_hash)`; if `!present` AND a url → `upload_file(...)`. All uploads run concurrently (`Promise.all` → `futures::future::try_join_all` or `tokio::task::JoinSet`).
- `serialize(steps)` → `TemplateBuildStartV2 { start_cmd, ready_cmd, steps, force, from_image?/from_template?/from_image_registry? }`.
- `CommandHandle` (`crate::sandbox::commands::handle`) is the reference for spawn + mpsc + oneshot + `next()`/`wait()`.

---

## File Structure

- `crates/e2b-rs/src/template/builder.rs` — CREATE: public `Template` struct, `BuildOptions`, base-image config, registry types, instruction accumulation, `serialize`.
- `crates/e2b-rs/src/template/build_api.rs` — CREATE `pub(crate)`: `request_build`, `trigger_build`, `get_build_status`, the upload orchestration helper.
- `crates/e2b-rs/src/template/handle.rs` — CREATE: `BuildHandle` (mpsc + oneshot + spawn) + `wait_for_build_finish` poll task.
- `crates/e2b-rs/src/template/tags.rs` — CREATE: tag ops + exists/alias_exists.
- `crates/e2b-rs/src/template/mod.rs`, `crates/e2b-rs/src/lib.rs` — MODIFY: wire + re-export the public surface (`Template`, `BuildHandle`, `BuildOptions`, registry types).
- `docs/parity-checklist.md`, `README.md` — MODIFY (Task 5).

---

### Task 1: `Template` builder core + registry config + `serialize`

**Files:** create `template/builder.rs`; modify `template/mod.rs`, `lib.rs`.

**Interfaces (public, documented):**
- `pub struct Template { base_image: Option<String>, base_template: Option<String>, registry_config: Option<RegistryConfig>, instructions: Vec<Instruction>, start_cmd: Option<String>, ready_cmd: Option<String>, force: bool, /* skip_cache */ cpu_count: Option<u32>, memory_mb: Option<u32> }` + `Template::new()`/`Default`.
- Minimal base/finish entry points needed to make a buildable template (the FULL set of from-variants + step methods is Plan 5d): `from_image(self, base_image: &str) -> Template`, `from_base_image(self) -> Template` (default `"e2bdev/base"`), `from_template(self, template_id_or_alias: &str) -> Template`, `from_dockerfile(self, content: &str) -> Result<Template>` (uses `crate::template::dockerfile::parse_dockerfile`, applies the resulting `DockerfileAction`s to set base_image + accumulate instructions + start/ready/user/workdir/envs), `set_start_cmd(self, cmd: &str, ready: ReadyCmd) -> Template`, `set_ready_cmd(self, ready: ReadyCmd) -> Template`, `skip_cache(self) -> Template` (sets force=true).
- `pub enum RegistryConfig { Aws { access_key_id, secret_access_key, region }, Gcp { service_account_json: String }, General { username, password } }` — hand-written; `pub(crate) fn to_wire(&self) -> crate::api::schema::FromImageRegistry`.
- `pub struct BuildOptions { cpu_count: Option<u32>, memory_mb: Option<u32>, skip_cache: bool, request_timeout_ms: Option<u64>, /* connection */ api_key: Option<String>, domain: Option<String>, api_url: Option<String> }` (`#[derive(Default)]`).
- `pub(crate) fn Template::serialize(&self, steps: Vec<TemplateStep>) -> crate::api::schema::TemplateBuildStartV2` — maps start_cmd/ready_cmd/steps/force + from_image/from_template/from_image_registry (port `serialize`, index.ts:1301).
- `pub(crate) fn Template::instruction_steps(&self) -> Vec<TemplateStep>` — map each `Instruction` → generated `TemplateStep { type_: <InstructionType string>, args, files_hash, force }`.

- [ ] **Step 1: Write failing tests** — `from_dockerfile_sets_base_and_instructions` (a small Dockerfile → base_image + instructions captured via the parser actions). `serialize_maps_from_image_and_steps` — a `Template::new().from_image("node:20").set_start_cmd("npm start", wait_for_timeout(1000))`, then `serialize(instruction_steps())` → assert the `TemplateBuildStartV2` has `from_image=Some("node:20")`, `start_cmd=Some("npm start")`, `force` per skip_cache. `registry_to_wire_aws/gcp/general` — each `RegistryConfig` → the right `FromImageRegistry` variant with the right `type` discriminator. (These are pure — no HTTP.)
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** — the struct + builder entry points (consuming `self`, returning `Template` for chaining), `from_dockerfile` applying `DockerfileAction`s, `RegistryConfig::to_wire`, `serialize`/`instruction_steps`. Verify the generated `TemplateBuildStartV2`/`TemplateStep`/`FromImageRegistry` field+rename names in `api/schema.rs` before mapping. Re-export `Template`/`RegistryConfig`/`BuildOptions`.
- [ ] **Step 4: Verify & commit** — clippy `-D warnings`, `cargo doc` clean, `cargo fmt --all` last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add Template builder core + registry config + serialize" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: build-api unary calls + upload orchestration (`pub(crate)`)

**Files:** create `template/build_api.rs`; modify `template/mod.rs`.

**Interfaces (`pub(crate)`):**
- `request_build(api: &ApiClient, name: &str, tags: &[String], cpu_count: Option<u32>, memory_mb: Option<u32>) -> Result<crate::api::schema::TemplateRequestResponseV3>` — POST `/v3/templates`.
- `trigger_build(api: &ApiClient, template_id: &str, build_id: &str, body: &crate::api::schema::TemplateBuildStartV2) -> Result<()>` — POST `/v2/templates/{template_id}/builds/{build_id}` (204).
- `get_build_status(api: &ApiClient, template_id: &str, build_id: &str, logs_offset: usize) -> Result<TemplateBuildStatusResponse>` — GET `/templates/{template_id}/builds/{build_id}/status` query `logsOffset` → decode `TemplateBuildInfo` → `TemplateBuildStatusResponse::from_wire` (the Plan-5a mapper).
- `upload_build_context(api, http: &reqwest::Client, template_id: &str, instructions: &[Instruction], context: &Path) -> Result<()>` — for each COPY instruction with a `files_hash` + a `src` (args[0]): `get_file_upload_link(api, template_id, files_hash)`; if `!present` and `url.is_some()` → `tar_file_stream` + `upload_file`. Run all concurrently and collect results (any error fails the whole upload) — use `tokio::task::JoinSet` or `futures::future::try_join_all`. (The hash is precomputed on the instruction — computing it is a Task-1/builder concern; if needed, expose a `Template::instructions_with_hashes(context)` that fills `files_hash` via `calculate_files_hash`.)

- [ ] **Step 1: Write failing tests** (wiremock + an `ApiClient` pointed at the server, like the sandbox/volume api tests): `request_build_posts` (assert body `{name,...}` → returns templateID/buildID). `trigger_build_posts` (assert the POST path + a `body_partial_json` on `from_image`/`steps`). `get_build_status_maps` (GET `/.../status?logsOffset=0` → an HONEST `TemplateBuildInfo` JSON (lowercase `"status":"building"`, camelCase renames) → `TemplateBuildStatusResponse` with `status==BuildStatus::Building`). For `upload_build_context`, a focused test with a tiny context + a COPY instruction: mock the file-upload-link GET (`present:false`,url=server) + the S3 PUT; assert the upload happened; and a `present:true` case asserts NO PUT.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per buildApi.ts (`requestBuild`/`triggerBuild`/`getBuildStatus`) + the index.ts upload orchestration (parallel). Verify generated field names.
- [ ] **Step 4: Verify & commit** — gate + fmt last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template
git commit -m "feat(template): add build-api calls + parallel context upload" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `BuildHandle` + poll task + `build`/`build_in_background`

**Files:** create `template/handle.rs`; modify `template/builder.rs` (the `build`/`build_in_background` methods), `template/mod.rs`, `lib.rs`.

**Interfaces (public):**
- `pub struct BuildHandle { logs: tokio::sync::mpsc::Receiver<LogEntry>, result: Option<tokio::sync::oneshot::Receiver<Result<BuildInfo>>>, task: tokio::task::JoinHandle<()>, info: BuildInfo /* templateId/buildId known up-front */ }` with: `pub fn template_id(&self) -> &str`, `pub fn build_id(&self) -> &str`, `pub async fn next(&mut self) -> Option<LogEntry>` (drains the channel), `pub async fn wait(mut self) -> Result<BuildInfo>` (drains remaining logs then awaits the oneshot; `Err(Internal)` if the sender dropped) — MIRROR `CommandHandle::wait` (drain-then-recv, deadlock-free). `impl Drop` aborts the task.
- On `Template`: `pub async fn build(self, name: &str, opts: BuildOptions) -> Result<BuildHandle>` — resolve `ConnectionConfig`+`ApiClient`; `request_build`; fill instruction hashes + `upload_build_context` (parallel); `trigger_build(serialize(...))`; build the `BuildInfo`; SPAWN the poll task running `wait_for_build_finish(api, template_id, build_id, logs_refresh_frequency, sender)` which forwards each `LogEntry` to the mpsc and sends the terminal `Result<BuildInfo>` to the oneshot; return the `BuildHandle`. `pub async fn build_in_background(self, name: &str, opts: BuildOptions) -> Result<BuildInfo>` — same setup (request/upload/trigger) but NO poll task — return `BuildInfo` immediately.
- `pub(crate) async fn wait_for_build_finish(api: Arc<ApiClient>, template_id, build_id, logs_refresh_frequency_ms: u64, logs: mpsc::Sender<LogEntry>) -> Result<BuildInfo>` — the poll loop (port buildApi.ts:296 exactly): offset tracking, forward entries to the channel, terminal drain, `Ready`→`Ok(BuildInfo)`, `Error`→`Err(Error::Build(reason.message))`. `name` parsing: `"name:tag"` → name + tag (port `normalizeBuildArguments`).

- [ ] **Step 1: Write failing tests** (wiremock): `build_streams_logs_then_ready` — mock `POST /v3/templates` (→ ids), no-COPY template so NO uploads, `POST /v2/.../builds/...` (204), and `GET /.../status` returning FIRST `{"status":"building","logEntries":[{"level":"info","message":"step 1",...}]}` then `{"status":"ready","logEntries":[]}` (use wiremock response sequencing / `up_to_n_times` or `Mock`-with-expect to return different bodies on successive calls). Assert: `handle.next().await` yields the "step 1" `LogEntry`; `handle.wait().await` → `Ok(BuildInfo)` with the right template_id. `build_error_status_fails` — status returns `{"status":"error","reason":{"message":"boom"},"logEntries":[]}` → `handle.wait()` → `Err(Error::Build(msg))` containing "boom". `build_in_background_skips_poll` — asserts it returns `BuildInfo` and does NOT call the status endpoint. (For successive different responses to the same GET path, use wiremock's `respond_with` with a stateful responder or mount multiple `Mock`s scoped by query/`expect` — document the approach.)
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** — `BuildHandle` (mirror `CommandHandle`: spawn forwards logs→mpsc, terminal→oneshot; `wait` drains-then-awaits; Drop aborts). `wait_for_build_finish` poll loop. `build`/`build_in_background`. `logs_refresh_frequency` default 200ms (configurable later). Re-export `BuildHandle`.
- [ ] **Step 4: Verify & commit** — gate + fmt last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add BuildHandle log streaming + build/build_in_background" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Tag operations + exists/alias_exists + get_build_status (public)

**Files:** create `template/tags.rs`; modify `template/mod.rs`, `lib.rs`.

**Interfaces (public assoc fns — they take a `BuildOptions`-like connection opts; reuse `BuildOptions` or a small `TemplateApiOpts`):**
- `Template::get_build_status(template_id: &str, build_id: &str, opts) -> Result<TemplateBuildStatusResponse>` (public wrapper over `get_build_status`).
- `Template::exists(name: &str, opts) -> Result<bool>` and `Template::alias_exists(alias: &str, opts) -> Result<bool>` — `GET /templates/aliases/{alias}`; 200→true, 404→false, 403→Err (port checkAliasExists).
- `Template::assign_tags(target_name: &str, tags: &[String], opts) -> Result<TemplateTagInfo>` (POST `/templates/tags` `{target, tags}` → `AssignedTemplateTags`); `Template::remove_tags(name: &str, tags: &[String], opts) -> Result<()>` (DELETE `/templates/tags` `{name, tags}` → 204); `Template::get_tags(template_id: &str, opts) -> Result<Vec<TemplateTag>>` (GET `/templates/{template_id}/tags` → `Vec<TemplateTag>` via the Plan-5a `TemplateTag::from_wire`).
- `pub struct TemplateTagInfo { build_id: String, tags: Vec<String> }` (+ from_wire from `AssignedTemplateTags`).

- [ ] **Step 1: Write failing tests** (wiremock, honest fixtures): `exists_true_false_forbidden` (200/404/403 → true/false/Err); `assign_tags` (POST body asserted → TemplateTagInfo); `remove_tags` (DELETE 204 → Ok); `get_tags` (GET → Vec<TemplateTag>, honest `buildID`/`createdAt`/uuid).
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per index.ts (`exists`/`aliasExists`/`assignTags`/`removeTags`/`getTags`/`getBuildStatus`) + buildApi (`checkAliasExists`/`assignTags`/`removeTags`/`getTemplateTags`). Re-export `TemplateTagInfo`.
- [ ] **Step 4: Verify & commit** — gate + fmt last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add tag operations + exists/alias_exists + get_build_status" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Quickstart doctest + parity + full gate

**Files:** modify `crates/e2b-rs/src/lib.rs` (crate-doc), `docs/parity-checklist.md`, `README.md`.

- [ ] **Step 1: Quickstart** — add a `## Templates` `no_run` doctest mirroring the existing sections: `let template = e2b_rs::Template::new().from_image("node:20").set_start_cmd("npm start", e2b_rs::wait_for_timeout(20_000));` then `let mut build = template.build("my-tpl", Default::default()).await?; while let Some(log) = build.next().await { println!("{log}"); } let info = build.wait().await?;`. Confirm signatures compile under `no_run`.
- [ ] **Step 2: Parity** — fill the `### 5c — Build pipeline` subsection: build/build_in_background (BuildHandle + mpsc log streaming), request/trigger/poll, parallel context upload, registry config (AWS/GCP/generic), tags, exists/alias_exists, get_build_status. Note the BuildHandle mirrors CommandHandle.
- [ ] **Step 3: README** — short template build snippet.
- [ ] **Step 4: Full gate** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; `cargo test --doc -p e2b-rs`; `cargo doc --no-deps -p e2b-rs`; `cargo xtask codegen && git status --porcelain` → empty.
- [ ] **Step 5: Commit**
```bash
cargo fmt --all && git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(template): document build-pipeline quickstart and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 5c is complete when:
- `Template::{new, from_image, from_base_image, from_template, from_dockerfile, set_start_cmd, set_ready_cmd, skip_cache, build, build_in_background, get_build_status, exists, alias_exists, assign_tags, remove_tags, get_tags}` work; `build` returns a `BuildHandle` streaming `LogEntry` over mpsc with `wait() -> Result<BuildInfo>`.
- The 4-phase pipeline (request → parallel uploads → trigger → poll-with-drain) matches the JS; `error` status → `Err(Error::Build)`; registry creds serialize to the right `FromImageRegistry` variant; no generated type exposed.
- `Template`/`BuildHandle`/`BuildOptions`/`RegistryConfig`/`TemplateTagInfo` re-exported at the crate root; `cargo fmt --check`, clippy `-D warnings`, `cargo test`, `cargo test --doc`, `cargo doc` pass; codegen idempotent.
- `docs/parity-checklist.md` + a `## Templates` quickstart reflect the pipeline.

**Carry-forwards (documented):** the builder convenience methods (copy/run/env/installers/gitClone/from-variants) are Plan 5d; stack-trace-to-build-step error mapping (JS `getBuildStepIndex`) is a simplification (surface `reason.message`); `logs_refresh_frequency`/cpu/mem knobs minimal; LogEntryStart/End synthetic markers optional.

**Next:** Plan 5d — the builder convenience methods (copy/copyItems/remove/rename/makeDir/makeSymlink/runCmd/setWorkdir/setUser/setEnvs/pipInstall/npmInstall/aptInstall/gitClone + the from* image variants) + final polish. The LAST sub-plan of the project.
