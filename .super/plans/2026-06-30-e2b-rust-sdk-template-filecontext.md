# Template File-Context (Plan 5b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Build the file-context machinery of the `template` subsystem: deterministic file discovery (glob + `.dockerignore`), the files-hash (cache key), gzipped-tar context creation, the presigned-URL upload flow, and a hand-ported minimal Dockerfile parser. These are the `pub(crate)` building blocks the Plan-5c build pipeline orchestrates.

**Architecture:** All `pub(crate)` internals under `crate::template` (no new public API surface except possibly `parse_dockerfile` if 5c needs it public — keep `pub(crate)` unless required). Two HTTP touchpoints reuse existing clients: `get_file_upload_link` does `GET /templates/{templateID}/files/{hash}` via the existing `ApiClient` (X-API-KEY); `upload_file` does a `PUT` to the returned S3 presigned URL via a plain `reqwest` request with an explicit `Content-Length` and a 1-hour timeout (S3 rejects chunked transfer-encoding). The files-hash is computed client-side and used purely as a CACHE KEY (the server stores the uploaded context under that hash and `GET /files/{hash}` reports `present`), so the hash MUST be deterministic and stable across runs; matching the JS hash byte-for-byte is a stretch goal (enables cross-SDK cache sharing) but NOT a correctness requirement.

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0). NEW deps (workspace root `Cargo.toml`, mirror the existing dep style): `sha2` (hashing), `tar` (archive), `flate2` (gzip — or reuse the already-present `async-compression`; prefer `flate2` for the synchronous spool-to-temp-file path), `glob` (path expansion) + `globset` (minimatch-style ignore matching), `tempfile` (spool the archive to a temp file), `walkdir` (directory recursion). `reqwest` (already present) for the S3 PUT.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` only in `#[cfg(test)]`. `missing_docs` applies to PUBLIC items; `pub(crate)` items don't need docs but document them anyway for clarity. Prefer `try_from().unwrap_or()` over `as`. `[crate::Type]` cross-module links.
- Do NOT expose generated types. New deps go in the WORKSPACE root `Cargo.toml` `[workspace.dependencies]` + referenced in `crates/e2b-rs/Cargo.toml` (follow how `reqwest`/`bytes`/`async-compression` are declared).
- The files-hash MUST be deterministic (same tree → same hash, verified by a same-input-twice test AND an explicit byte-sequence test). uid/gid/mtime are EXCLUDED from the hash (only mode + size + path + content), matching JS.
- Every task: `cargo fmt --all` before commit; `cargo doc --no-deps -p e2b-rs` + `cargo xtask codegen` idempotency in the final gate. Commit trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference: JS `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/template/{utils.ts,dockerfileParser.ts,buildApi.ts,consts.ts}`.

### Pre-verified facts (confirmed at `main` = 62663e7)
- `crate::api::client::ApiClient` has `request<T>(Method, path, query, body)`; the generated `api::schema::TemplateBuildFileUpload { present: bool, url: Option<String> }` is the `GET /templates/{templateID}/files/{hash}` response (confirm exact field names in `api/schema.rs`). 404/non-2xx → existing `Error::from_status`.
- `FILE_UPLOAD_TIMEOUT_MS = 3_600_000` (consts.ts:43).
- **`calculate_files_hash` algorithm (port EXACTLY)** — sha256, feed in THIS order: (1) the string `"COPY {src} {dest}"`; (2) `get_all_files_in_path(src, context, ignore)` → files sorted by full path (error if empty); (3) for each file: the relative-POSIX path string (forward slashes), then `mode.to_string()` (decimal of the raw unix st_mode), then `size.to_string()` (decimal), then — if a symlink that is NOT being followed: the readlink target string; else if a regular file: the raw file content bytes. Return the hex digest. (uid/gid/mtime excluded.)
- **`get_all_files_in_path`**: glob-expand `src` within `context_path` with the ignore patterns; for each matched DIRECTORY, additionally expand `dir/**/*`; dedupe; SORT by full path string. Deterministic order is essential.
- **`read_dockerignore(context)`**: read `{context}/.dockerignore` if it exists; split lines; drop empty lines and `#`-comment lines; return the patterns.
- **`validate_relative_path(src)`**: reject absolute paths and paths that escape the context (`..` or starting `../` after normalization) → `Error::InvalidArgument`.
- **`tar_file_stream`**: create a gzipped tar of the matched files rooted at `context_path`, SPOOLED to a temp file (`context.tar.gz`); return `(temp_file_path_or_handle, size_bytes)`. The temp file is cleaned up after upload.
- **`upload_file`**: `PUT {presigned_url}` with body = the tar.gz file stream, header `Content-Length: {size}` (NO chunked encoding — S3 returns 501), timeout = `FILE_UPLOAD_TIMEOUT_MS` (override via opts). Success = HTTP 2xx.
- **Dockerfile parse** (dockerfileParser.ts): first `FROM` = base image (multi-stage → error; no FROM → error). Dispatch keywords: `RUN`→run step; `COPY`/`ADD`→copy step (with `--chown`/`--chmod` flags → user/mode); `WORKDIR`→setWorkdir; `USER`→setUser; `ENV`/`ARG`→setEnvs (key=value, multi-line); `EXPOSE`/`VOLUME`→ignored; `CMD`/`ENTRYPOINT`→startCmd (+ a default `waitForTimeout`). After parsing, if USER/WORKDIR not explicitly set, apply E2B defaults (USER=`user`, WORKDIR=`/home/user`).

---

## File Structure

- `crates/e2b-rs/src/template/files.rs` — CREATE: `validate_relative_path`, `read_dockerignore`, `get_all_files_in_path`, `calculate_files_hash`.
- `crates/e2b-rs/src/template/archive.rs` — CREATE: `tar_file_stream` (gzip tar → temp file + size).
- `crates/e2b-rs/src/template/upload.rs` — CREATE: `get_file_upload_link`, `upload_file`.
- `crates/e2b-rs/src/template/dockerfile.rs` — CREATE: the hand-ported parser → `DockerfileParseResult { base_image: String, instructions: Vec<Instruction>, start_cmd: Option<String>, ready_cmd: Option<ReadyCmd>, user: Option<String>, workdir: Option<String>, envs: BTreeMap<String,String> }` (shape per 5c's needs — keep `pub(crate)`).
- `crates/e2b-rs/src/template/mod.rs` — MODIFY: wire the new `pub(crate)` submodules.
- WORKSPACE `Cargo.toml` + `crates/e2b-rs/Cargo.toml` — MODIFY: add the new deps.
- `docs/parity-checklist.md` — MODIFY (Task 4).

---

### Task 1: Add deps + file discovery + the files-hash (the deterministic core)

**Files:** workspace `Cargo.toml`, `crates/e2b-rs/Cargo.toml`; create `template/files.rs`; modify `template/mod.rs`.

**Interfaces (`pub(crate)`):** `validate_relative_path(src: &str) -> Result<()>`; `read_dockerignore(context: &std::path::Path) -> Vec<String>`; `get_all_files_in_path(src: &str, context: &std::path::Path, ignore: &[String]) -> Result<Vec<std::path::PathBuf>>` (sorted, deduped); `calculate_files_hash(src: &str, dest: &str, context: &std::path::Path, ignore: &[String], resolve_symlinks: bool) -> Result<String>` (hex sha256).

- [ ] **Step 1: Add deps** — add `sha2`, `glob`, `globset`, `walkdir`, `tempfile` to `[workspace.dependencies]` (workspace root `Cargo.toml`) and reference them in `crates/e2b-rs/Cargo.toml` (mirror the `bytes`/`async-compression` declaration style). Run `cargo build -p e2b-rs` to confirm they resolve. (`tar`/`flate2` are added in Task 2; add them now too if convenient.)
- [ ] **Step 2: Write failing tests** (`files.rs` `#[cfg(test)]`, using `tempfile::tempdir`):
  - `validate_relative_path_rejects_absolute_and_escape` — `/abs` and `../escape` → `Err(InvalidArgument)`; `foo/bar` → `Ok`.
  - `read_dockerignore_filters_comments_and_blanks` — write a `.dockerignore` with comments/blanks → only real patterns returned.
  - `get_all_files_sorted_and_deduped` — build a tiny tree (`a.txt`, `sub/b.txt`), assert the returned paths are sorted and stable; with an ignore pattern (`sub/**` or `*.log`) the ignored files are excluded.
  - `files_hash_is_deterministic` — same tree hashed twice → equal; changing a file's content → different hash.
  - `files_hash_byte_sequence` — build a ONE-file tree with known content, compute `calculate_files_hash`, and INDEPENDENTLY compute the expected digest in the test by feeding the exact byte sequence (`"COPY {src} {dest}"` + relpath + `mode.to_string()` + `size.to_string()` + content) into `sha2::Sha256`; assert equality. (This pins the algorithm to the spec.)
- [ ] **Step 3: Run to verify failure** — FAIL.
- [ ] **Step 4: Implement** — `get_all_files_in_path` via `glob`/`walkdir` + `globset` for the ignore filter (build a `GlobSet` from the patterns; a path is ignored if it matches any). SORT the final `Vec<PathBuf>` by path. `calculate_files_hash` ports the algorithm EXACTLY (unix `mode` via `std::os::unix::fs::MetadataExt::mode()` — gate the unix-specific path; on non-unix, fall back to a stable mode value and note it). Relative-POSIX path = the path relative to `context` with `/` separators. Symlink handling: `symlink_metadata` to detect; if not following, hash the lstat mode/size + the readlink target.
- [ ] **Step 5: Run tests green; verify & commit** — clippy `-D warnings`, `cargo doc` clean.
```bash
cargo fmt --all
git add Cargo.toml crates/e2b-rs/Cargo.toml crates/e2b-rs/src/template
git commit -m "feat(template): add file discovery + deterministic files-hash" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: gzip-tar context archive + presigned-URL upload

**Files:** create `template/archive.rs`, `template/upload.rs`; modify `template/mod.rs`, Cargo files (add `tar`, `flate2` if not added in Task 1).

**Interfaces (`pub(crate)`):**
- `archive.rs`: `tar_file_stream(src: &str, context: &std::path::Path, ignore: &[String], resolve_symlinks: bool) -> Result<(tempfile::NamedTempFile, u64)>` — write a gzipped tar of the matched files (paths relative to `context`) to a temp file; return the handle + byte size. (Use `tar::Builder` over a `flate2::write::GzEncoder` over the temp file.)
- `upload.rs`: `get_file_upload_link(api: &ApiClient, template_id: &str, files_hash: &str) -> Result<FileUploadLink>` where `FileUploadLink { present: bool, url: Option<String> }` (hand-written wrapper over generated `TemplateBuildFileUpload`) — `GET /templates/{template_id}/files/{files_hash}`. `upload_file(http: &reqwest::Client, url: &str, archive: tempfile::NamedTempFile, size: u64, timeout_ms: u64) -> Result<()>` — `PUT` with `Content-Length: size`, body = the file stream, `.timeout(Duration::from_millis(timeout_ms))`; 2xx → Ok, else Error.

- [ ] **Step 1: Write failing tests:**
  - `archive.rs`: `tar_roundtrips_files` — tar a tiny tree to a temp file, then read it back with `tar::Archive` (over `flate2::read::GzDecoder`) and assert the entries + contents match; assert size > 0.
  - `upload.rs`: `get_file_upload_link_present` / `_absent` — wiremock `GET /templates/tpl_1/files/<hash>` returning `{"present":true,"url":"https://s3/..."}` / `{"present":false}`; assert the wrapper. `upload_file_puts_with_content_length` — wiremock (or a local mock) `PUT` asserting the `Content-Length` header + body bytes, 200 → Ok; a 501/4xx → Err. (For `upload_file`, point it at a wiremock server URL as the "presigned URL".)
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per the JS (`tarFileStream` utils.ts:374; `getFileUploadLink`/`uploadFile` buildApi.ts). The PUT must set `Content-Length` explicitly and must NOT use chunked encoding (load the temp file and send it as a sized body — `reqwest::Body::from(std::fs::File)` sets Content-Length from the file length, or read into a `Vec<u8>`; prefer the file/sized-body path to avoid buffering huge archives, but a `Vec<u8>` body is acceptable for the first cut — document the choice).
- [ ] **Step 4: Run tests + commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/template Cargo.toml crates/e2b-rs/Cargo.toml
git commit -m "feat(template): add gzip-tar archive + presigned-URL upload" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Hand-ported minimal Dockerfile parser

**Files:** create `template/dockerfile.rs`; modify `template/mod.rs`.

**Interfaces (`pub(crate)`):** `parse_dockerfile(content: &str) -> Result<DockerfileParseResult>` where `DockerfileParseResult` carries the base image + the accumulated instructions/start/ready/user/workdir/envs (shape to match what Plan 5c consumes — keep it `pub(crate)` and minimal).

- [ ] **Step 1: Write failing tests:**
  - `parses_from_run_copy_workdir_user_env` — a Dockerfile with `FROM node:20`, `RUN npm i`, `COPY . /app`, `WORKDIR /app`, `USER me`, `ENV K=V` → assert base image `node:20`, the run/copy/workdir/user/env instructions captured in order.
  - `rejects_multistage` — two `FROM`s → `Err`.
  - `rejects_missing_from` — no `FROM` → `Err`.
  - `copy_with_chown_chmod_flags` — `COPY --chown=me:me --chmod=755 a b` → copy step carries user/mode.
  - `cmd_becomes_start_cmd` — `CMD ["npm","start"]` (and the `RUN`/exec vs shell forms) → start_cmd set.
  - `ignores_expose_volume` — `EXPOSE 8080` / `VOLUME /data` produce no instruction.
  - `applies_e2b_defaults` — a Dockerfile that sets neither USER nor WORKDIR → result has USER=`user`, WORKDIR=`/home/user`.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** a minimal line-oriented parser (port dockerfileParser.ts's keyword dispatch — you do NOT need a full AST library): handle line continuations (`\` at EOL), comments (`#`), the keyword switch (FROM/RUN/COPY/ADD/WORKDIR/USER/ENV/ARG/EXPOSE/VOLUME/CMD/ENTRYPOINT), `COPY --chown=/--chmod=` flag parsing, ENV `key=value` and `key value` and multi-pair forms, and exec-form (`["a","b"]`) vs shell-form for RUN/CMD/ENTRYPOINT. Apply the E2B USER/WORKDIR defaults at the end.
- [ ] **Step 4: Run tests + commit** — clippy + doc clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/template/dockerfile.rs crates/e2b-rs/src/template/mod.rs
git commit -m "feat(template): add minimal Dockerfile parser" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Parity + full gate

**Files:** modify `docs/parity-checklist.md`.

- [ ] **Step 1: Parity** — under `## Template (Plan 5)`, add a `### 5b — File context` subsection: files-hash (deterministic cache key; uid/gid/mtime excluded), glob + `.dockerignore`, gzip-tar archive, presigned-URL upload (Content-Length, 1h timeout), and the minimal Dockerfile parser. Note the hash is intra-SDK-deterministic (cross-SDK byte-parity is a stretch goal).
- [ ] **Step 2: Full gate** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features` (report counts); `cargo test --doc -p e2b-rs`; `cargo doc --no-deps -p e2b-rs`; `cargo xtask codegen && git status --porcelain` → empty. Also confirm the new deps didn't break MSRV by noting their declared versions are 1.95-compatible.
- [ ] **Step 3: Commit**
```bash
cargo fmt --all
git add docs/parity-checklist.md
git commit -m "docs(template): document 5b file-context parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 5b is complete when:
- `crate::template` (all `pub(crate)`) provides: `validate_relative_path`, `read_dockerignore`, `get_all_files_in_path`, `calculate_files_hash` (deterministic, byte-sequence-pinned); `tar_file_stream`; `get_file_upload_link`/`upload_file`; `parse_dockerfile`.
- The files-hash is deterministic + excludes uid/gid/mtime; the upload sends an explicit `Content-Length` with a 1-hour timeout; the parser rejects multi-stage / missing-FROM and applies E2B defaults.
- New deps declared at the workspace root; `cargo fmt --check`, clippy `-D warnings`, `cargo test`, `cargo test --doc`, `cargo doc --no-deps` pass; codegen idempotent.
- `docs/parity-checklist.md` has the 5b subsection.

**Carry-forwards (documented):** cross-SDK hash byte-parity vs JS (stretch); non-unix `mode` fallback; streaming-vs-buffered upload body; `.dockerignore` minimatch-vs-gitignore edge cases.

**Next:** Plan 5c — the build pipeline (request → upload orchestration → trigger → poll status with log draining over a `tokio::sync::mpsc` channel) + tag operations. HIGHEST-risk sub-plan; use opus for its whole-branch review.
