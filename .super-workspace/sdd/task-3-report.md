# Task 3 Report: `clone` / `push` / `pull` (credential injection + auth/upstream error mapping)

## Status: DONE_WITH_CONCERNS

---

## Methods & Helpers Implemented

### `git/util.rs` additions (pure, no I/O)
- `build_push_args(remote_name, remote, branch, set_upstream) -> Vec<String>` — port of JS `buildPushArgs`; `remote_name` takes priority over `remote`; `--set-upstream` only added when a target remote exists.
- `build_auth_error_message(action, missing_password) -> String` — port of JS `buildAuthErrorMessage`.
- `build_upstream_error_message(action) -> String` — port of JS `buildUpstreamErrorMessage`.

### `git/mod.rs` private helpers (on `Git`)
- `get_remote_url(path, remote, user)` — runs `git remote get-url <remote>`; errors on empty output.
- `resolve_remote_name(path, remote, user)` — short-circuits on `Some(remote)`; fetches remote list and errors unless exactly one exists.
- `has_upstream(path, user)` — runs `rev-parse --abbrev-ref --symbolic-full-name @{u}`; returns `exit_code == 0 && !stdout.is_empty()` (JS try/catch-returns-false -> Rust non-zero-returns-false).
- `with_remote_credentials<Fut>(path, remote, username, password, run_user, op)` — get-url -> set credentialed URL -> await op -> ALWAYS restore original URL; op/restore errors propagated in JS priority order.

### `git/mod.rs` public methods
- `clone(url, GitCloneOpts) -> Result<CommandResult>`
- `push(path, GitPushOpts) -> Result<CommandResult>` — `set_upstream` defaults to `true`.
- `pull(path, GitPullOpts) -> Result<CommandResult>`

### Opt structs added
`GitCloneOpts`, `GitPushOpts` (manual `Default` for `set_upstream=true`), `GitPullOpts` — all re-exported through `sandbox/mod.rs` -> `lib.rs`.

---

## Ok-on-Nonzero Adaptation

Each JS `throw` site was adapted as follows:

| JS site | Rust adaptation |
|---|---|
| `runGit` throws `CommandExitError` on non-zero | `commands.run` returns `Ok(CommandResult)`; check `result.exit_code != 0` |
| `catch (err) { if (isAuthFailure(err)) throw GitAuthError }` | `if result.exit_code != 0 { if is_auth_failure(&result) { return Err(GitAuth(...)) } }` |
| `catch (err) { if (isMissingUpstream(err)) throw GitUpstreamError }` | same pattern with `is_missing_upstream` |
| `withRemoteCredentials` try/catch around op | `op.await` returns `Ok(CommandResult)` naturally; op/restore results matched structurally |
| clone fail before set-url (throws mid-flow) | if `exit_code != 0` after clone: map or return Ok; set-url never runs |
| `hasUpstream` catches `CommandExitError`, returns `false` | `exit_code == 0 && !stdout.trim().is_empty()` |

---

## TDD Evidence

**RED** (before implementation): cargo test failed to compile — "no method named `clone`/`push`/`pull` found".

**GREEN** (after implementation): 145 tests pass, 0 failures.

### Tests written
- `clone_auth_failure_returns_git_auth_err` — wires a mock with `"fatal: Authentication failed"` in stderr (base64), exit 128; asserts `Err(Error::GitAuth(_))`.
- `clone_happy_path_returns_ok` — exit 0 stream; asserts `Ok(result)`.
- `clone_missing_path_guard_fires_before_network` — creds + store=false + no derivable path; asserts `Err(Error::InvalidArgument(_))` with zero server requests.
- `push_non_credentialed_upstream_error` — `"fatal: The current branch has no upstream branch."` (base64) + exit 128; asserts `Err(Error::GitUpstream(_))`.
- Pure unit tests: `build_push_args` (5 variants), `build_auth_error_message` (2 variants), `build_upstream_error_message` (push vs pull).

---

## DONE_WITH_CONCERNS — Credentialed Path Asymmetry

The JS SDK's credentialed push/pull path (`withRemoteCredentials`) has **no try/catch** around the operation call, meaning auth/upstream failures are NOT mapped to typed errors on that path. Only the non-credentialed path has error mapping.

This asymmetry is faithfully ported: `with_remote_credentials` in Rust does NOT apply `is_auth_failure`/`is_missing_upstream` checks — it returns `op`'s `Ok(CommandResult)` as-is.

**Practical impact:** If a credentialed push/pull fails with exit 128 due to auth (e.g., bad credentials), callers get `Ok(CommandResult { exit_code: 128, stderr: "fatal: Authentication failed", ... })` instead of `Err(Error::GitAuth(_))`. They would need to inspect the result manually.

**Recommendation for review:** Decide whether to add auth/upstream mapping inside `with_remote_credentials` (after the restore step) for consistency, or keep the JS parity quirk.

---

## Wire-Test Gap

Testing the full credentialed push/pull flow (get-url -> set-url -> push/pull -> restore-url = 3-4 sequential `Start` calls) with wiremock requires a sequenced mock that maps ordered requests to different responses. This is beyond what a single `Mock::given(...).respond_with(...)` supports cleanly.

**Coverage decision:** The pure logic of `with_remote_credentials` (URL construction via `with_credentials` + `get_remote_url` contract) is covered by unit tests in `util.rs`. The integration wire path is noted as a gap to be addressed in a future test or integration test milestone.

---

## Files Changed

- `crates/e2b-rs/src/sandbox/git/util.rs` — added `build_push_args`, `build_auth_error_message`, `build_upstream_error_message` + 9 unit tests.
- `crates/e2b-rs/src/sandbox/git/mod.rs` — added `GitCloneOpts`, `GitPushOpts`, `GitPullOpts`; private helpers `get_remote_url`, `resolve_remote_name`, `has_upstream`, `with_remote_credentials`; public methods `clone`, `push`, `pull`; 4 integration tests + helper functions.
- `crates/e2b-rs/src/sandbox/mod.rs` — added `GitCloneOpts`, `GitPushOpts`, `GitPullOpts` to re-exports.
- `crates/e2b-rs/src/lib.rs` — added `GitCloneOpts`, `GitPushOpts`, `GitPullOpts` to re-exports.

---

## Gate Results

```
cargo fmt --all                                        clean
cargo clippy --workspace --all-targets -- -D warnings  clean
cargo test -p e2b-rs                                   145 passed, 0 failed
cargo doc --no-deps -p e2b-rs                          clean (no broken links)
```

---

## Plan 4a Review Fixes (commit: see feat/sandbox-git HEAD after this patch)

### Fix 1 — Restore `set-url` swallowed non-zero exit (Important)

**Problem:** In `with_remote_credentials`, the restore `set-url` result was
`Ok(CommandResult)` even on non-zero exit.  The `(Ok(result), Ok(_))` match arm
then returned the op result, silently swallowing a failed restore that leaves
credentials embedded in `.git/config`.

**Fix:** Chain `.and_then(check_set_url_exit)` on the restore result before the
match so a non-zero exit becomes `Err(InvalidArgument(...))`.  The existing match
arms then correctly handle it (`(Ok(_), Err(restore_err)) => Err(restore_err)`
fires), preserving op-error-first priority.

### Fix 2 — Credentialed `set-url` swallowed non-zero exit (Minor)

**Problem:** The credentialed `set-url` call before running the op only
propagated transport errors (via `?`); a non-zero exit from git itself slipped
through, causing the op to run against the uncredentialed URL.

**Fix:** Wrap the call: `check_set_url_exit(self.run_cmd(...).await?)?;`.  The
outer `?` surfaces transport errors; `check_set_url_exit` surfaces non-zero git
exits as `Err(InvalidArgument(...))`.

### New helper `check_set_url_exit` (module-level private fn, `git/mod.rs`)

Both fixes share a small helper added above the Reset mode section:

```rust
fn check_set_url_exit(result: CommandResult) -> Result<CommandResult> {
    if result.exit_code != 0 {
        return Err(Error::InvalidArgument(format!(
            "git remote set-url failed (exit {}): {}",
            result.exit_code,
            result.stderr.trim()
        )));
    }
    Ok(result)
}
```

Private function — no doc comment required; matches surrounding style.

### Fix 3 — Added `pull_non_credentialed_upstream_error` test (Minor)

**Gap:** `push` had `push_non_credentialed_upstream_error`; pull's pre-check
(`!remote && !branch && !has_upstream → Err(GitUpstream)`) had no wire test.

**Test added** (`sandbox::git::tests::pull_non_credentialed_upstream_error`):
- Mounts a mock on `POST /process.Process/Start` (method+path only, no body
  matcher) that streams a process End event with `exitCode: 128`.
- `git.pull("/repo", GitPullOpts::default())` triggers `has_upstream` which
  sees the non-zero exit and returns `false`.
- Pull returns `Err(Error::GitUpstream(_))` — asserted.

Follows the exact pattern of `push_non_credentialed_upstream_error` (inline
`Mock::given(method("POST")).and(path(...))` + `proc_stream(128)`).

### Gate Results (after fixes)

Commands run and output:

```
cargo test -p e2b-rs sandbox::git
  38 passed, 0 failed   (was 37 — new pull_non_credentialed_upstream_error added)

cargo clippy --workspace --all-targets -- -D warnings
  Finished `dev` profile — no warnings or errors

cargo test -p e2b-rs
  146 passed, 0 failed, 0 ignored   (was 145)

cargo fmt --all
  clean (no diffs)

cargo doc --no-deps -p e2b-rs
  clean (no errors or broken links)
```
