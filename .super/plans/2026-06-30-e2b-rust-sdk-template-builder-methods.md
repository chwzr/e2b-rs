# Template Builder Methods (Plan 5d) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Complete the `Template` builder surface — the convenience methods that accumulate build `Instruction`s (file ops, commands, env, package installers, git clone) and the base-image `from*` variants. This is the FINAL sub-plan of the project. All methods are pure string-building + instruction accumulation — NO transport, NO concurrency.

**Architecture:** Each builder method consumes `self` and returns `Template` (fluent chaining), pushing to `self.instructions` (a `Vec<Instruction>`) one of `InstructionType::{Copy, Run, Workdir, User, Env}`, or setting `base_image`/`registry_config`. Most install/git/file-op methods reduce to a `RUN` instruction whose command string is built per the JS. The `force` flag on each pushed instruction is `self.force` (the template-level cache-bypass set by `skip_cache`; the JS per-layer `forceNextLayer` is simplified to template-level — a documented 5c carry-forward). `copy` produces a `COPY` instruction whose `args = [src, dest, user_or_empty, mode_or_empty]` — the EXACT shape Plan-5c's `instructions_with_hashes`/`upload_build_context` already consume (`src=args[0]`, `dest=args[1]`).

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), `crate::utils::shell_quote`, the Plan-5b `validate_relative_path`, the existing `Template`/`Instruction`/`InstructionType`/`RegistryConfig`.

## Global Constraints

- Package `e2b-rs`/lib `e2b_rs`; crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` only in `#[cfg(test)]`. Prefer `try_from().unwrap_or()` over `as`. `[crate::Type]` cross-module links.
- **Deferred (user decision):** NO `addMcpServer`, NO `betaDevContainerPrebuild`/`betaSetDevContainerStart`. Document them as deferred in the parity checklist.
- Every public method documented (with a `///` example where it clarifies). Each task: `cargo fmt --all` as the LAST step before `git add`; `cargo doc --no-deps` + `cargo xtask codegen` idempotency in the final gate. Commit trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference: JS `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/template/index.ts` (the `TemplateBuilder` methods).
- **Parity is verified against BOTH the JS and Python SDKs** (CLAUDE.md mandate; the Python SDK is at `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/python-sdk` if present — reviewers should spot-check command strings against it too).

### Pre-verified facts (confirmed at `main` = 918aefe)
- `Template { base_image, base_template, registry_config, instructions: Vec<Instruction>, start_cmd, ready_cmd, force, cpu_count, memory_mb }`. `from_image(self, &str) -> Template` exists (sets base_image, clears base_template). `RegistryConfig { Aws{access_key_id,secret_access_key,region}, Gcp{service_account_json}, General{username,password} }`. `Instruction { instruction_type, args: Vec<String>, force, force_upload: Option<bool>, files_hash: Option<String>, resolve_symlinks: bool }`. `InstructionType { Copy, Env, Run, Workdir, User }`.
- `crate::utils::shell_quote(&str) -> String`; `crate::template::files::validate_relative_path(&str) -> Result<()>`.
- **JS command strings (port verbatim):**
  - `from_debian_image(variant="stable")` → `from_image("debian:{variant}")`; `from_ubuntu_image("latest")` → `ubuntu:{v}`; `from_python_image("3")` → `python:{v}`; `from_node_image("lts")` → `node:{v}`; `from_bun_image("latest")` → `oven/bun:{v}`.
  - `from_aws_registry(image, {access_key_id, secret_access_key, region})` → set `base_image = image` + `registry_config = RegistryConfig::Aws{...}`; `from_gcp_registry(image, {service_account_json})` → base_image + `RegistryConfig::Gcp{...}`.
  - `copy(src, dest, opts)` — for each src (accept one or many): `validate_relative_path(src)?`; push `COPY { args: [src, dest, opts.user.unwrap_or(""), opts.mode.map(pad_octal).unwrap_or("")], force: opts.force_upload.unwrap_or(false) || self.force, force_upload: opts.force_upload, resolve_symlinks: opts.resolve_symlinks.unwrap_or(false) }`. `pad_octal(mode)` = the octal mode as a zero-padded string (port `padOctal`).
  - `remove(paths, {force?, recursive?, user?})` → `run_cmd("rm" + [" -r"]? + [" -f"]? + shell_quoted paths, user)`.
  - `rename(src, dest, {force?, user?})` → `run_cmd("mv" + [" -f"]? + shell_quote(src) + shell_quote(dest), user)`.
  - `make_dir(paths, {user?, mode?})` → `run_cmd("mkdir -p ...", user)` (verify exact flags in JS).
  - `make_symlink(src, dest, {user?})` → `run_cmd("ln -s src dest", user)`.
  - `run_cmd(cmd | cmds, {user?})` → `args = [cmds.join(" && ")]` + (push `user` as `args[1]` if set); push `RUN { args, force: self.force }`.
  - `set_workdir(path)` → push `WORKDIR { args: [path] }`. `set_user(user)` → push `USER { args: [user] }`.
  - `set_envs(map)` → skip if empty; `args = entries.flat_map([k, v])`; push `ENV { args }`.
  - `pip_install(packages?, {g=true})` → `["pip","install"]` + (`--user` if `!g`) + (`packages` or `["."]`) → `run_cmd(joined, None)`.
  - `npm_install(packages?, {g?, dev?})` → `["npm","install"]` + (`-g` if g) + (`--save-dev` if dev) + packages → run_cmd.
  - `bun_install(packages?, {g?, dev?})` → `["bun","install"]` + flags + packages (verify exact flags).
  - `apt_install(packages, {no_install_recommends?, fix_missing?})` → `run_cmd(["apt-get update", "DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes apt-get install -y {--no-install-recommends }?{--fix-missing }?{packages}"], user="root")`.
  - `git_clone(url, path?, {branch?, depth?, user?})` → `["git","clone", shell_quote(url)]` + (`--branch {shell_quote(branch)} --single-branch`)? + (`--depth {depth}`)? + (`shell_quote(path)`)? → `run_cmd(joined, user)`.

---

## File Structure

- `crates/e2b-rs/src/template/builder.rs` — MODIFY: add all the builder methods (impl blocks on `Template`). Consider a `builder_methods.rs` submodule if `builder.rs` grows too large — but a single file with clear sections is fine. Opt structs (`CopyOpts`/`RemoveOpts`/`RunCmdOpts`/`PackageInstallOpts`/`AptInstallOpts`/`GitCloneOpts`/`CopyItem`) `#[derive(Default)]`.
- `crates/e2b-rs/src/template/mod.rs`, `crates/e2b-rs/src/lib.rs` — re-export the new opt structs + `CopyItem`.
- `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc), `README.md` — MODIFY (Task 4).

---

### Task 1: `from*` image variants + registry variants

**Files:** modify `template/builder.rs`, `mod.rs`, `lib.rs`.

**Interfaces (public, on `Template`, consume `self` → `Template`):** `from_debian_image(self, variant: &str)`, `from_ubuntu_image(self, variant: &str)`, `from_python_image(self, version: &str)`, `from_node_image(self, variant: &str)`, `from_bun_image(self, variant: &str)` — each `self.from_image(&format!("debian:{variant}"))` etc. (Provide the JS defaults via doc — callers pass the variant explicitly; if you want default-arg ergonomics, add `from_debian_image_default(self)` calling with `"stable"`, OR document that the variant is required. KEEP it simple: required `&str` arg.) `from_aws_registry(self, image: &str, access_key_id: &str, secret_access_key: &str, region: &str) -> Template` (set base_image + `registry_config = RegistryConfig::Aws{...}`); `from_gcp_registry(self, image: &str, service_account_json: &str) -> Template`.

- [ ] **Step 1: Write failing tests** — `from_python_image_sets_base` (→ base_image `"python:3.12"`); each variant maps to the right `{repo}:{tag}` (debian/ubuntu/python/node, `oven/bun`). `from_aws_registry_sets_config` (base_image + `RegistryConfig::Aws` with the creds); `from_gcp_registry_sets_config`. (Pure — assert on `Template`'s state via a `pub(crate)` test accessor or by serializing.)
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** the 5 image variants + 2 registry variants.
- [ ] **Step 4: Verify & commit** — clippy + doc clean, fmt last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add from* image + registry builder variants" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: File-op builder methods (copy / copy_items / remove / rename / make_dir / make_symlink)

**Files:** modify `template/builder.rs`, `mod.rs`, `lib.rs`.

**Interfaces (public, on `Template`):** `copy(self, src: &str, dest: &str, opts: CopyOpts) -> Result<Template>` (validate_relative_path → COPY instruction; returns `Result` because validation can fail); `copy_items(self, items: Vec<CopyItem>) -> Result<Template>` (loop `copy`); `remove(self, paths: &[&str], opts: RemoveOpts) -> Template`; `rename(self, src: &str, dest: &str, opts: RenameOpts) -> Template`; `make_dir(self, paths: &[&str], opts: MakeDirOpts) -> Template`; `make_symlink(self, src: &str, dest: &str, opts: MakeSymlinkOpts) -> Template`. Opt structs `#[derive(Default, Clone)]`: `CopyOpts { force_upload: Option<bool>, user: Option<String>, mode: Option<u32>, resolve_symlinks: Option<bool> }`; `CopyItem { src: Vec<String>, dest: String, force_upload, user, mode, resolve_symlinks }` (already exists in Plan-5a types — reuse/align); `RemoveOpts { force: bool, recursive: bool, user: Option<String> }`; `RenameOpts { force: bool, user: Option<String> }`; `MakeDirOpts { user: Option<String>, mode: Option<u32> }`; `MakeSymlinkOpts { user: Option<String> }`.

- [ ] **Step 1: Write failing tests** — `copy_pushes_instruction` (args `[src, dest, user, mode_octal]`, `force_upload`/`resolve_symlinks` carried; the COPY args shape matches what 5c consumes — `args[0]`/`args[1]`); `copy_rejects_absolute_src` (validate_relative_path → Err); `remove_builds_rm` (`rm -r -f <quoted paths>` RUN, user); `rename_builds_mv`; `make_dir_builds_mkdir`; `make_symlink_builds_ln`. (Assert on the pushed `Instruction`s via a `pub(crate)` accessor on `Template`, e.g. `instructions()` — add it if needed.)
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per the JS (`copy` 565, `remove` 641, `rename` 659, `makeDir` 673, `makeSymlink` 688). `pad_octal(mode)` helper. The file-op methods build a `RUN` command via the same logic `run_cmd` uses (you may implement `run_cmd` first if Task 3 hasn't, or factor a private `push_run(cmd, user)`).
- [ ] **Step 4: Verify & commit** — clippy + doc clean, fmt last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add copy/remove/rename/make_dir/make_symlink builder methods" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Commands + env + package installers + git clone

**Files:** modify `template/builder.rs`, `mod.rs`, `lib.rs`.

**Interfaces (public, on `Template`):** `run_cmd(self, command: &str, opts: RunCmdOpts) -> Template` (+ a `run_cmds(self, commands: &[&str], opts)` for the array form — `args = [commands.join(" && ")]` + user); `set_workdir(self, path: &str) -> Template` (WORKDIR); `set_user(self, user: &str) -> Template` (USER); `set_envs(self, envs: BTreeMap<String, String>) -> Template` (skip-empty; ENV args flat k,v); `pip_install(self, packages: &[&str], opts: PipInstallOpts) -> Template` (`g: bool` default TRUE — `PipInstallOpts { global: bool }` with a manual `Default` setting `global=true`, since derive gives false; OR `g: Option<bool>` defaulting true); `npm_install(self, packages: &[&str], opts: NpmInstallOpts { global: bool, dev: bool })`; `bun_install(self, packages: &[&str], opts)`; `apt_install(self, packages: &[&str], opts: AptInstallOpts { no_install_recommends: bool, fix_missing: bool })`; `git_clone(self, url: &str, path: Option<&str>, opts: GitCloneOpts { branch: Option<String>, depth: Option<u32>, user: Option<String> })`. `RunCmdOpts { user: Option<String> }`.

- [ ] **Step 1: Write failing tests** — `run_cmd_pushes_run` (args `[cmd]` + user); `run_cmds_joins_with_and` (`a && b`); `set_workdir`/`set_user`/`set_envs` (ENV flat args, empty skipped); `pip_install_default_global` (`pip install .` when no packages + g=true; `--user` when g=false); `npm_install_flags` (`-g`/`--save-dev`); `apt_install_builds_two_commands` (`apt-get update` + the `DEBIAN_FRONTEND=... apt-get install -y [flags] pkgs`, user root); `git_clone_builds_command` (`git clone <url> --branch B --single-branch --depth N <path>`). Assert the built `RUN` command strings byte-match the JS.
- [ ] **Step 2: Run to verify failure** — FAIL.
- [ ] **Step 3: Implement** per the JS (`runCmd` 705, `setWorkdir` 728, `setUser` 739, `pipInstall` 750, `npmInstall` 778, `bunInstall` 805, `aptInstall` 832, `gitClone` 866, `setEnvs` 915). Port the exact command strings (the brief's pre-verified-facts block has them); `shell_quote` where the JS uses `shellQuote`.
- [ ] **Step 4: Verify & commit** — clippy + doc clean, fmt last.
```bash
cargo fmt --all && git add crates/e2b-rs/src/template crates/e2b-rs/src/lib.rs
git commit -m "feat(template): add run_cmd/env/installers/git_clone builder methods" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Quickstart, parity & full gate (project completion)

**Files:** modify `crates/e2b-rs/src/lib.rs` (crate-doc), `docs/parity-checklist.md`, `README.md`.

- [ ] **Step 1: Quickstart** — extend the `## Templates` `no_run` doctest to show a richer chain: `Template::new().from_python_image("3.12").copy("requirements.txt", "/app/", Default::default())?.run_cmd("pip install -r /app/requirements.txt", Default::default()).set_workdir("/app").set_start_cmd(...)`. Confirm it compiles under `cargo test --doc`.
- [ ] **Step 2: Parity** — fill the `### 5d — Builder methods` subsection: all from* variants, copy/copy_items/remove/rename/make_dir/make_symlink, run_cmd/set_workdir/set_user/set_envs, pip/npm/bun/apt installers, git_clone. Note `addMcpServer` + devcontainer-beta are DEFERRED (user decision). Mark the `## Template (Plan 5)` milestone COMPLETE. Consider a top-level note that the full E2B JS SDK port (sandbox + filesystem + commands + pty + git + volume + template) is now feature-complete.
- [ ] **Step 3: README** — a fuller template-build example.
- [ ] **Step 4: FULL PROJECT GATE** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features` (report counts); `cargo test --doc -p e2b-rs`; `cargo doc --no-deps -p e2b-rs`; `cargo xtask codegen && git status --porcelain` → empty.
- [ ] **Step 5: Commit**
```bash
cargo fmt --all && git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(template): document builder methods + mark Plan 5 complete" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 5d (and the PROJECT) is complete when:
- `Template` exposes the full builder surface: `from_debian/ubuntu/python/node/bun_image`, `from_aws/gcp_registry`, `copy`/`copy_items`/`remove`/`rename`/`make_dir`/`make_symlink`, `run_cmd`/`run_cmds`/`set_workdir`/`set_user`/`set_envs`, `pip/npm/bun/apt_install`, `git_clone` — each producing the correct `Instruction` (command strings byte-match the JS; `copy` produces the 5c-consumed `[src,dest,user,mode]` COPY args).
- `addMcpServer` + devcontainer-beta documented as deferred; no generated type exposed; new opt structs re-exported.
- `cargo fmt --check`, clippy `-D warnings`, `cargo test`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` marks Plan 5 (and the milestone) COMPLETE.

**Carry-forwards (documented):** per-layer `forceNextLayer` simplified to template-level `force`; `BuildOptions` has no `tags` field (multi-tag-via-option); browser-runtime guard (N/A in Rust); stack-trace-to-build-step error mapping. **This is the final sub-plan — after merge, the e2b-rs 1:1 port of the E2B JS SDK is feature-complete (modulo the documented MCP defer + carry-forwards).**
