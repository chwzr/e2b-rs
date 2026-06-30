# Task 5 Report — docs(volume): quickstart + parity + full gate (Plan 4b)

## Status: DONE

## Commit

- SHA: `13e2fe4`
- Message: `docs(volume): document volume quickstart and parity`
- Branch: `feat/volume`

---

## Step 1: Crate quickstart doctest added

Inserted `## Volumes` section after `## Git`, before `## Resolving configuration` in `crates/e2b-rs/src/lib.rs`:

```rust
//! ## Volumes
//!
//! ```no_run
//! # async fn run() -> e2b_rs::Result<()> {
//! use e2b_rs::Volume;
//! let volume = Volume::create("my-data", Default::default()).await?;
//! volume.write_file("/hello.txt", b"hi".to_vec(), Default::default()).await?;
//! let text = volume.read_file("/hello.txt").await?;
//! println!("{text}");
//! # Ok(())
//! # }
//! ```
```

Signatures verified from `crates/e2b-rs/src/volume/control.rs` before writing:
- `Volume::create(name: &str, opts: VolumeOpts) -> Result<Volume>` ✅
- `Volume::write_file(&self, path: &str, data: impl Into<Vec<u8>>, opts: VolumeWriteOpts) -> Result<VolumeEntryStat>` ✅
- `Volume::read_file(&self, path: &str) -> Result<String>` ✅

---

## Step 2: Parity checklist rows added

Replaced `## Volume & Template (Plan 4b / Plan 5)` placeholder with:
- `## Volume (Plan 4b)` — full table with control-plane (5 rows) + content (11 rows) + notes
- `## Template (Plan 5)` — placeholder for next milestone

### Control-plane rows (all ✅)
`create` / `list` / `get_info` / `connect` / `destroy` (note: 404→false)

### Content rows
`list_dir` / `make_dir` / `stat` / `update_metadata` / `read_file` (text) / `read_file_bytes` / `read_file_stream` / `write_file` / `remove` / `exists` — all ✅
`readFile(blob)` — N/A ⬜ (browser-only `Blob` type)

### Notes documented
- Transport split: X-API-KEY (control-plane) vs `Authorization: Bearer` (content)
- No pagination on list/list_dir
- Metadata via query params for make_dir/write_file; JSON body for update_metadata
- 1-hour file timeout for read_file*/write_file
- Rust rename rationale: `list_dir` (vs JS `list`) and `stat` (vs JS `getInfo`) to avoid assoc-fn/method clash
- Carry-forwards: debug/logger/signal not exposed; per-call requestTimeoutMs/proxy scope

---

## Step 3: README changed

Yes — added Volume usage snippet (create + write + read) after the Git section, consistent with existing fencing/style.

---

## Step 4: Full release gate results

| Step | Command | Result |
|---|---|---|
| 1 | `cargo fmt --all --check` | ✅ PASS — no output (clean) |
| 2 | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ PASS — Finished in 2.24s, 0 warnings |
| 3 | `cargo test --workspace --all-features` | ✅ PASS — 178 passed, 0 failed, 0 ignored |
| 4 | `cargo test --doc -p e2b-rs` | ✅ PASS — 9 passed, 0 failed (Volumes doctest at lib.rs line 92) |
| 5 | `cargo doc --no-deps -p e2b-rs` | ✅ PASS — Generated target/doc/e2b_rs/index.html |
| 6 | `cargo xtask codegen && git status --porcelain` | ✅ PASS — only `.super-workspace/sdd/task-3-report.md` dirty (pre-existing, not a generated file) |

Codegen rewrote 4 generated modules (envd proto, volume content types, control-plane API types, envd REST types) identically — all 4 idempotent.

---

## Files changed

- `crates/e2b-rs/src/lib.rs` — added `## Volumes` no_run doctest section (13 lines)
- `docs/parity-checklist.md` — replaced placeholder with `## Volume (Plan 4b)` table (16 rows + notes) and `## Template (Plan 5)` placeholder
- `README.md` — added Volume usage snippet (9 lines)

## Self-review

- Doctest is `no_run`, wrapped in `# async fn run() -> e2b_rs::Result<()>` exactly matching all other lib.rs sections.
- `Volume` is re-exported at crate root (lib.rs line 148), so `use e2b_rs::Volume` resolves in the doctest.
- Parity table uses the same pipe-delimited format with Status column as all other sections.
- No non-doc source files were modified.
- `deny(missing_docs, rustdoc::broken_intra_doc_links)` satisfied — `cargo doc` produced no warnings.

## Concerns

None. All 6 gate steps green. Plan 4b complete.

---

## Plan 4b completion criteria — all met

- `Volume::{create,list,get_info,connect,destroy}` (control-plane via `ApiClient`) ✅
- Instance `{list_dir,make_dir,stat,update_metadata,remove,exists,read_file,read_file_bytes,read_file_stream,write_file}` (content via `VolumeApiClient` Bearer) ✅
- No generated types exposed at crate root ✅
- All fixtures honest (lowercase `type`, `volumeID` rename) ✅
- 404→false for destroy/exists ✅
- `cargo fmt --check`, `cargo clippy`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` pass ✅
- Codegen idempotent ✅
- `docs/parity-checklist.md` reflects volume ✅

---

## Plan 4b final-review fixes (branch `feat/volume`, applied after task-5 commit)

### Fix 1 — Secret-bearing types redact credentials in `Debug`

Three types derived `Debug` while holding secrets. Removed `Debug` from `#[derive]` and added manual `impl std::fmt::Debug` with `"<redacted>"` for secret fields:

- `crates/e2b-rs/src/volume/control.rs` — `struct Volume`: prints `volume_id`, `name`, `api_url`, `request_timeout_ms` normally; `token: "<redacted>"`, `proxy: "<redacted>"`/`"None"`.
- `crates/e2b-rs/src/volume/control.rs` — `struct VolumeOpts`: prints `domain`, `api_url`, `request_timeout_ms` normally; `api_key` and `proxy` as `"<redacted>"`/`"None"`.
- `crates/e2b-rs/src/volume/types.rs` — `struct VolumeAndToken`: prints `volume_id`, `name` normally; `token: "<redacted>"`.

Tests added:
- `volume::control::tests::volume_debug_redacts_token_and_proxy` — constructs `Volume` with `token = "supersecret-token"` and `proxy = "http://user:pass@..."`, asserts neither leaks in `format!("{:?}", vol)`.
- `volume::control::tests::volume_opts_debug_redacts_api_key_and_proxy` — constructs `VolumeOpts` with `api_key = "e2b_secret"`, asserts not in debug output.
- `volume::types::tests::volume_and_token_debug_redacts_token` — constructs `VolumeAndToken` with `token = "supersecret-token"`, asserts not in debug output.

### Fix 2 — Control-plane 404 message parity for `get_info` / `connect`

Added `.map_err(|e| match e { Error::NotFound(_) => Error::NotFound(format!("Volume {volume_id} not found")), other => other })` to both `Volume::get_info` and `Volume::connect` after the GET request, mirroring JS `getInfo` `NotFoundError("Volume ${volumeId} not found")`.

Tests added:
- `volume::control::tests::get_info_404_includes_volume_id` — mock 404 on `/volumes/vol_missing`; asserts `Error::NotFound` with `"vol_missing"` in message.
- `volume::control::tests::connect_404_includes_volume_id` — mock 404 on `/volumes/vol_gone`; asserts `Error::NotFound` with `"vol_gone"` in message.

### Fix 3 — `read_file` doc comment corrected

Replaced inaccurate claim "matches JS strict `.text()` decode behaviour" with accurate statement: reads as strict UTF-8, returning an error for invalid bytes — a documented divergence from the JS SDK's lossy `.text()` (U+FFFD replacement). Added intra-doc link `[Self::read_file_bytes]`.

### Fix 4 — `write_file_puts_octet_stream` asserts request body

Added `.and(body_bytes(b"hello".to_vec()))` to the `Mock::given(...)` chain, so the test now verifies the PUT body, not just method/content-type/query params. Added `body_bytes` to the wiremock import.

### Full gate results

| Command | Result |
|---|---|
| `cargo test -p e2b-rs volume::` | ✅ 31 passed, 0 failed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | ✅ 0 warnings |
| `cargo test --workspace --all-features` | ✅ 183 passed, 0 failed + 9 doc-tests |
| `cargo test --doc -p e2b-rs` | ✅ 9 passed, 0 failed |
| `cargo fmt --all` | ✅ No changes |
| `cargo doc --no-deps -p e2b-rs` | ✅ No warnings, generated index.html |
