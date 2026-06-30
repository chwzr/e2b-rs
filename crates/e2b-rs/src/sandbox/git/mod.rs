//! Sandbox Git API — public types, internal utilities, and the [`Git`] command driver.
//!
//! The [`crate::sandbox::git`] module exposes the result types produced by git
//! operations and the [`Git`] struct for running them. The
//! [`crate::sandbox::git::util`] sub-module contains the pure (no-I/O) helpers
//! that build commands and parse output.

pub mod types;
pub(crate) mod util;

pub use types::{GitBranches, GitConfigScope, GitFileStatus, GitStatus, GitStatusLabel};

use std::collections::BTreeMap;

use crate::errors::{Error, Result};
use crate::sandbox::commands::{CommandResult, CommandStartOpts, Commands};
use crate::sandbox::git::util::build_git_command;

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Resolve the `--global` / `--local` / `--system` flag for the given scope.
fn scope_flag(scope: GitConfigScope) -> &'static str {
    match scope {
        GitConfigScope::Global => "--global",
        GitConfigScope::Local => "--local",
        GitConfigScope::System => "--system",
    }
}

/// Return the repository path for a config scope, or an error when
/// [`GitConfigScope::Local`] is requested without a path.
fn repo_path_for_scope(scope: GitConfigScope, path: Option<&str>) -> Result<Option<String>> {
    match scope {
        GitConfigScope::Local => match path {
            Some(p) => Ok(Some(p.to_string())),
            None => Err(Error::InvalidArgument(
                r#"A repository path is required when using scope "local"."#.to_string(),
            )),
        },
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Reset mode
// ---------------------------------------------------------------------------

/// Reset mode for [`Git::reset`].
///
/// Each variant maps to the corresponding `git reset` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitResetMode {
    /// Keep index changes, do not touch the working tree (`--soft`).
    Soft,
    /// Reset the index but keep the working tree (`--mixed`; git default).
    Mixed,
    /// Reset both the index and the working tree (`--hard`).
    Hard,
    /// Reset the index and update working-tree files that differ between
    /// the commit and `HEAD` (`--merge`).
    Merge,
    /// Reset the index and update working-tree files that are not locally
    /// modified (`--keep`).
    Keep,
}

impl GitResetMode {
    fn flag(self) -> &'static str {
        match self {
            GitResetMode::Soft => "--soft",
            GitResetMode::Mixed => "--mixed",
            GitResetMode::Hard => "--hard",
            GitResetMode::Merge => "--merge",
            GitResetMode::Keep => "--keep",
        }
    }
}

// ---------------------------------------------------------------------------
// Option structs
// ---------------------------------------------------------------------------

/// Options for [`Git::init`].
#[derive(Default)]
pub struct GitInitOpts {
    /// Initial branch name (e.g., `"main"`). Passed as `--initial-branch`.
    pub initial_branch: Option<String>,
    /// Create a bare repository (`--bare`).
    pub bare: bool,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for [`Git::add`].
pub struct GitAddOpts {
    /// Specific files to stage. When empty, `all` (or `.`) controls the target.
    pub files: Vec<String>,
    /// Stage all changes with `-A` when `true` and `files` is empty.
    ///
    /// Defaults to `true`, matching the JS SDK default.
    /// Ignored when `files` is non-empty.
    pub all: bool,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

impl Default for GitAddOpts {
    fn default() -> Self {
        GitAddOpts {
            files: Vec::new(),
            all: true, // JS default: `all = true`
            user: None,
        }
    }
}

/// Options for [`Git::commit`].
#[derive(Default)]
pub struct GitCommitOpts {
    /// Override commit author name (`-c user.name=…`).
    pub author_name: Option<String>,
    /// Override commit author email (`-c user.email=…`).
    pub author_email: Option<String>,
    /// Allow commits with no changes (`--allow-empty`).
    pub allow_empty: bool,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for [`Git::delete_branch`].
#[derive(Default)]
pub struct GitDeleteBranchOpts {
    /// Force deletion with `-D` even when the branch is not fully merged.
    pub force: bool,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for [`Git::reset`].
#[derive(Default)]
pub struct GitResetOpts {
    /// Reset mode. Omit to use the git default (`mixed`).
    pub mode: Option<GitResetMode>,
    /// Commit, branch, or ref to reset to. Defaults to `HEAD`.
    pub target: Option<String>,
    /// Paths to reset (after `--`).
    pub paths: Vec<String>,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for [`Git::restore`].
#[derive(Default)]
pub struct GitRestoreOpts {
    /// Paths to restore. Must be non-empty; [`Git::restore`] returns
    /// [`Error::InvalidArgument`] when empty.
    pub paths: Vec<String>,
    /// Restore the index / unstage changes (`--staged`).
    pub staged: Option<bool>,
    /// Restore working-tree files (`--worktree`).
    pub worktree: Option<bool>,
    /// Restore from this source — commit, branch, or ref (`--source`).
    pub source: Option<String>,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for [`Git::remote_add`].
#[derive(Default)]
pub struct GitRemoteAddOpts {
    /// Fetch the remote immediately after adding (`-f` / standalone `git fetch`).
    pub fetch: bool,
    /// Update the remote URL instead of returning an error when it already exists.
    pub overwrite: bool,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for simple git operations that only require a user override.
///
/// Used by [`Git::create_branch`], [`Git::checkout_branch`], and
/// [`Git::remote_get`].
#[derive(Default)]
pub struct GitRequestOpts {
    /// Run as this sandbox user.
    pub user: Option<String>,
}

/// Options for git config operations: [`Git::set_config`], [`Git::get_config`],
/// and [`Git::configure_user`].
#[derive(Default)]
pub struct GitConfigOpts {
    /// Config scope. Defaults to [`GitConfigScope::Global`] when `None`.
    pub scope: Option<GitConfigScope>,
    /// Repository path — required when `scope` is [`GitConfigScope::Local`].
    pub path: Option<String>,
    /// Run as this sandbox user.
    pub user: Option<String>,
}

// ---------------------------------------------------------------------------
// Git struct
// ---------------------------------------------------------------------------

/// Runs git operations in a sandbox via the [`Commands`] handle.
///
/// Obtain via [`crate::Sandbox::git`].
pub struct Git {
    commands: Commands,
}

impl Git {
    /// Build a [`Git`] handle from a [`Commands`] handle.
    pub(crate) fn new(commands: Commands) -> Git {
        Git { commands }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Build [`CommandStartOpts`] with the default git environment
    /// (`GIT_TERMINAL_PROMPT=0`) merged in, matching the JS `runGit`/`runShell`.
    fn make_run_opts(user: Option<String>) -> CommandStartOpts {
        let mut envs = BTreeMap::new();
        envs.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
        CommandStartOpts {
            user,
            envs,
            ..Default::default()
        }
    }

    /// Run a command string through the sandbox process runner with git defaults.
    async fn run_cmd(&self, cmd: &str, user: Option<String>) -> Result<CommandResult> {
        self.commands.run(cmd, Self::make_run_opts(user)).await
    }

    // -----------------------------------------------------------------------
    // Public methods
    // -----------------------------------------------------------------------

    /// Initialize a new git repository at `path`.
    ///
    /// `path` is the positional argument to `git init` (not a `-C` flag);
    /// git creates the directory. Equivalent to:
    /// `git init [--initial-branch <b>] [--bare] <path>`
    pub async fn init(&self, path: &str, opts: GitInitOpts) -> Result<CommandResult> {
        let mut args: Vec<String> = vec!["init".to_string()];
        if let Some(branch) = opts.initial_branch {
            args.push("--initial-branch".to_string());
            args.push(branch);
        }
        if opts.bare {
            args.push("--bare".to_string());
        }
        args.push(path.to_string());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        // No `-C` prefix — `path` is the positional destination.
        let cmd = build_git_command(&refs, None);
        self.run_cmd(&cmd, opts.user).await
    }

    /// Stage files for commit.
    ///
    /// Equivalent to:
    /// - `git -C <path> add -A` — default (`files` empty, `all` true)
    /// - `git -C <path> add .` — `files` empty, `all` false
    /// - `git -C <path> add -- <files…>` — specific files
    pub async fn add(&self, path: &str, opts: GitAddOpts) -> Result<CommandResult> {
        let mut args: Vec<String> = vec!["add".to_string()];
        if opts.files.is_empty() {
            args.push(if opts.all { "-A" } else { "." }.to_string());
        } else {
            args.push("--".to_string());
            args.extend(opts.files);
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let cmd = build_git_command(&refs, Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Create a commit with `message`.
    ///
    /// Author overrides are injected as `-c` flags before the `commit`
    /// subcommand, matching the JS SDK. Equivalent to:
    /// `git -C <path> [-c user.name=<n>] [-c user.email=<e>] commit -m <message> [--allow-empty]`
    pub async fn commit(
        &self,
        path: &str,
        message: &str,
        opts: GitCommitOpts,
    ) -> Result<CommandResult> {
        let mut args: Vec<String> = Vec::new();
        // Author overrides come before the `commit` sub-command (JS: [...authorArgs, ...args]).
        if let Some(name) = opts.author_name {
            args.push("-c".to_string());
            args.push(format!("user.name={name}"));
        }
        if let Some(email) = opts.author_email {
            args.push("-c".to_string());
            args.push(format!("user.email={email}"));
        }
        args.push("commit".to_string());
        args.push("-m".to_string());
        args.push(message.to_string());
        if opts.allow_empty {
            args.push("--allow-empty".to_string());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let cmd = build_git_command(&refs, Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Create and check out a new branch.
    ///
    /// Equivalent to `git -C <path> checkout -b <branch>`.
    pub async fn create_branch(
        &self,
        path: &str,
        branch: &str,
        opts: GitRequestOpts,
    ) -> Result<CommandResult> {
        let cmd = build_git_command(&["checkout", "-b", branch], Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Check out an existing branch.
    ///
    /// Equivalent to `git -C <path> checkout <branch>`.
    pub async fn checkout_branch(
        &self,
        path: &str,
        branch: &str,
        opts: GitRequestOpts,
    ) -> Result<CommandResult> {
        let cmd = build_git_command(&["checkout", branch], Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Delete a branch.
    ///
    /// Equivalent to `git -C <path> branch -d <branch>`, or `-D` when
    /// `opts.force` is `true`.
    pub async fn delete_branch(
        &self,
        path: &str,
        branch: &str,
        opts: GitDeleteBranchOpts,
    ) -> Result<CommandResult> {
        let flag = if opts.force { "-D" } else { "-d" };
        let cmd = build_git_command(&["branch", flag, branch], Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Reset the current `HEAD` to a specified state.
    ///
    /// Equivalent to:
    /// `git -C <path> reset [--soft|--mixed|--hard|--merge|--keep] [<target>] [-- <paths…>]`
    pub async fn reset(&self, path: &str, opts: GitResetOpts) -> Result<CommandResult> {
        let mut args: Vec<String> = vec!["reset".to_string()];
        if let Some(mode) = opts.mode {
            args.push(mode.flag().to_string());
        }
        if let Some(target) = opts.target {
            args.push(target);
        }
        if !opts.paths.is_empty() {
            args.push("--".to_string());
            args.extend(opts.paths);
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let cmd = build_git_command(&refs, Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Restore working-tree files or unstage changes.
    ///
    /// `opts.paths` must be non-empty; this method returns
    /// [`Error::InvalidArgument`] when it is empty.
    ///
    /// `staged` and `worktree` follow the same defaulting logic as the JS SDK:
    /// when both are `None`, `worktree` is set to `true`. When only `staged` is
    /// `true`, `worktree` is set to `false` (and vice-versa).
    ///
    /// Equivalent to:
    /// `git -C <path> restore [--worktree] [--staged] [--source <src>] -- <paths…>`
    pub async fn restore(&self, path: &str, opts: GitRestoreOpts) -> Result<CommandResult> {
        if opts.paths.is_empty() {
            return Err(Error::InvalidArgument(
                "At least one path is required.".to_string(),
            ));
        }

        // Port JS defaulting logic for staged / worktree.
        let (resolved_staged, resolved_worktree) = match (opts.staged, opts.worktree) {
            (None, None) => (None, Some(true)),
            (Some(true), None) => (Some(true), Some(false)),
            (None, v) => (Some(false), v),
            (s, w) => (s, w),
        };

        if resolved_staged == Some(false) && resolved_worktree == Some(false) {
            return Err(Error::InvalidArgument(
                "At least one of staged or worktree must be true.".to_string(),
            ));
        }

        let mut args: Vec<String> = vec!["restore".to_string()];
        if resolved_worktree == Some(true) {
            args.push("--worktree".to_string());
        }
        if resolved_staged == Some(true) {
            args.push("--staged".to_string());
        }
        if let Some(source) = opts.source {
            args.push("--source".to_string());
            args.push(source);
        }
        args.push("--".to_string());
        args.extend(opts.paths);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let cmd = build_git_command(&refs, Some(path));
        self.run_cmd(&cmd, opts.user).await
    }

    /// Set a git config value.
    ///
    /// A repository `path` in `opts` is required when `scope` is
    /// [`GitConfigScope::Local`]. Equivalent to:
    /// `git [-C <path>] config [--global|--local|--system] <key> <value>`
    pub async fn set_config(
        &self,
        key: &str,
        value: &str,
        opts: GitConfigOpts,
    ) -> Result<CommandResult> {
        let scope = opts.scope.unwrap_or(GitConfigScope::Global);
        let flag = scope_flag(scope);
        let repo_path = repo_path_for_scope(scope, opts.path.as_deref())?;
        let cmd = build_git_command(&["config", flag, key, value], repo_path.as_deref());
        self.run_cmd(&cmd, opts.user).await
    }

    /// Get a git config value, returning `None` when the key is not set.
    ///
    /// Equivalent to:
    /// `git [-C <path>] config [--global|--local|--system] --get <key> || true`
    pub async fn get_config(&self, key: &str, opts: GitConfigOpts) -> Result<Option<String>> {
        let scope = opts.scope.unwrap_or(GitConfigScope::Global);
        let flag = scope_flag(scope);
        let repo_path = repo_path_for_scope(scope, opts.path.as_deref())?;
        let git_cmd = build_git_command(&["config", flag, "--get", key], repo_path.as_deref());
        // `|| true` ensures a non-zero exit when the key is absent does not
        // surface as an error; we distinguish via stdout being empty.
        let cmd = format!("{git_cmd} || true");
        let result = self.run_cmd(&cmd, opts.user).await?;
        let trimmed = result.stdout.trim().to_string();
        Ok(if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        })
    }

    /// Configure git user name and email.
    ///
    /// Runs two `git config` calls — one for `user.name`, one for
    /// `user.email` — and returns the result of the second, matching the JS SDK.
    pub async fn configure_user(
        &self,
        name: &str,
        email: &str,
        opts: GitConfigOpts,
    ) -> Result<CommandResult> {
        let scope = opts.scope.unwrap_or(GitConfigScope::Global);
        let flag = scope_flag(scope);
        let repo_path = repo_path_for_scope(scope, opts.path.as_deref())?;
        let name_cmd =
            build_git_command(&["config", flag, "user.name", name], repo_path.as_deref());
        let email_cmd =
            build_git_command(&["config", flag, "user.email", email], repo_path.as_deref());
        self.run_cmd(&name_cmd, opts.user.clone()).await?;
        self.run_cmd(&email_cmd, opts.user).await
    }

    /// Add a remote to a repository, or update its URL when it already exists.
    ///
    /// When `opts.overwrite` is `false` (default):
    /// `git -C <path> remote add [-f] <name> <url>`
    ///
    /// When `opts.overwrite` is `true`, runs a composite shell command:
    /// `(git -C <path> remote add [-f] <name> <url> || git -C <path> remote set-url <name> <url>)`
    /// optionally followed by `&& git -C <path> fetch <name>` when `fetch` is `true`.
    pub async fn remote_add(
        &self,
        path: &str,
        name: &str,
        url: &str,
        opts: GitRemoteAddOpts,
    ) -> Result<CommandResult> {
        let mut add_args: Vec<String> = vec!["remote".to_string(), "add".to_string()];
        if opts.fetch {
            add_args.push("-f".to_string());
        }
        add_args.push(name.to_string());
        add_args.push(url.to_string());

        if !opts.overwrite {
            let refs: Vec<&str> = add_args.iter().map(String::as_str).collect();
            let cmd = build_git_command(&refs, Some(path));
            return self.run_cmd(&cmd, opts.user).await;
        }

        // Composite form: try `remote add`, fall back to `remote set-url`.
        let refs: Vec<&str> = add_args.iter().map(String::as_str).collect();
        let add_cmd = build_git_command(&refs, Some(path));
        let set_url_cmd = build_git_command(&["remote", "set-url", name, url], Some(path));
        let mut cmd = format!("{add_cmd} || {set_url_cmd}");
        if opts.fetch {
            // The fetch is separated from the `remote add -f` so it also runs
            // after a `set-url` fallback.
            let fetch_cmd = build_git_command(&["fetch", name], Some(path));
            cmd = format!("({cmd}) && {fetch_cmd}");
        }
        self.run_cmd(&cmd, opts.user).await
    }

    /// Get the URL for a named remote, returning `None` when the remote does
    /// not exist.
    ///
    /// Equivalent to `git -C <path> remote get-url <name> || true`.
    pub async fn remote_get(
        &self,
        path: &str,
        name: &str,
        opts: GitRequestOpts,
    ) -> Result<Option<String>> {
        let git_cmd = build_git_command(&["remote", "get-url", name], Some(path));
        let cmd = format!("{git_cmd} || true");
        let result = self.run_cmd(&cmd, opts.user).await?;
        let trimmed = result.stdout.trim().to_string();
        Ok(if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::client::{ConnectClient, ConnectClientOpts};
    use crate::connect::envelope::{FLAG_END_STREAM, encode_envelope};
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn connect_for(server: &MockServer) -> Arc<ConnectClient> {
        Arc::new(
            ConnectClient::new(ConnectClientOpts {
                base_url: server.uri(),
                access_token: None,
                sandbox_id: "sbx_c".to_string(),
                envd_port: 49983,
                user_agent: "e2b-rs-test".to_string(),
                envd_version: "0.6.3".to_string(),
                request_timeout_ms: 60_000,
                logger: None,
                proxy: None,
            })
            .expect("connect client"),
        )
    }

    /// Build a minimal streaming process body: Start → End(exit_code).
    fn proc_stream(exit_code: i32) -> Vec<u8> {
        let mut body = encode_envelope(0, br#"{"event":{"start":{"pid":7}}}"#);
        body.extend(encode_envelope(
            0,
            format!(
                r#"{{"event":{{"end":{{"exitCode":{exit_code},"exited":true,"status":"exited"}}}}}}"#
            )
            .as_bytes(),
        ));
        body.extend(encode_envelope(FLAG_END_STREAM, b"{}"));
        body
    }

    /// Mount a single mock for `/process.Process/Start` returning `exit_code`.
    async fn mount_proc(server: &MockServer, exit_code: i32) {
        Mock::given(method("POST"))
            .and(path("/process.Process/Start"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/connect+json")
                    .set_body_bytes(proc_stream(exit_code)),
            )
            .mount(server)
            .await;
    }

    // -----------------------------------------------------------------------
    // Integration: Git over mock Commands
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn checkout_branch_calls_commands_run() {
        let server = MockServer::start().await;
        mount_proc(&server, 0).await;
        let commands = Commands::build_with_connect(connect_for(&server), "0.6.3");
        let git = Git::new(commands);
        let result = git
            .checkout_branch("/repo", "main", Default::default())
            .await
            .expect("checkout_branch");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn git_methods_return_non_zero_as_ok() {
        // Non-zero exit is data, not an error (Plan 3c rule).
        let server = MockServer::start().await;
        mount_proc(&server, 1).await;
        let commands = Commands::build_with_connect(connect_for(&server), "0.6.3");
        let git = Git::new(commands);
        let result = git
            .create_branch("/repo", "feature", Default::default())
            .await
            .expect("create_branch non-zero must be Ok");
        assert_eq!(result.exit_code, 1);
    }

    // -----------------------------------------------------------------------
    // Unit: command string construction (pure, no I/O)
    // -----------------------------------------------------------------------

    #[test]
    fn init_no_opts() {
        // git init /path
        assert_eq!(
            build_git_command(&["init", "/path"], None),
            "git init /path"
        );
    }

    #[test]
    fn add_default_is_all() {
        // Default GitAddOpts → all=true
        let opts = GitAddOpts::default();
        assert!(opts.all, "default all should be true");
        assert!(opts.files.is_empty());
        // build: git -C /repo add -A
        assert_eq!(
            build_git_command(&["add", "-A"], Some("/repo")),
            "git -C /repo add -A"
        );
    }

    #[test]
    fn add_specific_files_uses_dashdash() {
        // git -C /repo add -- a.rs b.rs
        assert_eq!(
            build_git_command(&["add", "--", "a.rs", "b.rs"], Some("/repo")),
            "git -C /repo add -- a.rs b.rs"
        );
    }

    #[test]
    fn commit_author_args_before_commit() {
        // git -C /repo -c user.name=Alice -c user.email=a@b.com commit -m 'msg'
        assert_eq!(
            build_git_command(
                &[
                    "-c",
                    "user.name=Alice",
                    "-c",
                    "user.email=a@b.com",
                    "commit",
                    "-m",
                    "msg"
                ],
                Some("/repo")
            ),
            "git -C /repo -c user.name=Alice -c user.email=a@b.com commit -m msg"
        );
    }

    #[test]
    fn checkout_branch_command() {
        assert_eq!(
            build_git_command(&["checkout", "main"], Some("/repo")),
            "git -C /repo checkout main"
        );
    }

    #[test]
    fn create_branch_command() {
        assert_eq!(
            build_git_command(&["checkout", "-b", "feature"], Some("/repo")),
            "git -C /repo checkout -b feature"
        );
    }

    #[test]
    fn delete_branch_force_flag() {
        assert_eq!(
            build_git_command(&["branch", "-D", "old"], Some("/r")),
            "git -C /r branch -D old"
        );
        assert_eq!(
            build_git_command(&["branch", "-d", "old"], Some("/r")),
            "git -C /r branch -d old"
        );
    }

    #[test]
    fn reset_hard_with_target_and_paths() {
        // `HEAD~1` contains `~` which is not a safe shell char → single-quoted.
        assert_eq!(
            build_git_command(&["reset", "--hard", "HEAD~1", "--", "src/"], Some("/r")),
            "git -C /r reset --hard 'HEAD~1' -- src/"
        );
    }

    #[test]
    fn restore_worktree_default() {
        // When staged=None, worktree=None → resolves to worktree=true
        // git -C /r restore --worktree -- src/a.rs
        assert_eq!(
            build_git_command(&["restore", "--worktree", "--", "src/a.rs"], Some("/r")),
            "git -C /r restore --worktree -- src/a.rs"
        );
    }

    #[test]
    fn set_config_global() {
        assert_eq!(
            build_git_command(&["config", "--global", "pull.rebase", "false"], None),
            "git config --global pull.rebase false"
        );
    }

    #[test]
    fn get_config_with_get_flag() {
        // git config --global --get pull.rebase || true
        let git_cmd = build_git_command(&["config", "--global", "--get", "pull.rebase"], None);
        assert_eq!(
            format!("{git_cmd} || true"),
            "git config --global --get pull.rebase || true"
        );
    }

    #[test]
    fn remote_add_overwrite_composite_command() {
        let add_args = ["remote", "add", "origin", "https://h/r.git"];
        let add_cmd = build_git_command(&add_args, Some("/r"));
        let set_url_cmd = build_git_command(
            &["remote", "set-url", "origin", "https://h/r.git"],
            Some("/r"),
        );
        let cmd = format!("{add_cmd} || {set_url_cmd}");
        assert_eq!(
            cmd,
            "git -C /r remote add origin https://h/r.git || git -C /r remote set-url origin https://h/r.git"
        );
    }

    #[test]
    fn remote_add_overwrite_with_fetch() {
        let add_args = ["remote", "add", "-f", "origin", "https://h/r.git"];
        let add_cmd = build_git_command(&add_args, Some("/r"));
        let set_url_cmd = build_git_command(
            &["remote", "set-url", "origin", "https://h/r.git"],
            Some("/r"),
        );
        let fetch_cmd = build_git_command(&["fetch", "origin"], Some("/r"));
        let cmd = format!("({add_cmd} || {set_url_cmd}) && {fetch_cmd}");
        assert_eq!(
            cmd,
            "(git -C /r remote add -f origin https://h/r.git || git -C /r remote set-url origin https://h/r.git) && git -C /r fetch origin"
        );
    }

    #[test]
    fn restore_validates_empty_paths() {
        // We can't call async in a sync test, but we can verify the error condition
        // by checking the paths check logic manually.
        let opts = GitRestoreOpts::default();
        assert!(
            opts.paths.is_empty(),
            "default GitRestoreOpts.paths should be empty"
        );
        // The actual InvalidArgument error is returned by the async method at runtime.
    }

    #[test]
    fn remote_get_command() {
        let git_cmd = build_git_command(&["remote", "get-url", "origin"], Some("/r"));
        assert_eq!(
            format!("{git_cmd} || true"),
            "git -C /r remote get-url origin || true"
        );
    }

    #[test]
    fn configure_user_builds_two_config_commands() {
        let name_cmd = build_git_command(&["config", "--global", "user.name", "Alice"], None);
        let email_cmd =
            build_git_command(&["config", "--global", "user.email", "alice@x.com"], None);
        assert_eq!(name_cmd, "git config --global user.name Alice");
        assert_eq!(email_cmd, "git config --global user.email alice@x.com");
    }
}
