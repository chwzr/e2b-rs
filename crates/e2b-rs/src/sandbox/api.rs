//! Control-plane sandbox lifecycle calls.

use crate::api::client::ApiClient;
use crate::api::schema as api_schema;
use crate::envd::versions::version_gte;
use crate::errors::{Error, Result};
use crate::sandbox::opts::SandboxCreateOpts;
use crate::utils::timeout_to_seconds;
use std::time::Duration;

/// Default sandbox lifetime (5 minutes), matching `DEFAULT_SANDBOX_TIMEOUT_MS`.
const DEFAULT_TIMEOUT: Duration =
    Duration::from_millis(crate::connection_config::DEFAULT_SANDBOX_TIMEOUT_MS);

/// Lowest envd version this SDK can talk to; older templates must be rebuilt.
const MIN_ENVD_VERSION: &str = "0.1.0";

/// Map create options to the `NewSandbox` body, POST it, and validate the
/// returned envd version (mirrors JS `createSandbox`).
///
/// `POST /sandboxes` returns the lean `Sandbox` schema (NOT `SandboxDetail`),
/// so we decode into [`api_schema::Sandbox`]; the richer detail is only
/// returned by `GET /sandboxes/{id}` (see [`get_sandbox_info`]).
pub(crate) async fn create_sandbox(
    api: &ApiClient,
    opts: &SandboxCreateOpts,
) -> Result<api_schema::Sandbox> {
    let timeout = opts.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let timeout_secs = timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX));
    let template = opts.template.clone().unwrap_or_else(|| "base".to_string());

    // Field names follow the `NewSandbox` schema. The casing is deliberately
    // mixed and matches the spec: `templateID`/`envVars` are camelCase, but
    // `allow_internet_access` is snake_case (NewSandbox has no serde rename).
    // `secure`/`allow_internet_access` are always sent, defaulting to `true`,
    // exactly as the JS SDK does (`opts?.x ?? true`).
    let mut body = serde_json::json!({
        "templateID": template,
        "timeout": timeout_secs,
        "secure": opts.secure.unwrap_or(true),
        "allow_internet_access": opts.allow_internet_access.unwrap_or(true),
    });
    if !opts.metadata.is_empty() {
        body["metadata"] = serde_json::to_value(&opts.metadata).unwrap_or_default();
    }
    if !opts.envs.is_empty() {
        body["envVars"] = serde_json::to_value(&opts.envs).unwrap_or_default();
    }

    let sandbox: api_schema::Sandbox = api
        .request(reqwest::Method::POST, "/sandboxes", &[], Some(&body))
        .await?;

    // Templates older than envd 0.1.0 are incompatible with this SDK; JS kills
    // the freshly-created sandbox and raises `TemplateError`.
    if !version_gte(&sandbox.envd_version.0, MIN_ENVD_VERSION) {
        kill_sandbox(api, &sandbox.sandbox_id).await?;
        return Err(Error::Template(
            "You need to update the template to use the new SDK.".to_string(),
        ));
    }

    Ok(sandbox)
}

/// Resume/connect to a (possibly paused) sandbox.
///
/// Like [`create_sandbox`], `POST /sandboxes/{id}/connect` returns the lean
/// `Sandbox` schema. A 404 means the paused sandbox is gone, which JS surfaces
/// as `SandboxNotFoundError` rather than a generic not-found.
pub(crate) async fn connect_sandbox(
    api: &ApiClient,
    sandbox_id: &str,
    timeout: Duration,
) -> Result<api_schema::Sandbox> {
    let body = serde_json::json!({
        "timeout": timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX))
    });
    let path = format!("/sandboxes/{sandbox_id}/connect");
    match api
        .request::<api_schema::Sandbox>(reqwest::Method::POST, &path, &[], Some(&body))
        .await
    {
        Ok(sandbox) => Ok(sandbox),
        Err(Error::NotFound(_)) => Err(Error::SandboxNotFound(format!(
            "Paused sandbox {sandbox_id} not found"
        ))),
        Err(e) => Err(e),
    }
}

/// Kill a sandbox. Returns `false` if it was not found (already gone).
pub(crate) async fn kill_sandbox(api: &ApiClient, sandbox_id: &str) -> Result<bool> {
    let path = format!("/sandboxes/{sandbox_id}");
    match api
        .request_unit(reqwest::Method::DELETE, &path, &[], None)
        .await
    {
        Ok(()) => Ok(true),
        Err(Error::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Fetch sandbox info (`GET /sandboxes/{id}` returns the rich `SandboxDetail`).
pub(crate) async fn get_sandbox_info(
    api: &ApiClient,
    sandbox_id: &str,
) -> Result<api_schema::SandboxDetail> {
    let path = format!("/sandboxes/{sandbox_id}");
    api.request(reqwest::Method::GET, &path, &[], None).await
}

/// Pause a sandbox. `keep_memory` controls whether a full memory snapshot is
/// taken (warm resume) vs filesystem-only. Returns `false` if the sandbox is
/// already paused (HTTP 409, idempotent), matching the JS SDK.
pub(crate) async fn pause_sandbox(
    api: &ApiClient,
    sandbox_id: &str,
    keep_memory: bool,
) -> Result<bool> {
    let body = serde_json::json!({ "memory": keep_memory });
    let path = format!("/sandboxes/{sandbox_id}/pause");
    match api
        .request_unit(reqwest::Method::POST, &path, &[], Some(&body))
        .await
    {
        Ok(()) => Ok(true),
        // 409 = already paused; idempotent, not an error.
        Err(Error::Conflict(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Set the sandbox timeout (from now).
pub(crate) async fn set_sandbox_timeout(
    api: &ApiClient,
    sandbox_id: &str,
    timeout: Duration,
) -> Result<()> {
    let body = serde_json::json!({
        "timeout": timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX))
    });
    let path = format!("/sandboxes/{sandbox_id}/timeout");
    api.request_unit(reqwest::Method::POST, &path, &[], Some(&body))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
    use crate::sandbox::opts::SandboxCreateOpts;
    use std::time::Duration;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn api_for(server: &MockServer) -> ApiClient {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            api_url: Some(server.uri()),
            ..Default::default()
        });
        ApiClient::new(&config, true).expect("api client")
    }

    /// The lean `Sandbox` schema actually returned by `POST /sandboxes` and
    /// `/connect` — deliberately WITHOUT the `SandboxDetail`-only fields
    /// (`cpuCount`/`memoryMB`/`state`/...). If `create`/`connect` ever revert
    /// to decoding `SandboxDetail`, deserializing this body fails the test.
    fn sandbox_json(id: &str, envd_version: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "envdVersion": envd_version, "domain": "e2b.app"
        })
    }

    #[tokio::test]
    async fn create_posts_new_sandbox_with_template_and_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes"))
            // NewSandbox: templateID + timeout (seconds); secure /
            // allow_internet_access always sent, defaulting to true (snake_case).
            .and(body_partial_json(serde_json::json!({
                "templateID": "base", "timeout": 300,
                "secure": true, "allow_internet_access": true,
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(sandbox_json("sbx_new", "0.6.0")),
            )
            .mount(&server)
            .await;
        let api = api_for(&server);
        let opts = SandboxCreateOpts {
            timeout: Some(Duration::from_secs(300)),
            ..Default::default()
        };
        let sandbox = create_sandbox(&api, &opts).await.expect("create");
        assert_eq!(sandbox.sandbox_id, "sbx_new");
    }

    #[tokio::test]
    async fn create_rejects_and_kills_old_envd_template() {
        let server = MockServer::start().await;
        // POST returns a sandbox whose envd is older than the 0.1.0 floor.
        Mock::given(method("POST"))
            .and(path("/sandboxes"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(sandbox_json("sbx_old", "0.0.9")),
            )
            .mount(&server)
            .await;
        // The SDK must kill it before surfacing the TemplateError.
        Mock::given(method("DELETE"))
            .and(path("/sandboxes/sbx_old"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let err = create_sandbox(&api_for(&server), &SandboxCreateOpts::default())
            .await
            .expect_err("old template must be rejected");
        assert!(matches!(err, Error::Template(_)));
    }

    #[tokio::test]
    async fn connect_maps_404_to_sandbox_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes/sbx_paused/connect"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = connect_sandbox(&api_for(&server), "sbx_paused", Duration::from_secs(60))
            .await
            .expect_err("missing paused sandbox");
        assert!(matches!(err, Error::SandboxNotFound(_)));
    }

    #[tokio::test]
    async fn kill_returns_false_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/sandboxes/sbx_gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(
            !kill_sandbox(&api_for(&server), "sbx_gone")
                .await
                .expect("kill")
        );
    }

    #[tokio::test]
    async fn kill_returns_true_on_204() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/sandboxes/sbx_ok"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(
            kill_sandbox(&api_for(&server), "sbx_ok")
                .await
                .expect("kill")
        );
    }

    #[tokio::test]
    async fn pause_returns_true_on_204() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes/sbx_p/pause"))
            .and(body_partial_json(serde_json::json!({ "memory": true })))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(
            pause_sandbox(&api_for(&server), "sbx_p", true)
                .await
                .expect("pause")
        );
    }

    #[tokio::test]
    async fn pause_returns_false_when_already_paused_409() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes/sbx_p/pause"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;
        assert!(
            !pause_sandbox(&api_for(&server), "sbx_p", true)
                .await
                .expect("pause idempotent")
        );
    }
}
