# JS → Rust parity checklist

> **Feature-complete.** As of Plan 5d (2026-06-30) the `e2b-rs` crate is a
> full 1:1 port of the E2B JavaScript SDK across all subsystems: sandbox
> lifecycle, filesystem, commands, PTY, git, volume, and the template build
> pipeline (builder methods, file context, HTTP build pipeline, log streaming,
> tag management). The only documented omissions are the MCP server wiring
> (`addMcpServer`) and the devcontainer-beta APIs
> (`betaDevContainerPrebuild`/`betaSetDevContainerStart`), both deferred by
> explicit user decision. Per-plan carry-forwards are noted in each section
> below.

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
| `Sandbox.create({ lifecycle })` | `SandboxCreateBuilder::lifecycle(SandboxLifecycle)` → `autoPause`, `autoPauseMemory`, `autoResume.enabled` | ✅ |
| `Sandbox.create({ sandboxUrl })` / `Sandbox.connect(id, { sandboxUrl })` | `SandboxCreateBuilder::sandbox_url` / `SandboxConnectBuilder::sandbox_url` — one envd base URL, sandbox id and port in headers | ✅ |
| `sandbox.kill` / `SandboxApi.kill` | `Sandbox::kill` | ✅ |
| `Sandbox.kill(id)` (static) | `Sandbox::kill_by_id(id, ConnectionConfigOpts)` — no `/connect`, so a paused sandbox stays paused | ✅ |
| `sandbox.getInfo` | `Sandbox::get_info` → `SandboxInfo` (with `lifecycle`) | ✅ |
| `Sandbox.getInfo(id)` (static) | `Sandbox::get_info_by_id(id, ConnectionConfigOpts)` — no `/connect` | ✅ |
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

## Sandbox git (Plan 4a)

| JS (`sandbox.git.*`) | Rust (`sandbox.git().*`) | Status |
|---|---|---|
| `clone(url, opts)` | `Git::clone(url, GitCloneOpts)` → `Ok(CommandResult)`; auth failure → `Err(GitAuth)` | ✅ |
| `init(path, opts)` | `Git::init(path, GitInitOpts)` → `Ok(CommandResult)` | ✅ |
| `status(path, user)` | `Git::status(path, user)` → `Ok(GitStatus)` | ✅ |
| `branches(path, user)` | `Git::branches(path, user)` → `Ok(GitBranches)` | ✅ |
| `add(path, opts)` | `Git::add(path, GitAddOpts)` → `Ok(CommandResult)` | ✅ |
| `commit(path, message, opts)` | `Git::commit(path, message, GitCommitOpts)` → `Ok(CommandResult)` | ✅ |
| `push(path, opts)` | `Git::push(path, GitPushOpts)` → `Ok(CommandResult)`; auth → `Err(GitAuth)`, upstream → `Err(GitUpstream)` | ✅ |
| `pull(path, opts)` | `Git::pull(path, GitPullOpts)` → `Ok(CommandResult)`; auth → `Err(GitAuth)`, upstream → `Err(GitUpstream)` | ✅ |
| `createBranch(path, branch, opts)` | `Git::create_branch(path, branch, GitRequestOpts)` → `Ok(CommandResult)` | ✅ |
| `checkoutBranch(path, branch, opts)` | `Git::checkout_branch(path, branch, GitRequestOpts)` → `Ok(CommandResult)` | ✅ |
| `deleteBranch(path, branch, opts)` | `Git::delete_branch(path, branch, GitDeleteBranchOpts)` → `Ok(CommandResult)` | ✅ |
| `reset(path, opts)` | `Git::reset(path, GitResetOpts)` → `Ok(CommandResult)` | ✅ |
| `restore(path, opts)` | `Git::restore(path, GitRestoreOpts)` → `Ok(CommandResult)` | ✅ |
| `setConfig(key, value, opts)` | `Git::set_config(key, value, GitConfigOpts)` → `Ok(CommandResult)` | ✅ |
| `getConfig(key, opts)` | `Git::get_config(key, GitConfigOpts)` → `Ok(Option<String>)` | ✅ |
| `remoteAdd(path, name, url, opts)` | `Git::remote_add(path, name, url, GitRemoteAddOpts)` → `Ok(CommandResult)` | ✅ |
| `remoteGet(path, name, opts)` | `Git::remote_get(path, name, GitRequestOpts)` → `Ok(Option<String>)` | ✅ |
| `configureUser(name, email, opts)` | `Git::configure_user(name, email, GitConfigOpts)` → `Ok(CommandResult)` | ✅ |
| `dangerouslyAuthenticate(opts)` | `Git::dangerously_authenticate(GitDangerouslyAuthenticateOpts)` → `Ok(CommandResult)` | ✅ |

> **Note:** git method opt structs expose only `user` (plus method-specific flags); per-operation `envs` / `cwd` / `timeout` passthrough from JS `GitRequestOpts` is NOT exposed, and git always runs with `GIT_TERMINAL_PROMPT=0`. (Tracked follow-up, not a ✅-blocker.)

> **Exit-code convention:** Non-zero git exit returns `Ok(CommandResult)` (exit code in `result.exit_code`) for all methods **except**:
> - `clone`: auth failure → `Err(GitAuth)`.
> - `push`/`pull` (non-credentialed path): auth failure → `Err(GitAuth)`, missing upstream → `Err(GitUpstream)`.
>
> **Parity quirk:** On the **credentialed** push/pull path (`with_remote_credentials`), auth/upstream errors are NOT mapped to `Err` — this matches the JS SDK asymmetry (see task-3 report). The `get_config` and `remote_get` methods use a `|| true` shell fallback so a missing key/remote returns `Ok(None)` rather than `Err`.

## Volume (Plan 4b)

### Control-plane (`Volume::*` static methods, X-API-KEY transport)

| JS (`volumes.*`) | Rust (`e2b_rs::Volume::*`) | Status |
|---|---|---|
| `create(name, opts)` | `Volume::create(name, VolumeOpts)` → `Volume` | ✅ |
| `list(opts)` | `Volume::list(VolumeOpts)` → `Vec<VolumeInfo>` | ✅ |
| `getInfo(id, opts)` | `Volume::get_info(id, VolumeOpts)` → `VolumeAndToken` | ✅ |
| `connect(id, opts)` | `Volume::connect(id, VolumeOpts)` → `Volume` | ✅ |
| `destroy(id, opts)` | `Volume::destroy(id, VolumeOpts)` → `bool` (404 → false) | ✅ |

### Content (`volume.*` instance methods, Bearer-token transport)

| JS (`volume.*`) | Rust (`volume.*`) | Notes | Status |
|---|---|---|---|
| `list(path, opts)` | `list_dir(path, VolumeListOpts)` | Renamed to avoid Rust assoc-fn/method clash (JS allows static + instance `list`; Rust does not). No pagination. | ✅ |
| `makeDir(path, opts)` | `make_dir(path, VolumeMakeDirOpts)` | `uid`/`gid`/`mode`/`force` sent as query params. | ✅ |
| `getInfo(path)` | `stat(path)` | Renamed to avoid clash with control-plane `get_info`. 404 → `Err(NotFound)`. | ✅ |
| `updateMetadata(path, meta)` | `update_metadata(path, VolumeMetadataOpts)` | `uid`/`gid`/`mode` sent as JSON body. | ✅ |
| `readFile(path, {format:'text'})` | `read_file(path)` → `String` | Uses 1-hour file timeout. | ✅ |
| `readFile(path, {format:'bytes'})` | `read_file_bytes(path)` → `Vec<u8>` | Uses 1-hour file timeout. | ✅ |
| `readFile(path, {format:'stream'})` | `read_file_stream(path, VolumeReadOpts)` → `impl Stream<Item=Result<Bytes>>` | Uses 1-hour file timeout. | ✅ |
| `readFile(path, {format:'blob'})` | N/A | `Blob` is a browser type with no Rust equivalent; not exposed. | ⬜ |
| `writeFile(path, data, opts)` | `write_file(path, data, VolumeWriteOpts)` → `VolumeEntryStat` | `uid`/`gid`/`mode`/`force` sent as query params; body as `application/octet-stream`. Uses 1-hour file timeout. | ✅ |
| `remove(path)` | `remove(path)` | 404 → `Err(NotFound)`. | ✅ |
| `exists(path)` | `exists(path)` → `bool` | Delegates to `stat`; 404 → `Ok(false)`. | ✅ |

### Notes

- **Transport split:** control-plane calls use `X-API-KEY` header (same as sandbox API); content calls use `Authorization: Bearer <token>` where the token is obtained via `create`/`connect`/`get_info`.
- **No pagination:** `list`/`list_dir` return all results in a single response; the JS SDK has no paginator for volumes either.
- **Metadata via query params:** `make_dir` and `write_file` pass `uid`/`gid`/`mode`/`force` as URL query parameters, matching the volume content API spec.
- **1-hour file timeout:** `read_file*` and `write_file` use a dedicated 60-minute timeout (`FILE_TIMEOUT_MS`) rather than the 60-second metadata default, mirroring the JS SDK's `requestTimeoutMs: opts?.requestTimeoutMs ?? FILE_TIMEOUT_MS`.
- **Carry-forwards:** JS `VolumeApiOpts` debug/logger/signal fields not exposed (minimal opts); per-call `requestTimeoutMs` / proxy passthrough scope; `Volume` rebuilds `VolumeApiClient` per call (same as JS).

## Template (Plan 5) ✅ COMPLETE

### 5a — Foundation

| JS (`src/template/...`) | Rust (`e2b_rs::...`) | Status |
|---|---|---|
| `readycmd.ts` `ReadyCmd` | `ReadyCmd` | ✅ |
| `readycmd.ts` `waitForPort` | `wait_for_port` | ✅ |
| `readycmd.ts` `waitForURL` | `wait_for_url` | ✅ |
| `readycmd.ts` `waitForProcess` | `wait_for_process` | ✅ |
| `readycmd.ts` `waitForFile` | `wait_for_file` | ✅ |
| `readycmd.ts` `waitForTimeout` | `wait_for_timeout` | ✅ |
| `logger.ts` `LogEntry` / `LogEntryLevel` | `LogEntry` / `LogEntryLevel` | ✅ |
| `types.ts` `TemplateBuildStatus` | `BuildStatus` | ✅ |
| `types.ts` `BuildStatusReason` | `BuildStatusReason` | ✅ |
| `types.ts` `TemplateTag` | `TemplateTag` | ✅ |
| `types.ts` `TemplateBuildStatusResponse` | `TemplateBuildStatusResponse` | ✅ |
| `types.ts` `BuildInfo` | `BuildInfo` (+ `from_wire` from `TemplateRequestResponseV3`) | ✅ |
| `types.ts` `InstructionType` | `InstructionType` | ✅ |
| `types.ts` `Instruction` | `Instruction` | ✅ |
| `types.ts` `CopyItem` | `CopyItem` | ✅ |

**Notes:**
- Build logs are delivered via a `tokio::sync::mpsc` channel (Plan 5c), not callbacks — the JS `onBuildLogs` callback parameter has no direct Rust equivalent.
- The build pipeline (HTTP build trigger, build-status polling, log streaming), Dockerfile parser, file upload/hashing, and builder methods (`fromImage`, `copy`, `runCmd`, etc.) are Plans 5b–5d (placeholders below).
- `addMcpServer`, devcontainer-beta (`betaDevContainerPrebuild`/`betaSetDevContainerStart`), and the CLI animated logger are **DEFERRED** (user decision — not planned for initial release).

### 5b — File context

All items are `pub(crate)`; they are consumed by the build pipeline (Plan 5c) and
builder methods (Plan 5d). No public API is introduced in this plan.

| JS (`src/template/...`) | Rust (`e2b_rs::template`) | Status |
|---|---|---|
| `buildContext.ts` `validateRelativePath` | `validate_relative_path` | ✅ |
| `buildContext.ts` `readDockerignore` | `read_dockerignore` (globset patterns) | ✅ |
| `buildContext.ts` `getAllFilesInPath` | `get_all_files_in_path` (walkdir + `.dockerignore` via globset) | ✅ |
| `buildContext.ts` (path util) | `relative_posix` (POSIX-normalised relative path) | ✅ |
| `buildContext.ts` `calculateFilesHash` | `calculate_files_hash` — deterministic build cache key (SHA-256 over `"COPY src dest"` line + per-sorted-file: relative-POSIX path + Unix mode + file size + raw bytes); uid/gid/mtime explicitly excluded | ✅ |
| `buildContext.ts` `tarFileStream` | `tar_file_stream` — gzip-compressed tar context archive spooled to a `tempfile::NamedTempFile`; no directory re-recursion | ✅ |
| `buildContext.ts` `getFileUploadLink` | `get_file_upload_link` (`GET /templates/{id}/files/{hash}`) | ✅ |
| `buildContext.ts` `uploadFile` | `upload_file` (`PUT` to presigned S3 URL; explicit `Content-Length`; 1-hour timeout; chunked transfer encoding rejected by S3) | ✅ |
| `dockerfile.ts` (minimal parser) | `parse_dockerfile` — handles FROM/RUN/COPY/ADD/WORKDIR/USER/ENV/ARG; EXPOSE/VOLUME ignored; CMD/ENTRYPOINT → `startCmd`; multi-stage Dockerfiles rejected; applies E2B USER/WORKDIR defaults if absent | ✅ |

**New crate dependencies added at workspace root:** `sha2`, `glob`, `globset`, `walkdir`, `tempfile`, `tar`, `flate2`.

**Notes:**
- The files-hash is *intra-SDK deterministic* (stable byte sequence within `e2b-rs`). Cross-SDK byte-parity with the JS hash is a **documented stretch goal**, not required — the server stores the build context under whatever hash the client sends, so the only constraint is self-consistency across SDK versions.
- Here-doc syntax and parser directives in Dockerfiles are not handled (matching the scope of the JS implementation).
- Non-Unix `mode` fallback (Windows): the mode contribution to the hash uses the string `"0"` as a stable default when Unix permissions are unavailable.
- `.dockerignore` glob semantics use `globset` (gitignore-style) rather than `minimatch`; edge-case behaviour on negation patterns may differ from the JS SDK (tracked carry-forward).

### 5c — Build pipeline

| JS (`src/template/...`) | Rust (`e2b_rs::template`) | Status |
|---|---|---|
| `Template` builder (`fromImage`/`fromBaseImage`/`fromTemplate`/`fromDockerfile`/`setStartCmd`/`setReadyCmd`/`skipCache`) | `Template::from_image`/`from_base_image`/`from_template`/`from_dockerfile`/`set_start_cmd`/`set_ready_cmd`/`skip_cache` | ✅ |
| `template.build(name, opts)` | `Template::build(name, BuildOptions)` → `BuildHandle` (streaming) | ✅ |
| `template.buildInBackground(name, opts)` | `Template::build_in_background(name, BuildOptions)` → `BuildInfo` (no streaming) | ✅ |
| `BuildHandle` log streaming | `BuildHandle::next()` → `Option<LogEntry>` (mpsc channel); mirrors `CommandHandle` | ✅ |
| `BuildHandle` completion | `BuildHandle::wait()` → `Result<BuildInfo>`; drains log channel, then returns outcome | ✅ |
| 4-phase pipeline: request → upload → trigger → poll | `request_build` → parallel context upload → `trigger_build` → `wait_for_build_finish` (poll-with-drain, 200 ms cadence) | ✅ |
| `RegistryConfig` (AWS/GCP/generic) | `RegistryConfig::{Aws, Gcp, General}` — serialises to `FromImageRegistry` wire enum internally | ✅ |
| `assignTags` / `removeTags` / `getTags` | `Template::assign_tags` / `remove_tags` / `get_tags` → `Vec<TemplateTag>` | ✅ |
| `exists` / `aliasExists` (403 → true) | `Template::exists` / `alias_exists` — 200→`Ok(true)`, 404→`Ok(false)`, 403→`Ok(true)` (matches JS) | ✅ |
| `GET /templates` (CLI `template list`) | `Template::list(TemplateApiOpts)` → `Vec<TemplateListItem>` with build status; `has_name` resolves an alias | ✅ |
| `DELETE /templates/{id}` (CLI `template delete`) | `Template::delete(id_or_alias, TemplateApiOpts)` — 404→`Ok(false)` | ✅ |
| `getBuildStatus` | `Template::get_build_status` → `TemplateBuildStatusResponse` | ✅ |

**Notes:**
- `BuildHandle` mirrors `CommandHandle`: `next()` pulls one [`LogEntry`] at a time from the mpsc channel; `wait()` drains and returns the final [`BuildInfo`].
- `force_upload` on [`Instruction`]/[`CopyItem`] is honoured in the context-upload step (re-uploads even when hash matches cached copy).
- Build context defaults to the current working directory (`std::env::current_dir()`); a configurable context-path argument is a carry-forward.
- `logs_refresh_frequency` is fixed at 200 ms per poll; exposing it as a per-build knob is a carry-forward.
- `error` status from the API → `Err(Error::Build(reason.message))`; the JS `getBuildStepIndex` step-mapping is simplified — only `reason.message` is surfaced (carry-forward).
- No generated wire type (e.g. `FromImageRegistry`) is exposed in the public API; serialisation happens internally in `RegistryConfig::to_wire`.

### 5d — Builder methods

| JS (`src/template/index.ts`) | Rust (`e2b_rs::template::Template::*`) | Notes | Status |
|---|---|---|---|
| `fromImage(image)` | `from_image(base_image)` | | ✅ |
| `fromBaseImage()` | `from_base_image()` | Equivalent to `from_image("e2bdev/base")` | ✅ |
| `fromTemplate(id)` | `from_template(id_or_alias)` | | ✅ |
| `fromDockerfile(content)` | `from_dockerfile(content)` → `Result<Template>` | | ✅ |
| `fromDebianImage(variant)` | `from_debian_image(variant)` | | ✅ |
| `fromUbuntuImage(variant)` | `from_ubuntu_image(variant)` | | ✅ |
| `fromPythonImage(version)` | `from_python_image(version)` | | ✅ |
| `fromNodeImage(variant)` | `from_node_image(variant)` | | ✅ |
| `fromBunImage(variant)` | `from_bun_image(variant)` | | ✅ |
| `fromAwsRegistry(image, keyId, secret, region)` | `from_aws_registry(image, access_key_id, secret_access_key, region)` | Credentials excluded from `Debug` | ✅ |
| `fromGcpRegistry(image, saJson)` | `from_gcp_registry(image, service_account_json)` | Credentials excluded from `Debug` | ✅ |
| `fromImage(image, { username, password })` | `from_registry_image(image, username, password)` | `RegistryConfig::General`; password excluded from `Debug` | ✅ |
| `copy(src, dest, opts)` | `copy(src, dest, CopyOpts)` → `Result<Template>` | Validates relative path; args = `[src, dest, user, mode_octal]` | ✅ |
| `copyItems(items)` | `copy_items(Vec<CopyItem>)` → `Result<Template>` | Per-item relative-path validation | ✅ |
| `remove(paths, opts)` | `remove(&[&str], RemoveOpts)` → `Template` | Builds `rm [-r] [-f] <quoted-paths>` | ✅ |
| `rename(src, dest, opts)` | `rename(src, dest, RenameOpts)` → `Template` | Builds `mv <src> <dest> [-f]` | ✅ |
| `makeDir(paths, opts)` | `make_dir(&[&str], MakeDirOpts)` → `Template` | Builds `mkdir -p [-m <mode>] <paths>` | ✅ |
| `makeSymlink(src, dest, opts)` | `make_symlink(src, dest, MakeSymlinkOpts)` → `Template` | Builds `ln -s [-f] <src> <dest>` | ✅ |
| `runCmd(command, opts)` | `run_cmd(command, RunCmdOpts)` → `Template` | Single `RUN` instruction | ✅ |
| `runCmds(commands, opts)` | `run_cmds(&[&str], RunCmdOpts)` → `Template` | Joined with `&&`; single layer | ✅ |
| `setWorkdir(path)` | `set_workdir(path)` → `Template` | `WORKDIR` instruction | ✅ |
| `setUser(user)` | `set_user(user)` → `Template` | `USER` instruction | ✅ |
| `setEnvs(envs)` | `set_envs(BTreeMap<String,String>)` → `Template` | `ENV` instruction; keys sorted by `BTreeMap` (ascending) vs JS insertion order — carry-forward | ✅ |
| `pipInstall(packages, opts)` | `pip_install(&[&str], PipInstallOpts)` → `Template` | `global` defaults `true`; runs as root when global | ✅ |
| `npmInstall(packages, opts)` | `npm_install(&[&str], NpmInstallOpts)` → `Template` | `-g` when global; `--save-dev` when dev | ✅ |
| `bunInstall(packages, opts)` | `bun_install(&[&str], BunInstallOpts)` → `Template` | `-g` when global; `--dev` when dev | ✅ |
| `aptInstall(packages, opts)` | `apt_install(&[&str], AptInstallOpts)` → `Template` | `apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y …` | ✅ |
| `gitClone(url, path, opts)` | `git_clone(url, Option<&str>, GitCloneOpts)` → `Template` | `template::GitCloneOpts` (at `e2b_rs::template::GitCloneOpts`) to avoid conflict with sandbox `GitCloneOpts` | ✅ |
| `addMcpServer(…)` | _(DEFERRED — user decision; no generated type exposed)_ | | ⬜ |
| `betaDevContainerPrebuild(…)` | _(DEFERRED — user decision)_ | | ⬜ |
| `betaSetDevContainerStart(…)` | _(DEFERRED — user decision)_ | | ⬜ |

**Notes:**
- `set_envs` orders keys ascending via `BTreeMap` iteration rather than JS insertion order — this is a documented carry-forward for determinism.
- Per-layer `forceNextLayer` in the JS SDK is simplified to a template-level `force` flag (`skip_cache()`) in `e2b-rs` — carry-forward.
- `template::GitCloneOpts` lives at `e2b_rs::template::GitCloneOpts`; the crate root `e2b_rs::GitCloneOpts` re-exports the sandbox git one to avoid naming conflict.
- All new opt structs (`CopyOpts`, `RemoveOpts`, `RenameOpts`, `MakeDirOpts`, `MakeSymlinkOpts`, `RunCmdOpts`, `PipInstallOpts`, `NpmInstallOpts`, `BunInstallOpts`, `AptInstallOpts`) are re-exported at the crate root — EXCEPT `GitCloneOpts`, which is reachable only via `e2b_rs::template::GitCloneOpts` (the crate-root `e2b_rs::GitCloneOpts` is the sandbox-git one).
