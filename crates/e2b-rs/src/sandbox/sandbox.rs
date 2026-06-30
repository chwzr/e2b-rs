//! The public `Sandbox` type and its control-plane lifecycle.

use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::time::Duration;

use crate::api::client::ApiClient;
use crate::connection_config::ConnectionConfig;
use crate::envd::versions::version_gte;
use crate::errors::{Error, Result};
use crate::sandbox::api;
use crate::sandbox::opts::{SandboxConnectOpts, SandboxCreateOpts};
use crate::sandbox::types::{SandboxInfo, SandboxMetrics, SandboxState};

/// A boxed future returned by the lifecycle builders.
type SandboxFuture = Pin<Box<dyn Future<Output = Result<Sandbox>> + Send>>;

/// A running or paused E2B sandbox.
///
/// Create one with [`Sandbox::create`] or reconnect with [`Sandbox::connect`].
///
/// # Example
/// ```no_run
/// # async fn run() -> e2b_rs::Result<()> {
/// use e2b_rs::Sandbox;
/// let sandbox = Sandbox::create().template("base").await?;
/// println!("sandbox {} at {}", sandbox.sandbox_id(), sandbox.get_host(3000));
/// sandbox.kill().await?;
/// # Ok(())
/// # }
/// ```
pub struct Sandbox {
    /// Sandbox identifier.
    pub(crate) sandbox_id: String,
    /// Optional per-sandbox domain override.
    pub(crate) sandbox_domain: Option<String>,
    /// envd version string; used by version gates and envd I/O in Plan 3b.
    pub(crate) envd_version: String,
    /// Access token for envd communication; read by envd I/O in Plan 3b.
    #[allow(dead_code)] // read by envd I/O in Plan 3b
    pub(crate) envd_access_token: Option<String>,
    /// Resolved connection configuration.
    pub(crate) config: ConnectionConfig,
    /// Control-plane API client.
    pub(crate) api: ApiClient,
}

impl Sandbox {
    /// Start configuring a new sandbox. Await the builder to create it.
    pub fn create() -> SandboxCreateBuilder {
        SandboxCreateBuilder {
            opts: SandboxCreateOpts::default(),
        }
    }

    /// Start configuring a reconnect to an existing (possibly paused) sandbox.
    pub fn connect(sandbox_id: impl Into<String>) -> SandboxConnectBuilder {
        SandboxConnectBuilder {
            sandbox_id: sandbox_id.into(),
            opts: SandboxConnectOpts::default(),
        }
    }

    /// The sandbox identifier.
    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }

    /// The external host for a sandbox port, e.g. `3000-<id>.e2b.app`.
    pub fn get_host(&self, port: u16) -> String {
        let domain = self
            .sandbox_domain
            .as_deref()
            .unwrap_or(&self.config.domain);
        self.config.get_host(&self.sandbox_id, port, Some(domain))
    }

    /// Kill the sandbox. Returns `false` if it was already gone.
    pub async fn kill(&self) -> Result<bool> {
        api::kill_sandbox(&self.api, &self.sandbox_id).await
    }

    /// Fetch current sandbox info.
    pub async fn get_info(&self) -> Result<SandboxInfo> {
        let detail = api::get_sandbox_info(&self.api, &self.sandbox_id).await?;
        Ok(SandboxInfo::from_detail(detail))
    }

    /// Set the sandbox timeout, measured from now.
    pub async fn set_timeout(&self, timeout: Duration) -> Result<()> {
        api::set_sandbox_timeout(&self.api, &self.sandbox_id, timeout).await
    }

    /// Pause the sandbox, returning `false` if it was already paused.
    ///
    /// Takes a full memory snapshot so a later [`Sandbox::connect`] warm-boots.
    pub async fn pause(&self) -> Result<bool> {
        api::pause_sandbox(&self.api, &self.sandbox_id, true).await
    }

    /// Whether the sandbox is currently running (control-plane state).
    ///
    /// Note: Plan 3b refines this to the envd `/health` probe; for now it
    /// reflects the control-plane `state`.
    pub async fn is_running(&self) -> Result<bool> {
        Ok(matches!(
            self.get_info().await?.state,
            SandboxState::Running
        ))
    }

    /// Fetch the sandbox's resource-usage metrics.
    ///
    /// # Errors
    /// Returns [`Error::Template`] if the sandbox's envd is older than `0.1.5`
    /// (metrics are unsupported), matching the JS SDK.
    pub async fn get_metrics(&self) -> Result<Vec<SandboxMetrics>> {
        if !version_gte(&self.envd_version, "0.1.5") {
            return Err(Error::Template(
                "Metrics require a newer template (envd >= 0.1.5); rebuild the template."
                    .to_string(),
            ));
        }
        let raw = api::get_sandbox_metrics(&self.api, &self.sandbox_id, None, None).await?;
        Ok(raw.into_iter().map(SandboxMetrics::from_metric).collect())
    }

    /// List sandboxes (paginated). Filter by state/metadata via [`crate::SandboxListOpts`].
    ///
    /// Note: defaults to listing only `Running` and `Paused` sandboxes (the JS
    /// SDK lists all states by default); pass `SandboxListOpts::states` to override.
    pub fn list(
        opts: crate::sandbox::opts::SandboxListOpts,
    ) -> Result<crate::sandbox::paginator::SandboxPaginator> {
        crate::sandbox::paginator::SandboxPaginator::new(opts)
    }

    /// Build a `Sandbox` from a `create`/`connect` response (the lean
    /// `api::schema::Sandbox`) plus the resolved config/client.
    fn from_api_sandbox(
        s: crate::api::schema::Sandbox,
        config: ConnectionConfig,
        api: ApiClient,
    ) -> Sandbox {
        Sandbox {
            sandbox_id: s.sandbox_id,
            sandbox_domain: s.domain,
            envd_version: s.envd_version.0,
            envd_access_token: s.envd_access_token,
            config,
            api,
        }
    }
}

/// Builder for [`Sandbox::create`]; await it to create the sandbox.
pub struct SandboxCreateBuilder {
    opts: SandboxCreateOpts,
}

impl SandboxCreateBuilder {
    /// Template id or alias (default `"base"`).
    pub fn template(mut self, template: impl Into<String>) -> Self {
        self.opts.template = Some(template.into());
        self
    }

    /// Sandbox lifetime (default 5 minutes).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.opts.timeout = Some(timeout);
        self
    }

    /// Add metadata entries.
    pub fn metadata<K: Into<String>, V: Into<String>>(
        mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.opts
            .metadata
            .extend(entries.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Add environment variables.
    pub fn envs<K: Into<String>, V: Into<String>>(
        mut self,
        entries: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.opts
            .envs
            .extend(entries.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Set the API key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.opts.connection.api_key = Some(key.into());
        self
    }

    /// Set the domain.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.opts.connection.domain = Some(domain.into());
        self
    }

    /// Override the API base URL (mainly for tests/self-hosting).
    pub fn api_url(mut self, url: impl Into<String>) -> Self {
        self.opts.connection.api_url = Some(url.into());
        self
    }

    /// Enable debug mode.
    pub fn debug(mut self, debug: bool) -> Self {
        self.opts.connection.debug = Some(debug);
        self
    }

    /// Whether to use a secure (authenticated) connection to the sandbox
    /// (defaults to `true`, matching the JS SDK).
    pub fn secure(mut self, secure: bool) -> Self {
        self.opts.secure = Some(secure);
        self
    }

    /// Whether the sandbox may access the public internet (defaults to `true`,
    /// matching the JS SDK).
    pub fn allow_internet_access(mut self, allow: bool) -> Self {
        self.opts.allow_internet_access = Some(allow);
        self
    }
}

impl IntoFuture for SandboxCreateBuilder {
    type Output = Result<Sandbox>;
    type IntoFuture = SandboxFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let config = ConnectionConfig::new(self.opts.connection.clone());
            let api = ApiClient::new(&config, true)?;
            let sandbox = api::create_sandbox(&api, &self.opts).await?;
            Ok(Sandbox::from_api_sandbox(sandbox, config, api))
        })
    }
}

/// Builder for [`Sandbox::connect`]; await it to (re)connect.
pub struct SandboxConnectBuilder {
    sandbox_id: String,
    opts: SandboxConnectOpts,
}

impl SandboxConnectBuilder {
    /// Lifetime to set on (re)connect (default 5 minutes).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.opts.timeout = Some(timeout);
        self
    }

    /// Set the API key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.opts.connection.api_key = Some(key.into());
        self
    }

    /// Override the API base URL.
    pub fn api_url(mut self, url: impl Into<String>) -> Self {
        self.opts.connection.api_url = Some(url.into());
        self
    }

    /// Set the domain.
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.opts.connection.domain = Some(domain.into());
        self
    }
}

impl IntoFuture for SandboxConnectBuilder {
    type Output = Result<Sandbox>;
    type IntoFuture = SandboxFuture;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let config = ConnectionConfig::new(self.opts.connection.clone());
            let api = ApiClient::new(&config, true)?;
            let timeout = self.opts.timeout.unwrap_or(Duration::from_millis(
                crate::connection_config::DEFAULT_SANDBOX_TIMEOUT_MS,
            ));
            let sandbox = api::connect_sandbox(&api, &self.sandbox_id, timeout).await?;
            Ok(Sandbox::from_api_sandbox(sandbox, config, api))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// The rich `SandboxDetail` returned by `GET /sandboxes/{id}`.
    fn detail_json(id: &str, state: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "cpuCount": 2, "memoryMB": 1024, "diskSizeMB": 1024,
            "envdVersion": "0.6.0", "state": state, "domain": "e2b.app",
            "startedAt": "2026-06-30T10:00:00Z", "endAt": "2026-06-30T10:05:00Z"
        })
    }

    /// The lean `Sandbox` returned by `POST /sandboxes` and `/connect` — no
    /// `cpuCount`/`memoryMB`/`state` fields, matching the real wire response.
    fn sandbox_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "envdVersion": "0.6.0", "domain": "e2b.app"
        })
    }

    #[tokio::test]
    async fn create_builds_a_sandbox_and_awaits_directly() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes"))
            .respond_with(ResponseTemplate::new(201).set_body_json(sandbox_json("sbx_c")))
            .mount(&server)
            .await;
        // Builder + direct `.await` (IntoFuture).
        let sandbox = Sandbox::create()
            .template("base")
            .timeout(Duration::from_secs(60))
            .api_key("e2b_0123456789abcdef")
            .domain("e2b.app")
            .api_url(server.uri())
            .await
            .expect("create");
        assert_eq!(sandbox.sandbox_id(), "sbx_c");
        assert_eq!(sandbox.get_host(3000), "3000-sbx_c.e2b.app");
    }

    #[tokio::test]
    async fn get_info_and_kill_round_trip() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes"))
            .respond_with(ResponseTemplate::new(201).set_body_json(sandbox_json("sbx_i")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/sandboxes/sbx_i"))
            .respond_with(ResponseTemplate::new(200).set_body_json(detail_json("sbx_i", "running")))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/sandboxes/sbx_i"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let sandbox = Sandbox::create()
            .api_key("e2b_0123456789abcdef")
            .api_url(server.uri())
            .await
            .expect("create");
        let info = sandbox.get_info().await.expect("info");
        assert_eq!(info.sandbox_id, "sbx_i");
        assert!(sandbox.is_running().await.expect("running")); // state == running
        assert!(sandbox.kill().await.expect("kill"));
    }
}
