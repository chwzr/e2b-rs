//! Connect-over-JSON client: unary + (Task 5) server-streaming calls.

use std::sync::Arc;
use std::time::Duration;

use base64::prelude::{BASE64_STANDARD, Engine as _};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::connect::error::{map_code_to_error, parse_connect_error};
use crate::envd::versions::{ENVD_DEFAULT_USER, version_gte};
use crate::errors::{Error, Result};
use crate::logs::Logger;

/// Default sandbox user for Basic auth (matches `defaultUsername`).
const DEFAULT_USER: &str = "user";

/// Options for constructing a [`ConnectClient`].
#[allow(dead_code)] // used by Plan 3
pub(crate) struct ConnectClientOpts {
    /// Base URL of the sandbox envd RPC surface.
    pub base_url: String,
    /// envd access token (`X-Access-Token`).
    pub access_token: Option<String>,
    /// Sandbox id (`E2b-Sandbox-Id`).
    pub sandbox_id: String,
    /// envd port (`E2b-Sandbox-Port`).
    pub envd_port: u16,
    /// `User-Agent` header value.
    pub user_agent: String,
    /// envd version (gates the auth header).
    pub envd_version: String,
    /// Per-request timeout (ms).
    pub request_timeout_ms: u64,
    /// Optional logger.
    pub logger: Option<Arc<dyn Logger>>,
    /// Optional proxy URL.
    pub proxy: Option<String>,
}

/// The `Authorization: Basic base64("{user}:")` header, version-gated by
/// `ENVD_DEFAULT_USER`. For envd < 0.4.0 (no default-user support) returns
/// `None` unless an explicit user is given. Mirrors `authenticationHeader`.
#[allow(dead_code)] // used by Plan 3
pub(crate) fn auth_header(
    envd_version: &str,
    user: Option<&str>,
) -> Option<(HeaderName, HeaderValue)> {
    let username = match (user, version_gte(envd_version, ENVD_DEFAULT_USER)) {
        (Some(u), _) => u,
        (None, true) => DEFAULT_USER,
        (None, false) => return None,
    };
    let value = format!("Basic {}", BASE64_STANDARD.encode(format!("{username}:")));
    let header = HeaderValue::from_str(&value).ok()?;
    Some((reqwest::header::AUTHORIZATION, header))
}

/// Connect-over-JSON RPC client for the envd daemon.
#[allow(dead_code)] // used by Plan 3
pub(crate) struct ConnectClient {
    http: reqwest::Client,
    base_url: String,
    envd_version: String,
    request_timeout: Duration,
    logger: Option<Arc<dyn Logger>>,
}

#[allow(dead_code)] // methods used by Plan 3 callers
impl ConnectClient {
    /// Build the client, baking the sandbox/access/user-agent headers into the
    /// underlying `reqwest::Client` as default headers.
    pub(crate) fn new(opts: ConnectClientOpts) -> Result<Self> {
        let mut headers = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&opts.user_agent) {
            headers.insert(HeaderName::from_static("user-agent"), v);
        }
        if let Ok(v) = HeaderValue::from_str(&opts.sandbox_id) {
            headers.insert(HeaderName::from_static("e2b-sandbox-id"), v);
        }
        if let Ok(v) = HeaderValue::from_str(&opts.envd_port.to_string()) {
            headers.insert(HeaderName::from_static("e2b-sandbox-port"), v);
        }
        if let Some(token) = &opts.access_token
            && let Ok(v) = HeaderValue::from_str(token)
        {
            headers.insert(HeaderName::from_static("x-access-token"), v);
        }

        let mut builder = reqwest::Client::builder().default_headers(headers);
        if let Some(proxy) = &opts.proxy {
            let p = reqwest::Proxy::all(proxy)
                .map_err(|e| Error::InvalidArgument(format!("invalid proxy URL {proxy:?}: {e}")))?;
            builder = builder.proxy(p);
        }
        let http = builder.build()?;

        Ok(Self {
            http,
            base_url: opts.base_url.trim_end_matches('/').to_string(),
            envd_version: opts.envd_version,
            request_timeout: Duration::from_millis(opts.request_timeout_ms),
            logger: opts.logger,
        })
    }

    /// Make a unary Connect call: `POST {base}{path}` with `application/json`,
    /// returning the decoded `Resp`. Maps a non-2xx response to a typed error.
    pub(crate) async fn unary<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        path: &str,
        req: &Req,
        user: Option<&str>,
    ) -> Result<Resp> {
        let url = format!("{}{path}", self.base_url);
        if let Some(logger) = &self.logger {
            logger.debug(&format!("POST {url}"));
        }
        let body = serde_json::to_vec(req)
            .map_err(|e| Error::Internal(format!("failed to encode request for {path}: {e}")))?;

        let mut rb = self
            .http
            .post(&url)
            .timeout(self.request_timeout)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("connect-protocol-version", "1")
            .body(body);
        if let Some((name, value)) = auth_header(&self.envd_version, user) {
            rb = rb.header(name, value);
        }

        let resp = rb.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let (code, message) = parse_connect_error(status.as_u16(), &bytes);
            return Err(map_code_to_error(code, message));
        }
        serde_json::from_slice::<Resp>(&bytes)
            .map_err(|e| Error::Internal(format!("failed to decode response from {path}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn opts_for(server: &MockServer) -> ConnectClientOpts {
        ConnectClientOpts {
            base_url: server.uri(),
            access_token: Some("tok".to_string()),
            sandbox_id: "sbx".to_string(),
            envd_port: 49983,
            user_agent: "e2b-rs/test".to_string(),
            envd_version: "0.6.0".to_string(),
            request_timeout_ms: 5_000,
            logger: None,
            proxy: None,
        }
    }

    #[test]
    fn auth_header_is_basic_for_modern_envd() {
        // 0.6.0 >= 0.4.0 → Basic base64("user:") for the default user.
        let (name, value) = auth_header("0.6.0", None).expect("auth header");
        assert_eq!(name.as_str(), "authorization");
        // base64("user:") = "dXNlcjo="
        assert_eq!(value.to_str().unwrap_or(""), "Basic dXNlcjo=");
        // Explicit user.
        let (_, v2) = auth_header("0.6.0", Some("root")).expect("auth header");
        assert_eq!(
            v2.to_str().unwrap_or(""),
            format!("Basic {}", base64_user("root"))
        );
    }

    // Helper mirroring the impl for the test's expectation.
    fn base64_user(u: &str) -> String {
        use base64::prelude::{BASE64_STANDARD, Engine as _};
        BASE64_STANDARD.encode(format!("{u}:"))
    }

    #[tokio::test]
    async fn unary_posts_json_and_decodes_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/process.Process/List"))
            .and(header("content-type", "application/json"))
            .and(header("connect-protocol-version", "1"))
            .and(header("X-Access-Token", "tok"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"echoed": true})),
            )
            .mount(&server)
            .await;

        let client = ConnectClient::new(opts_for(&server)).expect("client");
        let req = serde_json::json!({"ping": 1});
        let resp: serde_json::Value = client
            .unary(super::super::PROC_LIST, &req, None)
            .await
            .expect("unary ok");
        assert_eq!(resp["echoed"], serde_json::Value::Bool(true));
    }

    #[tokio::test]
    async fn unary_maps_connect_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/Stat"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "code": "not_found", "message": "no such file"
            })))
            .mount(&server)
            .await;
        let client = ConnectClient::new(opts_for(&server)).expect("client");
        let err = client
            .unary::<_, serde_json::Value>(super::super::FS_STAT, &serde_json::json!({}), None)
            .await
            .unwrap_err();
        match err {
            crate::errors::Error::NotFound(m) => assert!(m.contains("no such file")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
