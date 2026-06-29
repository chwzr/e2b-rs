//! Control-plane sandbox lifecycle calls.

use crate::api::client::ApiClient;
use crate::api::schema as api_schema;
use crate::errors::{Error, Result};
use crate::sandbox::opts::SandboxCreateOpts;
use crate::utils::timeout_to_seconds;
use std::time::Duration;

/// Default sandbox lifetime (5 minutes), matching `DEFAULT_SANDBOX_TIMEOUT_MS`.
#[allow(dead_code)] // consumed by create_sandbox; silenced until Task 4 wires callers
const DEFAULT_TIMEOUT: Duration =
    Duration::from_millis(crate::connection_config::DEFAULT_SANDBOX_TIMEOUT_MS);

/// Map create options to the generated `NewSandbox` body and POST it.
#[allow(dead_code)] // consumed by Task 4 Sandbox::create
pub(crate) async fn create_sandbox(
    api: &ApiClient,
    opts: &SandboxCreateOpts,
) -> Result<api_schema::SandboxDetail> {
    let timeout = opts.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let timeout_secs = timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX));
    let template = opts.template.clone().unwrap_or_else(|| "base".to_string());

    // Build the request JSON by NewSandbox field names (camelCase on the wire).
    let mut body = serde_json::json!({
        "templateID": template,
        "timeout": timeout_secs,
    });
    if !opts.metadata.is_empty() {
        body["metadata"] = serde_json::to_value(&opts.metadata).unwrap_or_default();
    }
    if !opts.envs.is_empty() {
        body["envVars"] = serde_json::to_value(&opts.envs).unwrap_or_default();
    }
    if let Some(secure) = opts.secure {
        body["secure"] = serde_json::Value::Bool(secure);
    }
    if let Some(allow) = opts.allow_internet_access {
        body["allowInternetAccess"] = serde_json::Value::Bool(allow);
    }

    api.request(reqwest::Method::POST, "/sandboxes", &[], Some(&body))
        .await
}

/// Resume/connect to a sandbox.
#[allow(dead_code)] // consumed by Task 4 Sandbox::connect
pub(crate) async fn connect_sandbox(
    api: &ApiClient,
    sandbox_id: &str,
    timeout: Duration,
) -> Result<api_schema::SandboxDetail> {
    let body = serde_json::json!({
        "timeout": timeout_to_seconds(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX))
    });
    let path = format!("/sandboxes/{sandbox_id}/connect");
    api.request(reqwest::Method::POST, &path, &[], Some(&body))
        .await
}

/// Kill a sandbox. Returns `false` if it was not found (already gone).
#[allow(dead_code)] // consumed by Task 4 Sandbox::kill
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

/// Fetch sandbox info.
#[allow(dead_code)] // consumed by Task 4 Sandbox::get_info
pub(crate) async fn get_sandbox_info(
    api: &ApiClient,
    sandbox_id: &str,
) -> Result<api_schema::SandboxDetail> {
    let path = format!("/sandboxes/{sandbox_id}");
    api.request(reqwest::Method::GET, &path, &[], None).await
}

/// Set the sandbox timeout (from now).
#[allow(dead_code)] // consumed by Task 4 Sandbox::set_timeout
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

    fn detail_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "sandboxID": id, "templateID": "base", "clientID": "c1",
            "cpuCount": 2, "memoryMB": 1024, "diskSizeMB": 1024,
            "envdVersion": "0.6.0", "state": "running",
            "startedAt": "2026-06-30T10:00:00Z", "endAt": "2026-06-30T10:05:00Z"
        })
    }

    #[tokio::test]
    async fn create_posts_new_sandbox_with_template_and_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandboxes"))
            // NewSandbox uses templateID + timeout (seconds).
            .and(body_partial_json(
                serde_json::json!({"templateID": "base", "timeout": 300}),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(detail_json("sbx_new")))
            .mount(&server)
            .await;
        let api = api_for(&server);
        let opts = SandboxCreateOpts {
            timeout: Some(Duration::from_secs(300)),
            ..Default::default()
        };
        let detail = create_sandbox(&api, &opts).await.expect("create");
        assert_eq!(detail.sandbox_id, "sbx_new");
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
}
