//! envd daemon REST client (port of the client side of `envd/api.ts`).

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::errors::Result;
use crate::logs::Logger;

/// Health-check timeout for envd, matching `checkSandboxHealth` (5 s).
const HEALTH_TIMEOUT_MS: u64 = 5_000;

/// Options for constructing an [`EnvdApiClient`].
#[allow(dead_code)] // used by Plan 3
pub(crate) struct EnvdApiClientOpts {
    /// Base URL of the sandbox's envd REST surface.
    pub base_url: String,
    /// envd access token (sent as `X-Access-Token`).
    pub access_token: Option<String>,
    /// Sandbox id (sent as `E2b-Sandbox-Id`).
    pub sandbox_id: String,
    /// envd port (sent as `E2b-Sandbox-Port`).
    pub envd_port: u16,
    /// `User-Agent` header value.
    pub user_agent: String,
    /// Per-request timeout in milliseconds.
    pub request_timeout_ms: u64,
    /// Optional logger.
    pub logger: Option<Arc<dyn Logger>>,
    /// Optional proxy URL.
    pub proxy: Option<String>,
}

/// Client for the in-sandbox envd daemon's REST surface (`/health`, and — in a
/// later milestone — `/files`).
#[allow(dead_code)] // used by Plan 3
pub(crate) struct EnvdApiClient {
    http: reqwest::Client,
    base_url: String,
    request_timeout: Duration,
    logger: Option<Arc<dyn Logger>>,
}

#[allow(dead_code)] // methods used by Plan 3 callers
impl EnvdApiClient {
    /// Build the client, baking the sandbox/access headers into the underlying
    /// `reqwest::Client` as default headers.
    pub(crate) fn new(opts: EnvdApiClientOpts) -> Result<Self> {
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
        if let Some(proxy) = &opts.proxy
            && let Ok(p) = reqwest::Proxy::all(proxy)
        {
            builder = builder.proxy(p);
        }
        let http = builder.build()?;

        Ok(Self {
            http,
            base_url: opts.base_url.trim_end_matches('/').to_string(),
            request_timeout: Duration::from_millis(opts.request_timeout_ms),
            logger: opts.logger,
        })
    }

    /// Return `true` iff `GET /health` responds 2xx within the health timeout.
    /// Any error, non-2xx, or timeout yields `false` (mirrors `checkSandboxHealth`).
    pub(crate) async fn check_health(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        let timeout = self
            .request_timeout
            .min(Duration::from_millis(HEALTH_TIMEOUT_MS));
        if let Some(logger) = &self.logger {
            logger.debug(&format!("GET {url} (health)"));
        }
        match self.http.get(&url).timeout(timeout).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn opts_for(server: &MockServer) -> EnvdApiClientOpts {
        EnvdApiClientOpts {
            base_url: server.uri(),
            access_token: Some("tok-123".to_string()),
            sandbox_id: "sbx_test".to_string(),
            envd_port: 49983,
            user_agent: "e2b-rs/test".to_string(),
            request_timeout_ms: 5_000,
            logger: None,
            proxy: None,
        }
    }

    #[tokio::test]
    async fn check_health_true_on_200_with_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .and(header("X-Access-Token", "tok-123"))
            .and(header("E2b-Sandbox-Id", "sbx_test"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = EnvdApiClient::new(opts_for(&server)).expect("construct");
        assert!(client.check_health().await);
    }

    #[tokio::test]
    async fn check_health_false_on_502() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let client = EnvdApiClient::new(opts_for(&server)).expect("construct");
        assert!(!client.check_health().await);
    }
}
