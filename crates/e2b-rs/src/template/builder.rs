//! Template builder core — `Template`, `RegistryConfig`, and `BuildOptions`.
//!
//! This module provides the public `Template` struct and its builder methods
//! for constructing a template build request. It also defines
//! [`RegistryConfig`] for private registry authentication and [`BuildOptions`]
//! for per-build overrides.
//!
//! # Builder chain example
//!
//! ```rust
//! use e2b_rs::template::{Template, BuildOptions};
//! use e2b_rs::wait_for_timeout;
//!
//! let _template = Template::new()
//!     .from_base_image()
//!     .set_start_cmd("npm start", wait_for_timeout(1_000));
//! ```
//!
//! # Serialisation
//!
//! The `Template::serialize` and `Template::instruction_steps` methods are
//! `pub(crate)` and are called by the HTTP-build layer (Tasks 2–4) to produce
//! the wire-format body for the build API.

use crate::errors::Result;
use crate::template::ReadyCmd;
use crate::template::dockerfile::DockerfileAction;
use crate::template::types::{Instruction, InstructionType};

// ──────────────────────────────────────────────────────────────────────────────
// RegistryConfig
// ──────────────────────────────────────────────────────────────────────────────

/// Credentials for pulling the base image from a private container registry.
///
/// Stored on a [`Template`] instance and wired to the build request via the
/// `Template::with_registry` builder (Plan 5d).
///
/// # Secret handling
///
/// Credential fields are intentionally excluded from the `Debug` output —
/// printing `{:?}` shows `<redacted>` in place of any sensitive value.
/// This matches the convention used by [`crate::volume::VolumeOpts`].
#[derive(Clone)]
pub enum RegistryConfig {
    /// Amazon ECR credentials.
    Aws {
        /// AWS Access Key ID.
        access_key_id: String,
        /// AWS Secret Access Key.
        secret_access_key: String,
        /// AWS region where the ECR registry is located (e.g. `"us-east-1"`).
        region: String,
    },
    /// Google Container Registry / Artifact Registry credentials.
    Gcp {
        /// Service-account JSON key (as a raw JSON string).
        service_account_json: String,
    },
    /// Generic Docker Hub / OCI registry credentials.
    General {
        /// Registry username.
        username: String,
        /// Registry password or personal access token.
        password: String,
    },
}

impl std::fmt::Debug for RegistryConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aws { region, .. } => f
                .debug_struct("RegistryConfig::Aws")
                .field("access_key_id", &"<redacted>")
                .field("secret_access_key", &"<redacted>")
                .field("region", region)
                .finish(),
            Self::Gcp { .. } => f
                .debug_struct("RegistryConfig::Gcp")
                .field("service_account_json", &"<redacted>")
                .finish(),
            Self::General { username, .. } => f
                .debug_struct("RegistryConfig::General")
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
        }
    }
}

impl RegistryConfig {
    /// Converts this [`RegistryConfig`] into the wire-format
    /// [`crate::api::schema::FromImageRegistry`] enum.
    ///
    /// Used internally by [`Template::serialize`] to build the API request
    /// body. The generated type is never exposed in the public API.
    pub(crate) fn to_wire(&self) -> crate::api::schema::FromImageRegistry {
        use crate::api::schema::{
            AwsRegistry, AwsRegistryType, FromImageRegistry, GcpRegistry, GcpRegistryType,
            GeneralRegistry, GeneralRegistryType,
        };

        match self {
            Self::Aws {
                access_key_id,
                secret_access_key,
                region,
            } => FromImageRegistry::AwsRegistry(AwsRegistry {
                aws_access_key_id: access_key_id.clone(),
                aws_secret_access_key: secret_access_key.clone(),
                aws_region: region.clone(),
                type_: AwsRegistryType::Aws,
            }),
            Self::Gcp {
                service_account_json,
            } => FromImageRegistry::GcpRegistry(GcpRegistry {
                service_account_json: service_account_json.clone(),
                type_: GcpRegistryType::Gcp,
            }),
            Self::General { username, password } => {
                FromImageRegistry::GeneralRegistry(GeneralRegistry {
                    username: username.clone(),
                    password: password.clone(),
                    type_: GeneralRegistryType::Registry,
                })
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// BuildOptions
// ──────────────────────────────────────────────────────────────────────────────

/// Per-build overrides for a single template build invocation.
///
/// All fields are optional; unset values fall back to the environment-variable
/// resolution performed by [`crate::ConnectionConfig`].
///
/// # Secret handling
///
/// The `api_key` field is excluded from the `Debug` output — it is shown as
/// `<redacted>` to prevent accidental secret leakage in logs.
#[derive(Default, Clone)]
pub struct BuildOptions {
    /// Override the number of vCPUs for the build sandbox.
    pub cpu_count: Option<u32>,
    /// Override the RAM for the build sandbox, in MiB.
    pub memory_mb: Option<u32>,
    /// If `true`, bypass the build cache and force all steps to run.
    pub skip_cache: bool,
    /// Per-request HTTP timeout in milliseconds.
    pub request_timeout_ms: Option<u64>,
    /// Override the E2B API key for this build.
    pub api_key: Option<String>,
    /// Override the E2B domain for this build (e.g. `"e2b.app"`).
    pub domain: Option<String>,
    /// Override the API base URL for this build.
    pub api_url: Option<String>,
}

impl std::fmt::Debug for BuildOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildOptions")
            .field("cpu_count", &self.cpu_count)
            .field("memory_mb", &self.memory_mb)
            .field("skip_cache", &self.skip_cache)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field(
                "api_key",
                if self.api_key.is_some() {
                    &"<redacted>"
                } else {
                    &"None"
                },
            )
            .field("domain", &self.domain)
            .field("api_url", &self.api_url)
            .finish()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Template
// ──────────────────────────────────────────────────────────────────────────────

/// A template build specification.
///
/// Constructed via [`Template::new`] (or `Default::default()`) and configured
/// through a fluent builder chain. The builder methods consume `self` and
/// return a new `Template`, enabling method chaining without `&mut self`:
///
/// ```rust
/// use e2b_rs::template::Template;
/// use e2b_rs::wait_for_timeout;
///
/// let template = Template::new()
///     .from_image("node:20")
///     .set_start_cmd("npm start", wait_for_timeout(5_000))
///     .skip_cache();
/// ```
///
/// # Serialisation
///
/// Call `Template::instruction_steps` to convert accumulated instructions to
/// the wire type, then pass the result to `Template::serialize` to build the
/// full `TemplateBuildStartV2` request body. These methods are `pub(crate)`
/// and are called internally by the HTTP build layer.
#[derive(Clone, Default)]
pub struct Template {
    /// Base Docker image (e.g. `"node:20"`). Set by [`Template::from_image`].
    pub(crate) base_image: Option<String>,
    /// Base E2B template ID or alias. Set by [`Template::from_template`].
    pub(crate) base_template: Option<String>,
    /// Optional private registry credentials for pulling the base image.
    pub(crate) registry_config: Option<RegistryConfig>,
    /// Ordered list of build instructions accumulated by the builder methods.
    pub(crate) instructions: Vec<Instruction>,
    /// Command to start the sandbox after the build completes.
    pub(crate) start_cmd: Option<String>,
    /// Ready-check command that signals the sandbox is accepting traffic.
    pub(crate) ready_cmd: Option<String>,
    /// When `true`, bypass the build cache for the entire template
    /// (corresponds to [`Template::skip_cache`]).
    pub(crate) force: bool,
    /// CPU count override for the build sandbox.
    pub(crate) cpu_count: Option<u32>,
    /// Memory override for the build sandbox, in MiB.
    pub(crate) memory_mb: Option<u32>,
}

impl Template {
    /// Create a new, empty template builder.
    ///
    /// Equivalent to `Template::default()`. Use the builder methods to
    /// configure the template before calling `Template::serialize`.
    pub fn new() -> Self {
        Self::default()
    }

    // ── Base-image / base-template entry points ───────────────────────────────

    /// Use a specific Docker image as the base for this template.
    ///
    /// Clears any previously set `base_template`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use e2b_rs::template::Template;
    /// let t = Template::new().from_image("node:20");
    /// ```
    pub fn from_image(mut self, base_image: &str) -> Self {
        self.base_image = Some(base_image.to_string());
        self.base_template = None;
        self
    }

    /// Use the default E2B base image (`"e2bdev/base"`) as the base for this
    /// template.
    ///
    /// Equivalent to `from_image("e2bdev/base")`.
    pub fn from_base_image(self) -> Self {
        self.from_image("e2bdev/base")
    }

    /// Use an existing E2B template as the base for this template.
    ///
    /// `id_or_alias` may be either a numeric template ID or a human-readable
    /// alias. Clears any previously set `base_image`.
    pub fn from_template(mut self, id_or_alias: &str) -> Self {
        self.base_template = Some(id_or_alias.to_string());
        self.base_image = None;
        self
    }

    /// Parse a Dockerfile and configure this template from it.
    ///
    /// The content string must be the raw Dockerfile text (not a path). The
    /// parser is a minimal port of the JavaScript SDK's `dockerfileParser.ts`;
    /// only the subset of Dockerfile keywords that E2B supports are handled —
    /// `FROM`, `RUN`, `COPY`, `ADD`, `WORKDIR`, `USER`, `ENV`, `ARG`, `CMD`,
    /// `ENTRYPOINT`. Unknown keywords are silently ignored.
    ///
    /// Multi-stage Dockerfiles are **not** supported and return an error.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Template`] when:
    /// - The Dockerfile contains more than one `FROM` instruction.
    /// - The Dockerfile contains no `FROM` instruction.
    ///
    /// # Example
    ///
    /// ```rust
    /// use e2b_rs::template::Template;
    ///
    /// # fn main() -> e2b_rs::Result<()> {
    /// let t = Template::new()
    ///     .from_dockerfile("FROM node:20\nRUN npm install\n")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_dockerfile(mut self, content: &str) -> Result<Self> {
        let result = crate::template::dockerfile::parse_dockerfile(content)?;
        self.base_image = Some(result.base_image);
        self.base_template = None;

        for action in result.actions {
            self = apply_action(self, action);
        }

        Ok(self)
    }

    // ── Start-command / ready-command setters ─────────────────────────────────

    /// Set the sandbox start command and the corresponding ready-check.
    ///
    /// The `ready` argument is any [`ReadyCmd`] produced by the free functions
    /// in [`crate::template`] (e.g. [`crate::wait_for_port`],
    /// [`crate::wait_for_timeout`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// use e2b_rs::template::Template;
    /// use e2b_rs::wait_for_port;
    ///
    /// let t = Template::new()
    ///     .from_base_image()
    ///     .set_start_cmd("node server.js", wait_for_port(3000));
    /// ```
    pub fn set_start_cmd(mut self, cmd: &str, ready: ReadyCmd) -> Self {
        self.start_cmd = Some(cmd.to_string());
        self.ready_cmd = Some(ready.cmd().to_string());
        self
    }

    /// Override only the ready-check command without changing the start
    /// command.
    pub fn set_ready_cmd(mut self, ready: ReadyCmd) -> Self {
        self.ready_cmd = Some(ready.cmd().to_string());
        self
    }

    /// Bypass the build cache for the entire template.
    ///
    /// Sets the `force` flag on the build request, causing all steps to be
    /// re-executed even if the cache would otherwise mark them as up-to-date.
    pub fn skip_cache(mut self) -> Self {
        self.force = true;
        self
    }

    // ── Internal serialisation helpers ────────────────────────────────────────

    /// Map each accumulated [`Instruction`] to a generated
    /// [`crate::api::schema::TemplateStep`] wire type.
    ///
    /// The mapping is:
    ///
    /// | [`InstructionType`] | `TemplateStep.type_` |
    /// |---|---|
    /// | [`InstructionType::Copy`] | `"COPY"` |
    /// | [`InstructionType::Env`] | `"ENV"` |
    /// | [`InstructionType::Run`] | `"RUN"` |
    /// | [`InstructionType::Workdir`] | `"WORKDIR"` |
    /// | [`InstructionType::User`] | `"USER"` |
    ///
    /// Called by tests and by the HTTP build layer before passing the result to
    /// [`Template::serialize`].
    // Used in tests; `instruction_steps_from` is the production path.
    #[allow(dead_code)]
    pub(crate) fn instruction_steps(&self) -> Vec<crate::api::schema::TemplateStep> {
        self.instructions
            .iter()
            .map(|instr| crate::api::schema::TemplateStep {
                type_: instruction_type_str(instr.instruction_type),
                args: instr.args.clone(),
                files_hash: instr.files_hash.clone(),
                force: instr.force,
            })
            .collect()
    }

    /// Map a slice of [`Instruction`]s (typically hash-enriched) to
    /// [`crate::api::schema::TemplateStep`] wire types.
    ///
    /// This is like [`Template::instruction_steps`] but operates on an
    /// arbitrary instruction slice rather than `self.instructions`. The build
    /// layer calls this with the hash-filled instructions returned by
    /// [`crate::template::build_api::instructions_with_hashes`].
    pub(crate) fn instruction_steps_from(
        instructions: &[Instruction],
    ) -> Vec<crate::api::schema::TemplateStep> {
        instructions
            .iter()
            .map(|instr| crate::api::schema::TemplateStep {
                type_: instruction_type_str(instr.instruction_type),
                args: instr.args.clone(),
                files_hash: instr.files_hash.clone(),
                force: instr.force,
            })
            .collect()
    }

    /// Serialize the template into the wire-format
    /// [`crate::api::schema::TemplateBuildStartV2`] body.
    ///
    /// `steps` should be the result of [`Template::instruction_steps`] (or an
    /// enriched version with `files_hash` populated by the upload layer).
    ///
    /// This is a port of `serialize` in the JavaScript SDK (`index.ts:1301`).
    pub(crate) fn serialize(
        &self,
        steps: Vec<crate::api::schema::TemplateStep>,
    ) -> crate::api::schema::TemplateBuildStartV2 {
        crate::api::schema::TemplateBuildStartV2 {
            start_cmd: self.start_cmd.clone(),
            ready_cmd: self.ready_cmd.clone(),
            steps,
            force: self.force,
            from_image: self.base_image.clone(),
            from_template: self.base_template.clone(),
            from_image_registry: self.registry_config.as_ref().map(|r| r.to_wire()),
        }
    }

    /// Build this template and return a [`crate::template::handle::BuildHandle`]
    /// for streaming log entries.
    ///
    /// Consumes `self`. The returned handle allows callers to stream log entries
    /// via [`crate::template::handle::BuildHandle::next`] and to await build
    /// completion via [`crate::template::handle::BuildHandle::wait`].
    ///
    /// # Build context
    ///
    /// Files are uploaded from the **current working directory**
    /// (`std::env::current_dir()`). A configurable context path is a future
    /// carry-forward.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is missing or invalid, if any HTTP call
    /// fails, or if the build context cannot be read.
    pub async fn build(
        self,
        name: &str,
        opts: BuildOptions,
    ) -> crate::errors::Result<crate::template::handle::BuildHandle> {
        use std::sync::Arc;

        use crate::api::client::ApiClient;
        use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
        use crate::template::build_api::{
            instructions_with_hashes, request_build, trigger_build, upload_build_context,
        };
        use crate::template::handle::{BuildHandle, wait_for_build_finish};
        use tokio::sync::{mpsc, oneshot};

        // 1. Resolve ConnectionConfig + ApiClient.
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: opts.api_key.clone(),
            domain: opts.domain.clone(),
            api_url: opts.api_url.clone(),
            request_timeout_ms: opts.request_timeout_ms,
            ..Default::default()
        });
        let api = Arc::new(ApiClient::new(&config, true)?);

        // 2. Parse "name:tag" form.
        let (template_name, extra_tag) = normalize_build_name(name);
        let tags: Vec<String> = extra_tag.into_iter().collect();

        // 3. Request a build slot from the control plane.
        let resp = request_build(
            &api,
            &template_name,
            &tags,
            opts.cpu_count.or(self.cpu_count),
            opts.memory_mb.or(self.memory_mb),
        )
        .await?;

        // 4. Resolve build context directory (default: cwd).
        let context = std::env::current_dir().map_err(|e| {
            crate::errors::Error::Internal(format!("failed to get current directory: {e}"))
        })?;

        // 5. Populate `files_hash` on COPY instructions.
        let instrs = instructions_with_hashes(&self.instructions, &context)?;

        // 6. Upload file-context archives for instructions not already cached.
        let http = reqwest::Client::new();
        upload_build_context(&api, &http, &resp.template_id, &instrs, &context).await?;

        // 7. Trigger the build with hash-filled steps; honour skip_cache override.
        let steps = Template::instruction_steps_from(&instrs);
        let mut body = self.serialize(steps);
        body.force = opts.skip_cache || body.force;
        trigger_build(&api, &resp.template_id, &resp.build_id, &body).await?;

        // 8. Construct the BuildInfo returned to the caller.
        let info = crate::template::types::BuildInfo {
            template_id: resp.template_id.clone(),
            build_id: resp.build_id.clone(),
            name: Some(template_name.clone()),
            alias: None,
            tags: tags.clone(),
        };

        // 9. Spawn poll task; wire channels; return handle.
        let (tx_logs, rx_logs) = mpsc::channel::<crate::template::log::LogEntry>(128);
        let (tx_result, rx_result) =
            oneshot::channel::<crate::errors::Result<crate::template::types::BuildInfo>>();
        let info_clone = info.clone();
        let api_arc = Arc::clone(&api);
        let tid = resp.template_id.clone();
        let bid = resp.build_id.clone();
        let task = tokio::spawn(async move {
            let r = wait_for_build_finish(api_arc, tid, bid, 200, tx_logs)
                .await
                .map(|()| info_clone);
            let _ = tx_result.send(r);
        });

        Ok(BuildHandle::new(rx_logs, rx_result, task, info))
    }

    /// Build this template in the background and return immediately with
    /// [`crate::template::types::BuildInfo`] containing the template and build
    /// identifiers.
    ///
    /// Unlike [`Template::build`], this method does **not** wait for the build
    /// to complete or stream log entries. The build continues asynchronously on
    /// E2B infrastructure.
    ///
    /// # Build context
    ///
    /// Files are uploaded from the **current working directory**. See
    /// [`Template::build`] for details.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is missing or invalid, if any HTTP call
    /// fails, or if the build context cannot be read.
    pub async fn build_in_background(
        self,
        name: &str,
        opts: BuildOptions,
    ) -> crate::errors::Result<crate::template::types::BuildInfo> {
        use std::sync::Arc;

        use crate::api::client::ApiClient;
        use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
        use crate::template::build_api::{
            instructions_with_hashes, request_build, trigger_build, upload_build_context,
        };

        // 1. Resolve ConnectionConfig + ApiClient.
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: opts.api_key.clone(),
            domain: opts.domain.clone(),
            api_url: opts.api_url.clone(),
            request_timeout_ms: opts.request_timeout_ms,
            ..Default::default()
        });
        let api = Arc::new(ApiClient::new(&config, true)?);

        // 2. Parse "name:tag" form.
        let (template_name, extra_tag) = normalize_build_name(name);
        let tags: Vec<String> = extra_tag.into_iter().collect();

        // 3. Request a build slot from the control plane.
        let resp = request_build(
            &api,
            &template_name,
            &tags,
            opts.cpu_count.or(self.cpu_count),
            opts.memory_mb.or(self.memory_mb),
        )
        .await?;

        // 4. Resolve build context directory (default: cwd).
        let context = std::env::current_dir().map_err(|e| {
            crate::errors::Error::Internal(format!("failed to get current directory: {e}"))
        })?;

        // 5. Populate `files_hash` on COPY instructions.
        let instrs = instructions_with_hashes(&self.instructions, &context)?;

        // 6. Upload file-context archives for instructions not already cached.
        let http = reqwest::Client::new();
        upload_build_context(&api, &http, &resp.template_id, &instrs, &context).await?;

        // 7. Trigger the build; honour skip_cache override.
        let steps = Template::instruction_steps_from(&instrs);
        let mut body = self.serialize(steps);
        body.force = opts.skip_cache || body.force;
        trigger_build(&api, &resp.template_id, &resp.build_id, &body).await?;

        // 8. Return BuildInfo immediately — no poll task is spawned.
        Ok(crate::template::types::BuildInfo {
            template_id: resp.template_id,
            build_id: resp.build_id,
            name: Some(template_name),
            alias: None,
            tags,
        })
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Apply a single [`DockerfileAction`] to the template, returning the updated
/// template.
///
/// The mapping follows the JavaScript SDK's `parseDockerfile` → builder method
/// call pattern:
///
/// | [`DockerfileAction`] | Effect |
/// |---|---|
/// | `SetUser(u)` | push `USER u` instruction |
/// | `SetWorkdir(p)` | push `WORKDIR p` instruction |
/// | `Copy { src, dest, user }` | push `COPY src dest [user] [""]` instruction |
/// | `RunCmd(cmd)` | push `RUN cmd` instruction |
/// | `SetEnvs(map)` | push `ENV k1 v1 k2 v2 …` instruction |
/// | `SetStartCmd { cmd, ready }` | set `start_cmd` + `ready_cmd` |
fn apply_action(mut template: Template, action: DockerfileAction) -> Template {
    match action {
        DockerfileAction::SetUser(user) => {
            template.instructions.push(Instruction {
                instruction_type: InstructionType::User,
                args: vec![user],
                force: false,
                force_upload: None,
                files_hash: None,
                resolve_symlinks: false,
            });
        }
        DockerfileAction::SetWorkdir(path) => {
            template.instructions.push(Instruction {
                instruction_type: InstructionType::Workdir,
                args: vec![path],
                force: false,
                force_upload: None,
                files_hash: None,
                resolve_symlinks: false,
            });
        }
        DockerfileAction::Copy { src, dest, user } => {
            // Mirror the JS SDK copy() args format:
            // [src, dest, user || '', mode || '']
            template.instructions.push(Instruction {
                instruction_type: InstructionType::Copy,
                args: vec![src, dest, user.unwrap_or_default(), String::new()],
                force: false,
                force_upload: None,
                files_hash: None,
                resolve_symlinks: false,
            });
        }
        DockerfileAction::RunCmd(cmd) => {
            template.instructions.push(Instruction {
                instruction_type: InstructionType::Run,
                args: vec![cmd],
                force: false,
                force_upload: None,
                files_hash: None,
                resolve_symlinks: false,
            });
        }
        DockerfileAction::SetEnvs(map) => {
            if !map.is_empty() {
                // Mirror JS setEnvs: flatMap(([k, v]) => [k, v])
                let args: Vec<String> = map.into_iter().flat_map(|(k, v)| [k, v]).collect();
                template.instructions.push(Instruction {
                    instruction_type: InstructionType::Env,
                    args,
                    force: false,
                    force_upload: None,
                    files_hash: None,
                    resolve_symlinks: false,
                });
            }
        }
        DockerfileAction::SetStartCmd { cmd, ready } => {
            template.start_cmd = Some(cmd);
            template.ready_cmd = Some(ready.cmd().to_string());
        }
    }
    template
}

/// Split a `"name"` or `"name:tag"` string into `(name, Option<tag>)`.
///
/// If the name contains a colon, everything before the first colon is the
/// base name and everything after is treated as a tag for the build request.
/// Names without a colon return `(name, None)`.
fn normalize_build_name(raw: &str) -> (String, Option<String>) {
    match raw.split_once(':') {
        Some((n, t)) if !n.is_empty() && !t.is_empty() => (n.to_string(), Some(t.to_string())),
        _ => (raw.to_string(), None),
    }
}

/// Convert an [`InstructionType`] to its wire-format string representation.
fn instruction_type_str(ty: InstructionType) -> String {
    match ty {
        InstructionType::Copy => "COPY".to_string(),
        InstructionType::Env => "ENV".to_string(),
        InstructionType::Run => "RUN".to_string(),
        InstructionType::Workdir => "WORKDIR".to_string(),
        InstructionType::User => "USER".to_string(),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::api::schema::FromImageRegistry;
    use crate::wait_for_timeout;

    // ── from_dockerfile → base_image + instructions ───────────────────────────

    #[test]
    fn from_dockerfile_sets_base_and_instructions() {
        let dockerfile = "\
FROM node:20
RUN npm install
COPY . /app
WORKDIR /app
USER myuser
ENV FOO=bar
CMD npm start
";
        let template = Template::new()
            .from_dockerfile(dockerfile)
            .expect("valid Dockerfile");

        // base_image extracted from FROM
        assert_eq!(template.base_image.as_deref(), Some("node:20"));

        // start_cmd from CMD
        assert_eq!(template.start_cmd.as_deref(), Some("npm start"));
        // ready_cmd from CMD default (20s)
        assert_eq!(template.ready_cmd.as_deref(), Some("sleep 20"));

        // Check instruction types in order:
        // Docker defaults: USER root, WORKDIR /
        // RUN, COPY, WORKDIR, USER, ENV
        // No E2B defaults because USER + WORKDIR were set
        let types: Vec<InstructionType> = template
            .instructions
            .iter()
            .map(|i| i.instruction_type)
            .collect();

        assert_eq!(
            types,
            vec![
                InstructionType::User,    // root (docker default)
                InstructionType::Workdir, // / (docker default)
                InstructionType::Run,     // npm install
                InstructionType::Copy,    // . /app
                InstructionType::Workdir, // /app
                InstructionType::User,    // myuser
                InstructionType::Env,     // FOO=bar
            ]
        );

        // Verify COPY args: [src, dest, user_or_empty, mode_or_empty]
        let copy = template
            .instructions
            .iter()
            .find(|i| i.instruction_type == InstructionType::Copy)
            .expect("expected Copy instruction");
        assert_eq!(copy.args[0], ".");
        assert_eq!(copy.args[1], "/app");
        assert_eq!(copy.args[2], ""); // no --chown

        // Verify ENV args: interleaved [k, v]
        let env_instr = template
            .instructions
            .iter()
            .find(|i| i.instruction_type == InstructionType::Env)
            .expect("expected Env instruction");
        assert_eq!(env_instr.args, vec!["FOO", "bar"]);
    }

    // ── serialize → TemplateBuildStartV2 mapping ──────────────────────────────

    #[test]
    fn serialize_maps_from_image_and_steps() {
        let template = Template::new()
            .from_image("node:20")
            .set_start_cmd("npm start", wait_for_timeout(1_000))
            .skip_cache();

        let steps = template.instruction_steps();
        let body = template.serialize(steps);

        assert_eq!(body.from_image.as_deref(), Some("node:20"));
        assert_eq!(body.start_cmd.as_deref(), Some("npm start"));
        assert!(body.force, "skip_cache should set force=true");
        assert_eq!(body.from_template, None);
        assert!(body.from_image_registry.is_none());
        // No instructions were added directly, so steps should be empty
        assert!(body.steps.is_empty());
    }

    // ── RegistryConfig::to_wire — all three variants ─────────────────────────

    #[test]
    fn registry_to_wire_aws() {
        let cfg = RegistryConfig::Aws {
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            region: "us-east-1".to_string(),
        };
        let wire = cfg.to_wire();
        match wire {
            FromImageRegistry::AwsRegistry(aws) => {
                assert_eq!(aws.aws_access_key_id, "AKID");
                assert_eq!(aws.aws_secret_access_key, "SECRET");
                assert_eq!(aws.aws_region, "us-east-1");
                assert_eq!(
                    aws.type_,
                    crate::api::schema::AwsRegistryType::Aws,
                    "type discriminator must be Aws"
                );
            }
            other => panic!("expected AwsRegistry, got {other:?}"),
        }
    }

    #[test]
    fn registry_to_wire_gcp() {
        let cfg = RegistryConfig::Gcp {
            service_account_json: r#"{"type":"service_account"}"#.to_string(),
        };
        let wire = cfg.to_wire();
        match wire {
            FromImageRegistry::GcpRegistry(gcp) => {
                assert_eq!(gcp.service_account_json, r#"{"type":"service_account"}"#);
                assert_eq!(
                    gcp.type_,
                    crate::api::schema::GcpRegistryType::Gcp,
                    "type discriminator must be Gcp"
                );
            }
            other => panic!("expected GcpRegistry, got {other:?}"),
        }
    }

    #[test]
    fn registry_to_wire_general() {
        let cfg = RegistryConfig::General {
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        let wire = cfg.to_wire();
        match wire {
            FromImageRegistry::GeneralRegistry(general) => {
                assert_eq!(general.username, "user");
                assert_eq!(general.password, "pass");
                assert_eq!(
                    general.type_,
                    crate::api::schema::GeneralRegistryType::Registry,
                    "type discriminator must be Registry"
                );
            }
            other => panic!("expected GeneralRegistry, got {other:?}"),
        }
    }

    // ── Secret-redaction tests ────────────────────────────────────────────────

    #[test]
    fn registry_config_debug_redacts_secrets() {
        let aws = RegistryConfig::Aws {
            access_key_id: "MY_ACCESS_KEY_ID".to_string(),
            secret_access_key: "MY_SECRET_KEY".to_string(),
            region: "eu-west-1".to_string(),
        };
        let debug_str = format!("{aws:?}");
        assert!(
            !debug_str.contains("MY_ACCESS_KEY_ID"),
            "access_key_id must not appear in Debug: {debug_str}"
        );
        assert!(
            !debug_str.contains("MY_SECRET_KEY"),
            "secret_access_key must not appear in Debug: {debug_str}"
        );
        assert!(
            debug_str.contains("<redacted>"),
            "Debug must contain '<redacted>': {debug_str}"
        );
        // Non-secret fields should still appear
        assert!(
            debug_str.contains("eu-west-1"),
            "region should appear in Debug: {debug_str}"
        );

        let gcp = RegistryConfig::Gcp {
            service_account_json: "SUPER_SECRET_JSON".to_string(),
        };
        let gcp_debug = format!("{gcp:?}");
        assert!(
            !gcp_debug.contains("SUPER_SECRET_JSON"),
            "service_account_json must not appear in Debug: {gcp_debug}"
        );

        let general = RegistryConfig::General {
            username: "myuser".to_string(),
            password: "MY_PASSWORD".to_string(),
        };
        let gen_debug = format!("{general:?}");
        assert!(
            !gen_debug.contains("MY_PASSWORD"),
            "password must not appear in Debug: {gen_debug}"
        );
        // username is not a secret
        assert!(
            gen_debug.contains("myuser"),
            "username should appear in Debug: {gen_debug}"
        );
    }

    #[test]
    fn build_options_debug_redacts_api_key() {
        let opts = BuildOptions {
            api_key: Some("e2b_super_secret_key".to_string()),
            domain: Some("e2b.app".to_string()),
            ..Default::default()
        };
        let debug_str = format!("{opts:?}");
        assert!(
            !debug_str.contains("e2b_super_secret_key"),
            "api_key must not appear in Debug: {debug_str}"
        );
        assert!(
            debug_str.contains("<redacted>"),
            "Debug must contain '<redacted>': {debug_str}"
        );
        assert!(
            debug_str.contains("e2b.app"),
            "domain should appear in Debug: {debug_str}"
        );
    }

    // ── instruction_steps maps types to strings correctly ─────────────────────

    #[test]
    fn instruction_steps_type_strings() {
        let template = Template::new()
            .from_dockerfile("FROM scratch\nRUN echo hi\nCOPY . /app\n")
            .expect("valid Dockerfile");

        let steps = template.instruction_steps();
        // Collect type_ strings
        let type_strings: Vec<&str> = steps.iter().map(|s| s.type_.as_str()).collect();

        // We should have USER, WORKDIR (docker defaults), RUN, COPY,
        // USER, WORKDIR (e2b defaults)
        assert!(
            type_strings.contains(&"RUN"),
            "should contain RUN: {type_strings:?}"
        );
        assert!(
            type_strings.contains(&"COPY"),
            "should contain COPY: {type_strings:?}"
        );
        assert!(
            type_strings.contains(&"USER"),
            "should contain USER: {type_strings:?}"
        );
        assert!(
            type_strings.contains(&"WORKDIR"),
            "should contain WORKDIR: {type_strings:?}"
        );

        // All strings must be from the known set
        for s in &type_strings {
            assert!(
                ["COPY", "RUN", "ENV", "WORKDIR", "USER"].contains(s),
                "unexpected type string: {s}"
            );
        }
    }

    // ── builder entry-point smoke tests ──────────────────────────────────────

    #[test]
    fn from_base_image_uses_default() {
        let t = Template::new().from_base_image();
        assert_eq!(t.base_image.as_deref(), Some("e2bdev/base"));
    }

    #[test]
    fn from_template_clears_base_image() {
        let t = Template::new()
            .from_image("node:20")
            .from_template("base-sandbox");
        assert_eq!(t.base_template.as_deref(), Some("base-sandbox"));
        assert_eq!(t.base_image, None);
    }

    #[test]
    fn skip_cache_sets_force() {
        let t = Template::new().from_base_image().skip_cache();
        assert!(t.force);
    }

    #[test]
    fn serialize_includes_from_image_registry() {
        let t = Template::new().from_base_image();
        // Manually set registry_config to test the round-trip
        let mut t = t;
        t.registry_config = Some(RegistryConfig::General {
            username: "u".to_string(),
            password: "p".to_string(),
        });
        let steps = t.instruction_steps();
        let body = t.serialize(steps);
        assert!(body.from_image_registry.is_some());
    }

    // ── BTreeMap ordering: SetEnvs uses BTreeMap so args are sorted ──────────

    #[test]
    fn set_envs_interleaved_args_are_sorted() {
        // Build a map via BTreeMap directly (parse_dockerfile uses BTreeMap too)
        let mut map = BTreeMap::new();
        map.insert("Z".to_string(), "last".to_string());
        map.insert("A".to_string(), "first".to_string());

        let mut t = Template::new().from_base_image();
        let action = DockerfileAction::SetEnvs(map);
        t = apply_action(t, action);

        // Find the ENV instruction
        let env = t
            .instructions
            .iter()
            .find(|i| i.instruction_type == InstructionType::Env)
            .expect("expected Env instruction");

        // BTreeMap iteration is sorted ascending
        assert_eq!(env.args, vec!["A", "first", "Z", "last"]);
    }

    // ── normalize_build_name ──────────────────────────────────────────────────

    #[test]
    fn normalize_build_name_splits_tag() {
        assert_eq!(
            normalize_build_name("my-env:v1"),
            ("my-env".to_string(), Some("v1".to_string()))
        );
        // No colon → no tag.
        assert_eq!(normalize_build_name("my-env"), ("my-env".to_string(), None));
        // Empty tag after colon → treated as no tag (whole string is the name).
        assert_eq!(
            normalize_build_name("my-env:"),
            ("my-env:".to_string(), None)
        );
    }

    // ── build_in_background — no poll task ─────────────────────────────────────

    /// `build_in_background` must request a slot, trigger the build, and return
    /// the [`BuildInfo`] immediately WITHOUT polling the build-status endpoint.
    #[tokio::test]
    async fn build_in_background_skips_poll() {
        use wiremock::matchers::{method, path, path_regex};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        // request_build → returns template/build ids.
        Mock::given(method("POST"))
            .and(path("/v3/templates"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "templateID": "tpl_bg",
                "buildID": "bld_bg",
                "aliases": [],
                "names": ["my-env"],
                "public": false,
                "tags": []
            })))
            .mount(&server)
            .await;

        // trigger_build → 204.
        Mock::given(method("POST"))
            .and(path("/v2/templates/tpl_bg/builds/bld_bg"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        // status endpoint must NEVER be called by build_in_background.
        Mock::given(method("GET"))
            .and(path_regex(r"^/templates/.+/builds/.+/status$"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        // A template with NO COPY instructions → no file uploads happen.
        let template = Template::new().from_image("node:20");
        let opts = BuildOptions {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            api_url: Some(server.uri()),
            ..Default::default()
        };

        let info = template
            .build_in_background("my-env", opts)
            .await
            .expect("build_in_background should succeed");

        assert_eq!(info.template_id, "tpl_bg");
        assert_eq!(info.build_id, "bld_bg");
        assert_eq!(info.name.as_deref(), Some("my-env"));

        // wiremock asserts the status-endpoint `expect(0)` on server drop.
    }
}
