# Sandbox Git (Plan 4a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use super:subagent-driven-development (recommended) or super:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `sandbox.git()` — git operations (clone/init/status/branches/add/commit/push/pull/checkout/config/...) run inside the sandbox as `git` commands, matching the E2B JS SDK 1:1.

**Architecture:** A `Git` struct that holds a clone of the sandbox's `Commands` (Plan 3c) and runs every operation as `commands.run("git …")` — there is NO git RPC/endpoint; the command string is the wire. Commands are built with `buildGitCommand` (`git [-C <repo>] <args…>`, each part shell-quoted via the existing `crate::utils::shell_quote`). `clone`/`push`/`pull` inject `user:pass` credentials into the remote URL; their `CommandExitError`-equivalent (a non-zero `CommandResult`) is inspected for known auth/upstream stderr substrings and surfaced as `Error::GitAuth`/`Error::GitUpstream`. `status`/`branches` parse `git status --porcelain=1 -b` / `git branch --format=…` output into hand-written `GitStatus`/`GitBranches`.

**Tech Stack:** Rust (edition 2024, MSRV 1.95.0), the Plan-3c `Commands`/`CommandResult`, `crate::utils::shell_quote`, `url` crate (for credential URL parsing — confirm/add); no new transport.

## Global Constraints

- Package `e2b-rs` / lib `e2b_rs`; all crates under `crates/`; edition 2024, MSRV 1.95.0.
- `deny(clippy::unwrap_used, clippy::expect_used, missing_docs, rustdoc::broken_intra_doc_links)` — `unwrap`/`expect` allowed ONLY in `#[cfg(test)]`. Prefer `try_from(...).unwrap_or(...)` over `as`. Use `[crate::Type]` for cross-module intra-doc links.
- **Non-zero exit is NOT an error** for the plain git methods — they return `Ok(CommandResult)` (the caller inspects `exit_code`), consistent with Plan 3c. EXCEPTION: `clone`/`push`/`pull` map a non-zero exit whose output matches the auth/upstream substrings to `Err(Error::GitAuth/GitUpstream)` (matching JS, which throws `GitAuthError`/`GitUpstreamError`); other non-zero exits still return `Ok(CommandResult)`.
- Do NOT expose generated types; `Git` returns the public `CommandResult` (Plan 3c) or hand-written `GitStatus`/`GitBranches`/`String`.
- Every task: `cargo fmt --all` before commit; run `cargo doc --no-deps -p e2b-rs` in the gate (broken intra-doc links are denied and only `cargo doc` catches them). Commit trailer (exact): `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference (source of truth): `/Users/chwzr/flxkpe/superworkspace/rust-sdk/E2B/packages/js-sdk/src/sandbox/git/` (`index.ts`, `utils.ts`).

### Pre-verified facts (confirmed against the codebase at `main` = 0656ec1)
- Plan 3c `Commands` (`crate::sandbox::commands::Commands`) has `pub async fn run(&self, cmd: &str, opts: CommandStartOpts) -> Result<CommandResult>`; `CommandResult { exit_code: i32, error: Option<String>, stdout: String, stderr: String }` (public). `CommandStartOpts { cwd, user, envs, stdin }` (`#[derive(Default)]`). `Commands` fields are `pub(crate)` (`connect: Arc<ConnectClient>`, `envd_version`, `default_user`) — **add `#[derive(Clone)]` to `Commands`** in Task 2 so `Git` can hold a clone (`Arc<ConnectClient>`+`String`+`Option<String>` are all `Clone`).
- `crate::utils::shell_quote(s: &str) -> String` exists (Plan 1; verify exact name/signature in `utils.rs`).
- `Error::GitAuth(String)` + `Error::GitUpstream(String)` exist (`errors.rs`); both `is_authentication()`-true. `Error::InvalidArgument(String)` exists (for the credential validation).
- `Sandbox` (`sandbox/sandbox.rs`): `from_api_sandbox(s, config, api) -> Result<Sandbox>` already builds `files`/`commands`/`pty`; add a `git` field built from the `commands` clone. `local_sandbox` test helper also constructs the literal — add `git` there. `Sandbox::commands()` exists.
- JS `buildGitCommand(args, repoPath?)` = `["git", ("-C", repoPath)?, ...args]` each `shellQuote`'d, joined by `" "`. Auth substrings (lowercased match against `"{stderr}\n{stdout}"`): `authentication failed`, `terminal prompts disabled`, `could not read username`, `invalid username or password`, `access denied`, `permission denied`, `not authorized`. Upstream substrings: `has no upstream branch`, `no upstream branch`, `no upstream configured`, `no tracking information for the current branch`, `no tracking information`, `set the remote as upstream`, `set the upstream branch`, `please specify which branch you want to merge with`.
- `withCredentials(url, user, pass)`: if neither set → return url unchanged; if exactly one set → `Error::InvalidArgument("Both username and password are required…")`; else parse URL (must be `http`/`https`, else `Error::InvalidArgument`), set username+password, return. `deriveRepoDirFromUrl(url)`: parse, trim trailing `/`, take last path segment, strip a trailing `.git`; `None` on parse failure / empty.
- The `url` crate: check `cargo tree -p e2b-rs | grep '^url'` — reqwest depends on it, but it may not be a DIRECT dep. If `reqwest::Url` is accessible use it (it is — Plan 3a-extras used `reqwest::Url`); prefer `reqwest::Url` for credential parsing (`set_username`/`set_password`) to avoid a new dep.

---

## File Structure

- `crates/e2b-rs/src/sandbox/git/mod.rs` — CREATE: `Git` struct + construction + all git methods.
- `crates/e2b-rs/src/sandbox/git/types.rs` — CREATE: public `GitStatus`/`GitFileStatus`/`GitStatusLabel`/`GitBranches`/`GitConfigScope` + option structs.
- `crates/e2b-rs/src/sandbox/git/util.rs` — CREATE: `build_git_command`, `with_credentials`, `strip_credentials`, `derive_repo_dir_from_url`, `is_auth_failure`, `is_missing_upstream`, `parse_git_status`, `parse_git_branches` (the bug-prone pure functions + their unit tests).
- `crates/e2b-rs/src/sandbox/commands/mod.rs` — MODIFY: `#[derive(Clone)]` on `Commands`.
- `crates/e2b-rs/src/sandbox/sandbox.rs` — MODIFY: `git: Git` field + `Sandbox::git()`; built in `from_api_sandbox` + `local_sandbox`.
- `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs` — MODIFY: re-export public git types.
- `docs/parity-checklist.md`, `README.md` — MODIFY (Task 5).

---

### Task 1: Git util functions + public types (the bug-prone pure layer)

**Files:**
- Create: `crates/e2b-rs/src/sandbox/git/types.rs`, `crates/e2b-rs/src/sandbox/git/util.rs`, `crates/e2b-rs/src/sandbox/git/mod.rs` (module wiring + re-exports; the `Git` struct arrives in Task 2)
- Modify: `crates/e2b-rs/src/sandbox/mod.rs`, `crates/e2b-rs/src/lib.rs`

**Interfaces:**
- Produces (public, in `types.rs`): `enum GitConfigScope { Global, Local, System }`; `enum GitStatusLabel { Conflict, Renamed, Copied, Deleted, Added, Modified, TypeChange, Untracked, Unknown }`; `struct GitFileStatus { name, status: GitStatusLabel, index_status: char, working_tree_status: char, staged: bool, renamed_from: Option<String> }`; `struct GitStatus { current_branch: Option<String>, upstream: Option<String>, ahead: i32, behind: i32, detached: bool, file_status: Vec<GitFileStatus>, is_clean: bool, has_changes: bool, has_staged: bool, has_untracked: bool, has_conflicts: bool, total_count: usize, staged_count: usize, unstaged_count: usize, untracked_count: usize, conflict_count: usize }`; `struct GitBranches { branches: Vec<String>, current_branch: Option<String> }`.
- Produces (`pub(crate)`, in `util.rs`): `build_git_command(args: &[&str], repo_path: Option<&str>) -> String`; `with_credentials(url: &str, username: Option<&str>, password: Option<&str>) -> Result<String>`; `strip_credentials(url: &str) -> String`; `derive_repo_dir_from_url(url: &str) -> Option<String>`; `is_auth_failure(result: &CommandResult) -> bool`; `is_missing_upstream(result: &CommandResult) -> bool`; `parse_git_status(output: &str) -> GitStatus`; `parse_git_branches(output: &str) -> GitBranches`.

- [ ] **Step 1: Write the failing tests** (in `util.rs` `#[cfg(test)] mod tests`):
```rust
//! Pure git command-building, credential, and output-parsing helpers.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_command_with_repo_and_quotes() {
        assert_eq!(build_git_command(&["status"], None), "git status");
        assert_eq!(
            build_git_command(&["commit", "-m", "a b"], Some("/repo")),
            "git -C /repo commit -m 'a b'"
        );
    }

    #[test]
    fn credentials_validate_and_inject() {
        assert_eq!(with_credentials("https://h/r.git", None, None).expect("noop"), "https://h/r.git");
        assert!(with_credentials("https://h/r.git", Some("u"), None).is_err()); // one-of
        assert!(with_credentials("ftp://h/r", Some("u"), Some("p")).is_err()); // non-http(s)
        let u = with_credentials("https://h/r.git", Some("u"), Some("p")).expect("creds");
        assert!(u.starts_with("https://u:p@h"));
    }

    #[test]
    fn derives_repo_dir() {
        assert_eq!(derive_repo_dir_from_url("https://h/owner/repo.git").as_deref(), Some("repo"));
        assert_eq!(derive_repo_dir_from_url("https://h/owner/repo/").as_deref(), Some("repo"));
        assert_eq!(derive_repo_dir_from_url("not a url"), None);
    }

    #[test]
    fn detects_auth_and_upstream() {
        let auth = CommandResult { exit_code: 128, error: None, stdout: String::new(),
            stderr: "fatal: Authentication failed for 'x'".to_string() };
        assert!(is_auth_failure(&auth));
        let up = CommandResult { exit_code: 128, error: None, stdout: String::new(),
            stderr: "fatal: The current branch has no upstream branch.".to_string() };
        assert!(is_missing_upstream(&up));
        let ok = CommandResult { exit_code: 0, error: None, stdout: String::new(), stderr: String::new() };
        assert!(!is_auth_failure(&ok) && !is_missing_upstream(&ok));
    }

    #[test]
    fn parses_status_branch_ahead_behind_and_files() {
        let out = "## main...origin/main [ahead 1, behind 2]\n M src/a.rs\n?? new.txt\nA  staged.txt\n";
        let s = parse_git_status(out);
        assert_eq!(s.current_branch.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.ahead, 1);
        assert_eq!(s.behind, 2);
        assert_eq!(s.file_status.len(), 3);
        assert!(s.has_untracked);
        assert!(s.has_staged);
        assert!(!s.is_clean);
    }

    #[test]
    fn parses_clean_status() {
        let s = parse_git_status("## main\n");
        assert!(s.is_clean);
        assert_eq!(s.file_status.len(), 0);
    }

    #[test]
    fn parses_branches_with_current() {
        let b = parse_git_branches("main\t*\nfeature\t\n");
        assert_eq!(b.branches, vec!["main".to_string(), "feature".to_string()]);
        assert_eq!(b.current_branch.as_deref(), Some("main"));
    }
}
```
Add to `sandbox/mod.rs`: `pub(crate) mod git;`. In `git/mod.rs`: `pub mod types; pub(crate) mod util;` + `pub use types::{GitBranches, GitConfigScope, GitFileStatus, GitStatus, GitStatusLabel};`. Re-export those public types through `sandbox/mod.rs` → `lib.rs`.

- [ ] **Step 2: Run to verify failure** — `cargo test -p e2b-rs sandbox::git::util` → FAIL (functions/types not defined).

- [ ] **Step 3: Implement the public types** (`git/types.rs`) — the structs/enums from the Interfaces block, each with `///` docs on every public item + field, `#[derive(Debug, Clone)]` (and `PartialEq, Eq` on the enums + `Copy` where trivial).

- [ ] **Step 4: Implement the util functions** (`git/util.rs`). Port the JS faithfully:
```rust
use super::types::{GitBranches, GitFileStatus, GitStatus, GitStatusLabel};
use crate::errors::{Error, Result};
use crate::sandbox::commands::CommandResult;

/// Build a shell command: `git [-C <repo>] <args…>`, each part shell-quoted.
pub(crate) fn build_git_command(args: &[&str], repo_path: Option<&str>) -> String {
    let mut parts: Vec<String> = vec!["git".to_string()];
    if let Some(repo) = repo_path {
        parts.push("-C".to_string());
        parts.push(repo.to_string());
    }
    parts.extend(args.iter().map(|a| (*a).to_string()));
    parts
        .iter()
        .map(|p| crate::utils::shell_quote(p))
        .collect::<Vec<_>>()
        .join(" ")
}

const AUTH_SNIPPETS: &[&str] = &[
    "authentication failed",
    "terminal prompts disabled",
    "could not read username",
    "invalid username or password",
    "access denied",
    "permission denied",
    "not authorized",
];

const UPSTREAM_SNIPPETS: &[&str] = &[
    "has no upstream branch",
    "no upstream branch",
    "no upstream configured",
    "no tracking information for the current branch",
    "no tracking information",
    "set the remote as upstream",
    "set the upstream branch",
    "please specify which branch you want to merge with",
];

fn output_matches(result: &CommandResult, snippets: &[&str]) -> bool {
    let haystack = format!("{}\n{}", result.stderr, result.stdout).to_lowercase();
    snippets.iter().any(|s| haystack.contains(s))
}

/// Whether a failed git command's output indicates an auth failure.
pub(crate) fn is_auth_failure(result: &CommandResult) -> bool {
    output_matches(result, AUTH_SNIPPETS)
}

/// Whether a failed git command's output indicates a missing upstream branch.
pub(crate) fn is_missing_upstream(result: &CommandResult) -> bool {
    output_matches(result, UPSTREAM_SNIPPETS)
}

/// Embed `user:pass` into an http(s) git URL. Returns the URL unchanged if both
/// are unset; errors if exactly one is set or the URL is not http(s).
pub(crate) fn with_credentials(
    url: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<String> {
    match (username, password) {
        (None, None) => Ok(url.to_string()),
        (Some(_), None) | (None, Some(_)) => Err(Error::InvalidArgument(
            "Both username and password are required when using Git credentials.".to_string(),
        )),
        (Some(user), Some(pass)) => {
            let mut parsed = reqwest::Url::parse(url)
                .map_err(|_| Error::InvalidArgument(format!("Invalid Git URL: {url}")))?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(Error::InvalidArgument(
                    "Only http(s) Git URLs support username/password credentials.".to_string(),
                ));
            }
            parsed
                .set_username(user)
                .map_err(|()| Error::InvalidArgument("Invalid Git URL host".to_string()))?;
            parsed
                .set_password(Some(pass))
                .map_err(|()| Error::InvalidArgument("Invalid Git URL host".to_string()))?;
            Ok(parsed.to_string())
        }
    }
}

/// Remove any `user:pass` from an http(s) URL; returns the input unchanged if it
/// can't be parsed (safe fallback).
pub(crate) fn strip_credentials(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(mut parsed) => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        Err(_) => url.to_string(),
    }
}

/// Default clone destination: the URL's last path segment with `.git` stripped.
pub(crate) fn derive_repo_dir_from_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let trimmed = parsed.path().trim_end_matches('/');
    let last = trimmed.rsplit('/').next()?;
    if last.is_empty() {
        return None;
    }
    Some(last.strip_suffix(".git").unwrap_or(last).to_string())
}
```
Then the parsers — port `parseGitStatus`/`parseGitBranches`/`parseAheadBehind`/`normalizeBranchName`/the status-label mapping from `git/utils.ts` (read it: the `## branch...upstream [ahead N, behind M]` line; `?? path` = untracked; `XY path` where `X`=index status, `Y`=worktree status, with ` -> ` rename detection; the `GitStatusLabel` mapping from the index/worktree codes; and the aggregate counts `is_clean`/`has_*`/`*_count`). **IMPLEMENTER: port `parse_git_status` line-by-line from `git/utils.ts`'s `parseGitStatus` (around the `export function parseGitStatus` definition) — it is the bug-prone piece; the 2 status tests + the clean-status test are the contract.** `parse_git_branches`: split each non-empty trimmed line on `\t`; the first field is the branch name (push it), and if the second field is `*` that branch is `current_branch`.

- [ ] **Step 5: Run tests + re-export** — `cargo test -p e2b-rs sandbox::git::util` → all pass. Re-export `GitStatus`/`GitFileStatus`/`GitStatusLabel`/`GitBranches`/`GitConfigScope` through `sandbox/mod.rs` → `lib.rs`.

- [ ] **Step 6: Verify & commit** — `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc --no-deps -p e2b-rs` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/git crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(git): add git util helpers + public status/branch types" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: `Git` struct, construction, and the command-wrapper methods

**Files:**
- Modify: `crates/e2b-rs/src/sandbox/git/mod.rs`, `crates/e2b-rs/src/sandbox/commands/mod.rs` (`#[derive(Clone)]`), `crates/e2b-rs/src/sandbox/sandbox.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone)]` on `Commands`.
  - `pub struct Git { commands: Commands }` + `pub(crate) fn Git::new(commands: Commands) -> Git`.
  - Option structs (`#[derive(Default)]`): `GitInitOpts { initial_branch: Option<String>, bare: bool, user: Option<String> }`; `GitCommitOpts { author_name: Option<String>, author_email: Option<String>, allow_empty: bool, user: Option<String> }`; `GitAddOpts { all: bool, files: Vec<String>, user: Option<String> }`; etc. (one per method that needs options — keep each minimal, matching the JS opts).
  - The straightforward methods (each `pub async fn …(&self, …) -> Result<CommandResult>` running `self.commands.run(build_git_command(&args, Some(path)), CommandStartOpts{ user, ..})`): `init`, `add`, `commit`, `create_branch`, `checkout_branch`, `delete_branch`, `reset`, `restore`, `set_config`, `get_config` (→ `Result<Option<String>>`), `configure_user`, `remote_add`, `remote_get` (→ `Result<Option<String>>`).
  - `Sandbox`: `pub(crate) git: Git` field; `pub fn git(&self) -> &Git`; built in `from_api_sandbox` (`Git::new(commands.clone())`) + `local_sandbox`.

- [ ] **Step 1: Write the failing test** — in `git/mod.rs` `mod tests`, build a `Git` over a mock `Commands` (reuse the Plan-3c `commands` test seam: `Commands::build_with_connect(connect_for(&server), "0.6.3")`, then `Git::new(commands)`). Mock the `Start` stream to echo a process; assert a simple method (e.g. `git.checkout_branch("/repo", "main", Default::default())`) runs and returns a `CommandResult`. (Like the Plan-3c `run_foreground` test: match method+path on `/process.Process/Start`, stream Start+End; the command string itself isn't asserted at the wire here — a focused `commit` test that asserts the built command string can use `build_git_command` directly in `util.rs` tests; the integration test just confirms the method calls `commands.run`.)

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement** — add `#[derive(Clone)]` to `Commands` (`commands/mod.rs`); implement `Git` + the methods in `git/mod.rs`. Each method builds its arg vector per the JS command table and calls `self.commands.run(&build_git_command(&args, Some(path)), CommandStartOpts { user: opts.user, ..Default::default() })`. Command strings (from `git/index.ts` — port the exact flag handling):
  - `init(path, opts)` → `git init [--initial-branch <b>] [--bare] <path>` (the `path` is the positional arg, NOT `-C`; init creates the dir).
  - `add(path, opts)` → `git -C <path> add [-A | .] [-- <files…>]` (JS: `--` then files if `files` given, else `-A` or `.`; check `git/index.ts:547`).
  - `commit(path, message, opts)` → `git -C <path> [-c user.name=<n> -c user.email=<e>] commit -m <message> [--allow-empty]`.
  - `create_branch(path, branch, opts)` → `git -C <path> checkout -b <branch>`.
  - `checkout_branch(path, branch, opts)` → `git -C <path> checkout <branch>`.
  - `delete_branch(path, branch, opts)` → `git -C <path> branch [-D | -d] <branch>` (force → `-D`).
  - `reset(path, opts)` → `git -C <path> reset [--soft|--mixed|--hard|--merge|--keep] [<target>] [-- <paths…>]`.
  - `restore(path, opts)` → `git -C <path> restore [--worktree] [--staged] [--source <src>] [-- <paths…>]`.
  - `set_config(path, key, value, opts)` → `git -C <path> config [--global|--local|--system] <key> <value>`.
  - `get_config(path, key, opts) -> Result<Option<String>>` → `git -C <path> config [scope] --get <key>` ; return `Some(stdout.trim())` if non-empty exit 0, else `None` (JS appends `|| true` then checks; mirror: on non-zero exit or empty stdout → `None`).
  - `configure_user(path, name, email, opts)` → two configs (`user.name`, `user.email`) — JS runs them; you may run a single composite `git -C <path> config <scope> user.name <n> && git -C <path> config <scope> user.email <e>` OR two `commands.run` calls (match JS at `git/index.ts:898` — likely two calls; return the second `CommandResult`).
  - `remote_add(path, name, url, opts)` → `git -C <path> remote add [-f] <name> <url>` (the composite set-url||add form at `git/index.ts:398` — port faithfully).
  - `remote_get(path, name, opts) -> Result<Option<String>>` → `git -C <path> remote get-url <name>` ; `Some(stdout.trim())` on success else `None`.
  Every public item documented; `scope` flag from `GitConfigScope` → `--global`/`--local`/`--system`. Wire `Sandbox.git` (field + `git()` accessor + build in `from_api_sandbox` via `Git::new(commands.clone())` + `local_sandbox`).

- [ ] **Step 4: Run tests** — `cargo test -p e2b-rs sandbox::` pass.

- [ ] **Step 5: Re-export `Git` + opts; verify & commit** — re-export `Git` + the opts structs (`sandbox/mod.rs` → `lib.rs`). clippy + `cargo doc` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/git crates/e2b-rs/src/sandbox/commands/mod.rs crates/e2b-rs/src/sandbox/sandbox.rs crates/e2b-rs/src/sandbox/mod.rs crates/e2b-rs/src/lib.rs
git commit -m "feat(git): add Git struct + command-wrapper methods wired into Sandbox" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: `clone` / `push` / `pull` (credential injection + auth/upstream error mapping)

**Files:** Modify `crates/e2b-rs/src/sandbox/git/mod.rs`.

**Interfaces:**
- Produces (on `Git`):
  - `pub struct GitCloneOpts { branch: Option<String>, depth: Option<u32>, path: Option<String>, username: Option<String>, password: Option<String>, store_credentials: bool, user: Option<String> }` (`#[derive(Default)]`).
  - `pub async fn clone(&self, url: &str, opts: GitCloneOpts) -> Result<CommandResult>` — builds the URL via `with_credentials`, runs `git clone [--branch <b> --single-branch] [--depth <d>] <url> [<path>]`; if `!store_credentials`, runs `git -C <dest> remote set-url origin <strip_credentials(url)>` after; on a non-zero result matching `is_auth_failure` → `Err(Error::GitAuth(<message>))`.
  - `pub struct GitPushOpts { remote: Option<String>, branch: Option<String>, set_upstream: bool, username: Option<String>, password: Option<String>, user: Option<String> }`; `pub async fn push(&self, path: &str, opts: GitPushOpts) -> Result<CommandResult>` — optional `with_remote_credentials` (get current remote URL, temporarily set to credentialed URL, push, restore), `git push [--set-upstream <remote> <branch>]`; non-zero + `is_auth_failure` → `Err(GitAuth)`, + `is_missing_upstream` → `Err(GitUpstream)`.
  - `pub struct GitPullOpts { remote: Option<String>, branch: Option<String>, username: Option<String>, password: Option<String>, user: Option<String> }`; `pub async fn pull(&self, path: &str, opts: GitPullOpts) -> Result<CommandResult>` — same credential dance, `git pull [<remote>] [<branch>]`; non-zero + auth/upstream mapping.

- [ ] **Step 1: Write failing tests** — these run over the mock `Commands`. For `clone` auth: mock `/process.Process/Start` to stream a process whose End event has a non-zero exit AND whose stdout/stderr (Data events) contain `"fatal: Authentication failed"`; assert `git.clone(url, GitCloneOpts{ username: Some("u"), password: Some("p"), ..})` returns `Err(Error::GitAuth(_))`. (Stream a `{"event":{"data":{"stderr":"<base64 of 'fatal: Authentication failed'>"}}}` frame + `{"event":{"end":{"exitCode":128,…}}}`.) A `clone` happy-path test: exit 0 → `Ok(CommandResult)`. NOTE the streaming-request caveat from Plan 3c: match method+path only, not `body_partial_json` (the Start request is a binary Connect envelope).

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement** — port `clone`/`push`/`pull` from `git/index.ts` (lines ~296/681/740). Credential flow per the JS: `clone` embeds creds in the URL, runs clone, then (unless `store_credentials`) resets origin to the stripped URL; `push`/`pull` use `with_remote_credentials` (read `git remote get-url <remote>`, set-url to credentialed, run, set-url back to original). The error mapping wraps the FINAL `CommandResult`: `if result.exit_code != 0 { if is_auth_failure(&result) { return Err(Error::GitAuth(<msg>)); } if is_missing_upstream(&result) { return Err(Error::GitUpstream(<msg>)); } }` then `Ok(result)`. (`clone` only checks auth; `push`/`pull` check both.) Build the auth-error message from the result's stderr (JS `buildAuthErrorMessage` — port it, or use a concise message including the stderr).

- [ ] **Step 4: Run tests + commit** — clippy + `cargo doc` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/git/mod.rs
git commit -m "feat(git): add clone/push/pull with credential injection + auth error mapping" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `status` / `branches` (parsing) + `dangerously_authenticate`

**Files:** Modify `crates/e2b-rs/src/sandbox/git/mod.rs`.

**Interfaces:**
- Produces (on `Git`):
  - `pub async fn status(&self, path: &str, user: Option<&str>) -> Result<GitStatus>` — runs `git -C <path> status --porcelain=1 -b`, parses stdout via `parse_git_status`.
  - `pub async fn branches(&self, path: &str, user: Option<&str>) -> Result<GitBranches>` — runs `git -C <path> branch --format=%(refname:short)\t%(HEAD)`, parses via `parse_git_branches`.
  - `pub struct GitAuthenticateOpts { username: String, password: String, host: Option<String>, protocol: Option<String>, user: Option<String> }`; `pub async fn dangerously_authenticate(&self, opts: GitAuthenticateOpts) -> Result<CommandResult>` — `git config --global credential.helper store` then `git credential approve` with the credential block on stdin (port `git/index.ts:855`; deliver the stdin via `commands.run` with a heredoc/`printf` piped to `git credential approve`, OR via the command's stdin — JS uses `shellQuote`'d stdin injection; mirror the JS command construction).

- [ ] **Step 1: Write failing tests** — `status`: stream a process whose stdout (Data stdout frame) is the base64 of `"## main...origin/main [ahead 1]\n M a.rs\n"` and End exit 0; assert `git.status("/repo", None)` → `GitStatus { current_branch: Some("main"), ahead: 1, file_status.len()==1, .. }`. `branches`: stdout = `"main\t*\nfeature\t\n"`; assert `current_branch == Some("main")`, 2 branches. (Match method+path only.)

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement** — `status`/`branches` run the command, then `parse_git_status(&result.stdout)` / `parse_git_branches(&result.stdout)`. (If `result.exit_code != 0`, JS still parses; mirror — parse the stdout regardless, OR return the parsed result. Confirm against `git/index.ts:465/481`.) `dangerously_authenticate` per the JS.

- [ ] **Step 4: Run tests + commit** — clippy + `cargo doc` clean.
```bash
cargo fmt --all
git add crates/e2b-rs/src/sandbox/git/mod.rs
git commit -m "feat(git): add status/branches parsing + dangerously_authenticate" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Parity checklist, quickstart & full gate

**Files:** Modify `docs/parity-checklist.md`, `crates/e2b-rs/src/lib.rs` (crate-doc), `README.md`.

- [ ] **Step 1: Crate quickstart** — add a `## Git` `no_run` doctest to `lib.rs` `//!` docs: `sandbox.git().clone("https://github.com/owner/repo.git", Default::default()).await?;` then `let status = sandbox.git().status("/home/user/repo", None).await?; println!("{:?}", status.current_branch);`. Verify signatures so it compiles under `no_run`.
- [ ] **Step 2: Parity checklist** — add `## Sandbox git (Plan 4a)` table (clone/init/status/branches/add/commit/push/pull/checkout/createBranch/deleteBranch/reset/restore/setConfig/getConfig/remoteAdd/remoteGet/configureUser/dangerouslyAuthenticate). Note non-zero-exit = `Ok(CommandResult)` except clone/push/pull auth/upstream → `Err`.
- [ ] **Step 3: README** — short git snippet. Only stage if changed.
- [ ] **Step 4: Full release gate** — `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features` (0 failures, report counts); `cargo test --doc -p e2b-rs`; `cargo doc --no-deps -p e2b-rs`; `cargo xtask codegen && git status --porcelain` → empty.
- [ ] **Step 5: Commit**
```bash
cargo fmt --all
git add crates/e2b-rs/src/lib.rs docs/parity-checklist.md README.md
git commit -m "docs(git): document git quickstart and parity" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Plan completion criteria

Plan 4a is complete when:
- `Sandbox::git()` exposes `clone`/`init`/`status`/`branches`/`add`/`commit`/`push`/`pull`/`create_branch`/`checkout_branch`/`delete_branch`/`reset`/`restore`/`set_config`/`get_config`/`remote_add`/`remote_get`/`configure_user`/`dangerously_authenticate`.
- `clone`/`push`/`pull` inject credentials into http(s) URLs and map auth/upstream failures to `Error::GitAuth`/`GitUpstream`; other non-zero exits return `Ok(CommandResult)`. `status`/`branches` parse into the public `GitStatus`/`GitBranches`.
- All public types (`Git`, `GitStatus`, `GitFileStatus`, `GitStatusLabel`, `GitBranches`, `GitConfigScope`, the opt structs) re-exported at the crate root; no generated type leaked.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo test --doc`, `cargo doc --no-deps` all pass; codegen idempotent.
- `docs/parity-checklist.md` reflects git.

**Carry-forwards (documented):** `with_remote_credentials` URL restore is best-effort (if the original remote read fails, behavior matches JS's fallback); `buildAuthErrorMessage` richness; any git method whose exact flag set differs from the JS table should be reconciled in review against `git/index.ts`.

**Next:** Plan 4b (Volume) — top-level `Volume` resource (control-plane CRUD via `ApiClient` + volume-content read/write/list via a new Bearer-token `VolumeApiClient`), wrapping `volume::schema` types.
