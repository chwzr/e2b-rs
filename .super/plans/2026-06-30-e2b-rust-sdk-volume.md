# Volume (Plan 4b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the top-level `Volume` resource — persistent network volumes — matching the E2B JS SDK 1:1. `Volume` is NOT attached to `Sandbox`; it is a standalone type with control-plane CRUD (create/list/get_info/connect/destroy) and volume-content file ops (list/make_dir/get_info/update_metadata/read_file/write_file/remove/exists).

**Architecture:** Two transports.
1. **Control-plane** — the existing `crate::api::client::ApiClient` (X-API-KEY auth) against `/volumes` on `https://api.{domain}`. Mirror `crate::sandbox::api` exactly (it already uses `ApiClient::request`/`request_unit`).
2. **Volume-content** — a NEW `crate::volume::client::VolumeApiClient` (HTTP **Bearer** auth with the per-volume token) against `/volumecontent/{volumeID}/*` on the SAME `https://api.{domain}`. Model it on `ApiClient`/`EnvdApiClient`: a `reqwest::Client` with an `Authorization: Bearer <token>` default header + base_url, JSON `request_json<T>` for dir/path ops, and octet-stream byte I/O (`read_bytes`/`read_stream`/`write_bytes`) for `/file` — exactly how Plan-3b `EnvdApiClient` handled `/files`.

Generated wire types stay `pub(crate)` and are wrapped behind hand-written public types. The `Volume` instance holds the resolved connection config + `{volume_id, name, token}`; each content method builds a `VolumeApiClient` from that (JS rebuilds the client per call — mirror by holding the config and constructing the client in each method, OR build it once in the constructor; prefer build-once in the `Volume` for simplicity unless a per-call timeout differs).

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), `reqwest` 0.13 (json/stream/octet-stream — already a dep with the 3b features gzip/multipart/stream), `tokio::sync::mpsc` not needed here (reads are `impl Stream` like 3b byte reads), `chrono` (VolumeEntryStat timestamps), the generated `api::schema` + `volume::schema` types.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; all crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` only in `#[cfg(test)]`. Prefer `try_from().unwrap_or()` over `as`. `[crate::Type]` for cross-module intra-doc links.
- **Do NOT expose generated types.** `api::schema::{NewVolume,Volume,VolumeAndToken,VolumeToken}` and `volume::schema::{VolumeDirectoryListing,VolumeEntryStat,VolumeEntryStatType,Error}` stay `pub(crate)`; wrap them in hand-written public types.
- **Honest fixtures (non-negotiable — this class of bug hit 3a/3b):** every test fixture JSON must match the REAL wire. The volume-content `VolumeEntryStat.type` enum is LOWERCASE (`"file"`/`"directory"`/`"symlink"`/`"unknown"`) per the generated `VolumeEntryStatType` serde renames — map via the generated enum, never hand-write uppercase. Control-plane `volumeID` → `volume_id` (generated serde rename) — fixtures use `"volumeID"`.
- Every public item + field documented. Every task: `cargo fmt --all` before commit; run `cargo doc --no-deps -p e2b-rs` in the gate. Commit trailer (exact): `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference (source of truth): JS `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/volume/{index.ts,client.ts,types.ts}`; specs `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/spec/openapi.yml` (control-plane `/volumes`, ~line 3555) + `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/spec/openapi-volumecontent.yml` (content endpoints).

### Pre-verified facts (confirmed against the codebase/specs at `main` = 3c2c4e8)
- Generated `volume::schema` (mod `crate::volume::schema`): `Error{code,message}`; `VolumeDirectoryListing(pub Vec<VolumeEntryStat>)` (`#[serde(transparent)]`); `VolumeEntryStat{ atime/ctime/mtime: chrono::DateTime<Utc>, gid/mode/uid: u32, name/path: String, size: i64, target: Option<String> (skip-if-none), type_ (rename "type"): VolumeEntryStatType }`; `VolumeEntryStatType` enum renames `unknown`/`file`/`directory`/`symlink` (LOWERCASE).
- Generated `api::schema`: `NewVolume{ name: NewVolumeName }` (name is a validated newtype `NewVolumeName` — construct via its `TryFrom<String>`/`FromStr`, propagate the conversion error as `Error::InvalidArgument`); `Volume{ name: String, volume_id (rename "volumeID"): String }`; `VolumeAndToken{ name: String, token: String, volume_id (rename "volumeID"): String }`; `VolumeToken{ token: String }`.
- Control-plane endpoints (openapi.yml): `POST /volumes` body `NewVolume` → `VolumeAndToken`; `GET /volumes` → `[Volume]`; `GET /volumes/{volumeID}` → `VolumeAndToken`; `DELETE /volumes/{volumeID}` → 204. NO pagination.
- Volume-content endpoints (openapi-volumecontent.yml), auth = HTTP **bearer**: `/volumecontent/{volumeID}/dir` GET(list; query `path`,`depth`)→`VolumeDirectoryListing`, POST(make_dir; query `path`,`uid?`,`gid?`,`mode?`,`force?`)→`VolumeEntryStat`; `/volumecontent/{volumeID}/path` GET(get_info; query `path`)→`VolumeEntryStat`, PATCH(update_metadata; query `path`; body `{uid?,gid?,mode?}`)→`VolumeEntryStat`, DELETE(remove; query `path`)→204; `/volumecontent/{volumeID}/file` GET(read_file; query `path`)→octet-stream bytes, PUT(write_file; query `path`,`uid?`,`gid?`,`mode?`,`force?`; body application/octet-stream)→`VolumeEntryStat`. Metadata (uid/gid/mode/force) are QUERY params here (NOT `X-Metadata-*` headers like 3b `/files`). NO pagination on `/dir`.
- `ApiClient` (control-plane) is reused as-is: `ApiClient::new(&ConnectionConfig, validate: bool) -> Result<ApiClient>`; `request<T: DeserializeOwned>(Method, path: &str, query: &[(&str,&str)], body: Option<&impl Serialize>) -> Result<T>`; `request_unit(...)-> Result<()>`. See `crate::sandbox::api` for the exact call pattern (e.g. `api.request(Method::POST, "/sandboxes", &[], Some(&body))`). 404 handling: mirror `sandbox::api`'s helpers.
- `ConnectionConfig::new(ConnectionConfigOpts{ api_key, domain, api_url, validate_api_key, .. })` resolves `api_url = E2B_API_URL || https://api.{domain}`, `api_key = E2B_API_KEY`, `domain = E2B_DOMAIN || e2b.app`. Volume content uses the SAME `api_url` base with Bearer (the volume token), not the API key.
- JS `VolumeConnectionConfig` (client.ts): `apiUrl = opts.apiUrl || E2B_VOLUME_API_URL || (debug ? http://localhost:8080 : https://api.{domain})`; `token = opts.token || volume.token`; `requestTimeoutMs` default 60s; `FILE_TIMEOUT_MS = 3_600_000` (1h) used for file read/write. VolumeApiClient sets `Authorization: Bearer {token}` + default headers.
- Plan-3b precedent for byte I/O + streaming + status→Error mapping lives in `crate::envd::rest` (`EnvdApiClient`) and `crate::sandbox::filesystem::io` — read these to mirror `read_bytes`/`read_stream`/`write_bytes` and the gzip-decompress-on-read behavior.

---

## File Structure

- `crates/e2b-rs/src/volume/mod.rs` — MODIFY: currently only `pub(crate) mod schema;`. Add `pub(crate) mod client; pub mod types; mod api;` + the `Volume` struct (or put `Volume` in `volume.rs`). Re-export public types.
- `crates/e2b-rs/src/volume/types.rs` — CREATE: public `VolumeFileType`, `VolumeInfo`, `VolumeAndToken`, `VolumeEntryStat`, `VolumeMetadataOpts`, `VolumeWriteOpts`, `VolumeReadOpts`, `VolumeListOpts`/`VolumeMakeDirOpts` + the generated→public mapping fns.
- `crates/e2b-rs/src/volume/client.rs` — CREATE: `VolumeApiClient` (Bearer transport).
- `crates/e2b-rs/src/volume/volume.rs` — CREATE: the public `Volume` struct + control-plane CRUD + content methods (or split control-plane into `volume/api.rs` like sandbox). 
- `crates/e2b-rs/src/lib.rs` — MODIFY: `pub use` the public volume types at the crate root; add a `## Volumes` quickstart section.
- `docs/parity-checklist.md`, `README.md` — MODIFY (Task 5).

---

### Task 1: Public volume types + the `VolumeApiClient` (Bearer transport)

**Files:** Create `crates/e2b-rs/src/volume/types.rs`, `crates/e2b-rs/src/volume/client.rs`; modify `crates/e2b-rs/src/volume/mod.rs`, `crates/e2b-rs/src/lib.rs`.

**Interfaces:**
- Produces (public, `types.rs`): `enum VolumeFileType { Unknown, File, Directory, Symlink }`; `struct VolumeInfo { volume_id: String, name: String }`; `struct VolumeAndToken { volume_id: String, name: String, token: String }`; `struct VolumeEntryStat { name, path: String, file_type: VolumeFileType, size: i64, mode: u32, uid: u32, gid: u32, atime/mtime/ctime: chrono::DateTime<Utc>, target: Option<String> }`; opt structs `#[derive(Default)]`: `VolumeMetadataOpts { uid: Option<u32>, gid: Option<u32>, mode: Option<u32> }`, `VolumeWriteOpts { uid, gid, mode: Option<u32>, force: Option<bool> }`, `VolumeReadOpts { stream_idle_timeout_ms: Option<u64> }`, `VolumeListOpts { depth: Option<u32> }`, `VolumeMakeDirOpts { uid, gid, mode: Option<u32>, force: Option<bool> }`. Plus `pub(crate) fn` mappers: `VolumeEntryStat::from_wire(crate::volume::schema::VolumeEntryStat) -> VolumeEntryStat` (map `type_` via the generated `VolumeEntryStatType` → `VolumeFileType`); `VolumeAndToken`/`VolumeInfo` from the generated `api::schema` types.
- Produces (`pub(crate)`, `client.rs`): `struct VolumeApiClient { http: reqwest::Client, base_url: String, file_timeout_ms: u64 }`; `VolumeApiClient::new(api_url: &str, token: &str, request_timeout_ms: u64, proxy: Option<&str>) -> Result<VolumeApiClient>` (default headers + `Authorization: Bearer {token}`); `async fn request_json<T: DeserializeOwned>(&self, method, path: &str, query: &[(&str, String)], body: Option<&impl Serialize>) -> Result<T>`; `async fn read_bytes(&self, path: &str, query: &[(&str,String)]) -> Result<Vec<u8>>`; `async fn read_stream(&self, path, query, idle_timeout_ms: u64) -> Result<impl Stream<Item=Result<bytes::Bytes>>>`; `async fn write_bytes(&self, path, query, body: Vec<u8>) -> Result<T>` (octet-stream). Status→Error mapping mirrors `ApiClient`/`EnvdApiClient` (decode the `volume::schema::Error{code,message}` body for messages; 404 → `Error::NotFound`).

- [ ] **Step 1: Write failing tests** (in `client.rs` + `types.rs` `#[cfg(test)]`):
  - `types.rs`: `maps_wire_entry_stat` — build a `crate::volume::schema::VolumeEntryStat` with `type_ = VolumeEntryStatType::Directory` and assert `VolumeEntryStat::from_wire(..).file_type == VolumeFileType::Directory`; map each enum variant.
  - `client.rs` (wiremock): `read_bytes_gets_file` — mock `GET /volumecontent/vol_1/file?path=/a.txt` (assert `Authorization: Bearer tkn` header + path query) returning octet-stream `b"hello"`; assert `read_bytes` returns `b"hello"`. `request_json_decodes_entry_stat` — mock `GET /volumecontent/vol_1/path?path=/a` returning an HONEST `VolumeEntryStat` JSON (lowercase `"type":"file"`, `volumeID`-style fields N/A here, camelCase N/A — fields are `atime/ctime/mtime/gid/mode/uid/name/path/size/type`); assert it decodes + maps. `error_body_maps` — mock 404 with `{"code":"not_found","message":"x"}` → `Err(Error::NotFound(_))`.
- [ ] **Step 2: Run to verify failure** — `cargo test -p e2b-rs volume::` → FAIL.
- [ ] **Step 3: Implement types** (`types.rs`) — public structs + docs; `from_wire` mappers (the `VolumeEntryStatType` → `VolumeFileType` match: `Unknown→Unknown, File→File, Directory→Directory, Symlink→Symlink`).
- [ ] **Step 4: Implement `VolumeApiClient`** (`client.rs`) — model on `crate::api::client::ApiClient` (read it). Build the `reqwest::Client` with default headers incl. `Authorization: Bearer {token}` (use `HeaderValue::from_str(&format!("Bearer {token}")).map_err(...)`, never unwrap). `request_json` builds `{base_url}{path}` + query, sends, maps status→Error (decode `volume::schema::Error`), decodes T. `read_bytes`/`read_stream` GET the `/file` path (reqwest `gzip` auto-decompresses; `read_stream` returns `resp.bytes_stream()` mapped to `Result<Bytes>`, no/long timeout = `file_timeout_ms`). `write_bytes` PUTs with `.header(CONTENT_TYPE, "application/octet-stream").body(bytes)`.
- [ ] **Step 5: Wire module + re-export** — `volume/mod.rs`: `pub(crate) mod client; pub mod types;` + `pub use types::{VolumeEntryStat, VolumeFileType, VolumeInfo, VolumeAndToken, VolumeMetadataOpts, VolumeWriteOpts, VolumeReadOpts, VolumeListOpts, VolumeMakeDirOpts};`. Re-export those at the crate root in `lib.rs` (mirror how `sandbox` types are re-exported). Run tests green.
- [ ] **Step 6: Verify & commit** — clippy `-D warnings`, `cargo doc` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/volume crates/e2b-rs/src/lib.rs
git commit -m "feat(volume): add public volume types + Bearer VolumeApiClient transport" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `Volume` struct + control-plane CRUD (create / list / get_info / connect / destroy)

**Files:** Create `crates/e2b-rs/src/volume/volume.rs` (+ optionally `volume/api.rs`); modify `crates/e2b-rs/src/volume/mod.rs`, `crates/e2b-rs/src/lib.rs`.

**Interfaces:**
- Produces: `pub struct Volume { volume_id: String, name: String, token: String, config: ConnectionConfig }` (+ getters `volume_id()`/`name()`/`token()` returning `&str`).
- `VolumeOpts` (`#[derive(Default)]`): `{ api_key: Option<String>, domain: Option<String>, api_url: Option<String>, request_timeout_ms: Option<u64>, proxy: Option<String> }` (the connection knobs; mirror JS `VolumeApiOpts` minus debug/logger/signal — keep it minimal but cover api_key/domain/api_url/timeout/proxy).
- Associated fns (mirror JS statics; control-plane via `ApiClient`):
  - `pub async fn create(name: &str, opts: VolumeOpts) -> Result<Volume>` — `POST /volumes` body `NewVolume{name: NewVolumeName::try_from(name.to_string()).map_err(|e| Error::InvalidArgument(e.to_string()))?}` → `VolumeAndToken` → build `Volume`.
  - `pub async fn list(opts: VolumeOpts) -> Result<Vec<VolumeInfo>>` — `GET /volumes` → `Vec<api::schema::Volume>` → map to `VolumeInfo`.
  - `pub async fn get_info(volume_id: &str, opts: VolumeOpts) -> Result<VolumeAndToken>` — `GET /volumes/{volume_id}` → `VolumeAndToken`.
  - `pub async fn connect(volume_id: &str, opts: VolumeOpts) -> Result<Volume>` — calls `get_info` then builds a `Volume` from the result + resolved config.
  - `pub async fn destroy(volume_id: &str, opts: VolumeOpts) -> Result<bool>` — `DELETE /volumes/{volume_id}` → `true`; 404 → `false` (mirror the JS "already gone → false" + the sandbox `kill` 404→false pattern using `request_unit` + matching `Err(Error::NotFound(_))`/`Err(Error::SandboxNotFound)` → check which 404 variant the control-plane returns and map to `false`).
- Build helper: `fn build_api_client(opts: &VolumeOpts) -> Result<(ApiClient, ConnectionConfig)>` resolving `ConnectionConfig::new(ConnectionConfigOpts{ api_key: opts.api_key.clone(), domain: opts.domain.clone(), api_url: opts.api_url.clone(), .. })`.

- [ ] **Step 1: Write failing tests** (wiremock, honest fixtures):
  - `create_posts_and_builds_volume` — mock `POST /volumes` (assert `X-API-KEY` header + body `{"name":"v"}`) returning `{"volumeID":"vol_1","name":"v","token":"tkn"}`; assert `Volume::create("v", opts_pointing_at_server).volume_id()=="vol_1"` and `token()=="tkn"`.
  - `list_maps_to_info` — mock `GET /volumes` → `[{"volumeID":"vol_1","name":"v"}]`; assert one `VolumeInfo`.
  - `destroy_404_is_false` — mock `DELETE /volumes/vol_x` → 404; assert `Ok(false)`. `destroy_204_is_true` → 204 → `Ok(true)`.
  - (Use a `VolumeOpts` whose `api_url` points at the wiremock server; set `api_key` so validation passes — mirror how `sandbox::api` tests inject the server URL.)
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per JS `volume/index.ts` (create ~113, list ~214, getInfo ~175, connect ~151, destroy ~239). Map generated → public types via Task-1 mappers. Wire `Volume` + `VolumeOpts` re-exports.
- [ ] **Step 4: Run tests + commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/volume crates/e2b-rs/src/lib.rs
git commit -m "feat(volume): add Volume struct + control-plane CRUD" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Volume content metadata ops (list / make_dir / get_info / update_metadata / remove / exists)

**Files:** Modify `crates/e2b-rs/src/volume/volume.rs`.

**Interfaces (instance methods on `Volume`; build a `VolumeApiClient` from `self.config.api_url` + `self.token`):**
- `pub async fn list(&self, path: &str, opts: VolumeListOpts) -> Result<Vec<VolumeEntryStat>>` — `GET /volumecontent/{id}/dir` query `path` + `depth?` → `VolumeDirectoryListing` → map each.
- `pub async fn make_dir(&self, path: &str, opts: VolumeMakeDirOpts) -> Result<VolumeEntryStat>` — `POST /volumecontent/{id}/dir` query `path,uid?,gid?,mode?,force?` → `VolumeEntryStat`.
- `pub async fn get_info(&self, path: &str) -> Result<VolumeEntryStat>` — `GET /volumecontent/{id}/path` query `path` → `VolumeEntryStat`. (NOTE name clash with the static `get_info(volume_id,...)` — these are different `impl` items: one assoc fn taking `volume_id`, one `&self` method taking a content `path`. Both can coexist; if Rust disallows the same name for assoc-fn vs method, rename the content one to keep both — confirm and, if needed, name the instance one `stat`/`get_path_info` and note the JS divergence.)
- `pub async fn update_metadata(&self, path: &str, metadata: VolumeMetadataOpts) -> Result<VolumeEntryStat>` — `PATCH /volumecontent/{id}/path` query `path`, body `{uid?,gid?,mode?}` → `VolumeEntryStat`.
- `pub async fn remove(&self, path: &str) -> Result<()>` — `DELETE /volumecontent/{id}/path` query `path` → 204.
- `pub async fn exists(&self, path: &str) -> Result<bool>` — calls the content get_info; `Ok(true)` on success, `Ok(false)` on `Err(Error::NotFound(_))` (mirror JS exists → getInfo 404→false).

- [ ] **Step 1: Write failing tests** (wiremock, honest lowercase-`type` fixtures, assert Bearer header): `list_dir`, `make_dir`, content `get_info`/`stat`, `update_metadata` (assert PATCH body), `remove` (204), `exists` true + `exists` false (404). Build the `Volume` directly in tests via a `pub(crate)` test constructor that injects `{volume_id, name, token, config}` with `config.api_url` = wiremock server.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per JS `volume/index.ts` (list ~276, makeDir ~317, getInfo ~366, updateMetadata ~432, remove ~722, exists ~411). Query params: numbers → `n.to_string()`, `force`/bools → `"true"`/`"false"`. Resolve the name-clash decision from the interfaces note.
- [ ] **Step 4: Run tests + commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/volume/volume.rs
git commit -m "feat(volume): add volume-content metadata ops" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Volume content file I/O (read_file text/bytes/stream + write_file)

**Files:** Modify `crates/e2b-rs/src/volume/volume.rs`.

**Interfaces (on `Volume`):**
- `pub async fn read_file(&self, path: &str) -> Result<String>` — `GET /volumecontent/{id}/file` query `path`, decode bytes as UTF-8 (`String::from_utf8` → `Error::Internal` on invalid; or `from_utf8_lossy` — match JS text decode, which is UTF-8 strict via `.text()`; prefer strict + map error).
- `pub async fn read_file_bytes(&self, path: &str) -> Result<Vec<u8>>` — raw bytes.
- `pub async fn read_file_stream(&self, path: &str, opts: VolumeReadOpts) -> Result<impl Stream<Item = Result<bytes::Bytes>>>` — streaming, idle timeout default `FILE_TIMEOUT_MS` (1h) overridable via `opts.stream_idle_timeout_ms`. Mirror Plan-3b `read_stream`.
- `pub async fn write_file(&self, path: &str, data: Vec<u8>, opts: VolumeWriteOpts) -> Result<VolumeEntryStat>` — `PUT /volumecontent/{id}/file` query `path,uid?,gid?,mode?,force?`, body octet-stream → `VolumeEntryStat`. (Provide a `&str`/`&[u8]` ergonomic entry if it matches the other write APIs in the crate; otherwise `Vec<u8>`/`Into<Vec<u8>>` is fine — match how `filesystem::write` accepts data.)

- [ ] **Step 1: Write failing tests** (wiremock): `read_file_text` (octet-stream body `b"hello"` → `"hello"`), `read_file_bytes`, `write_file` (assert `PUT`, octet-stream content-type, query `path` + a metadata param, body bytes; response a `VolumeEntryStat` JSON → mapped). A `read_file_stream` test collecting chunks → `b"hello"`.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per JS `volume/index.ts` (readFile ~486 text / ~501 bytes / ~531 stream, writeFile ~653). Use the Task-1 `VolumeApiClient` `read_bytes`/`read_stream`/`write_bytes`.
- [ ] **Step 4: Run tests + commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/volume/volume.rs
git commit -m "feat(volume): add volume-content file read/write/stream" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Parity checklist, quickstart & full gate

**Files:** Modify `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc), `README.md`.

- [ ] **Step 1: Crate quickstart** — add a `## Volumes` `no_run` doctest to `lib.rs` `//!` docs (mirror the existing sections' async-wrapper style): `let volume = Volume::create("my-data", Default::default()).await?;` then `volume.write_file("/hello.txt", b"hi".to_vec(), Default::default()).await?;` then `let text = volume.read_file("/hello.txt").await?;`. Verify signatures so it compiles under `no_run`.
- [ ] **Step 2: Parity checklist** — add a `## Volume (Plan 4b)` section: control-plane (create/list/getInfo/connect/destroy) + content (list/makeDir/getInfo/updateMetadata/readFile[text/bytes/stream]/writeFile/remove/exists). Note: no pagination; Bearer-token content transport vs X-API-KEY control-plane; metadata via query params.
- [ ] **Step 3: README** — short volume snippet. Stage only if changed.
- [ ] **Step 4: Full release gate** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features` (0 failures, report counts); `cargo test --doc -p e2b-rs`; `cargo doc --no-deps -p e2b-rs`; `cargo xtask codegen && git status --porcelain` → empty.
- [ ] **Step 5: Commit**
```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(volume): document volume quickstart and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 4b is complete when:
- `Volume::{create,list,get_info,connect,destroy}` (control-plane via `ApiClient`) and instance `{list,make_dir,get_info/stat,update_metadata,remove,exists,read_file,read_file_bytes,read_file_stream,write_file}` (content via `VolumeApiClient` Bearer) all work.
- No generated type is exposed; `Volume`/`VolumeInfo`/`VolumeAndToken`/`VolumeEntryStat`/`VolumeFileType` + opt structs re-exported at the crate root.
- All fixtures honest (lowercase `type`, `volumeID` rename); 404→false for destroy/exists.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` pass; codegen idempotent.
- `docs/parity-checklist.md` reflects volume.

**Carry-forwards (documented):** JS `VolumeApiOpts` debug/logger/signal not exposed (minimal opts); per-call `requestTimeoutMs`/proxy passthrough scope; whether `Volume` rebuilds `VolumeApiClient` per call (JS) vs once (decided in Task 1).

**Next:** Plan 5 — Template build pipeline & polish (the final milestone).
