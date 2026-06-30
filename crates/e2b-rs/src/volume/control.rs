//! Public [`Volume`] struct and control-plane CRUD.
//!
//! The control-plane methods ([`Volume::create`], [`Volume::list`],
//! [`Volume::get_info`], [`Volume::connect`], [`Volume::destroy`]) talk to the
//! E2B API service via the internal `ApiClient`.
//!
//! Volume *content* operations (reading/writing files) are added in Tasks 3–4
//! and build a `VolumeApiClient` from the stored connection parameters.

use crate::api::client::ApiClient;
use crate::api::schema as api_schema;
use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts, REQUEST_TIMEOUT_MS};
use crate::errors::{Error, Result};
use crate::volume::types::{VolumeAndToken, VolumeInfo};

/// Default per-request timeout stored in [`Volume`] when
/// [`VolumeOpts::request_timeout_ms`] is `None`.
const DEFAULT_REQUEST_TIMEOUT_MS: u64 = REQUEST_TIMEOUT_MS;

/// Options for authenticating and connecting to the E2B volume API.
///
/// All fields are optional and fall back to environment variables where
/// documented. This mirrors the JS `VolumeApiOpts` type.
#[derive(Default, Debug, Clone)]
pub struct VolumeOpts {
    /// E2B API key. Falls back to the `E2B_API_KEY` environment variable.
    pub api_key: Option<String>,
    /// E2B domain override. Falls back to `E2B_DOMAIN` (default `e2b.app`).
    pub domain: Option<String>,
    /// Control-plane API base URL override. Falls back to `E2B_API_URL`.
    /// Primarily used in tests to point at a local or mock server.
    pub api_url: Option<String>,
    /// Per-request timeout in milliseconds applied to **both** control-plane
    /// calls (create/list/get_info/connect/destroy) and the volume-content
    /// client constructed in Tasks 3–4. Defaults to
    /// [`crate::connection_config::REQUEST_TIMEOUT_MS`] (60 s).
    pub request_timeout_ms: Option<u64>,
    /// Optional HTTP/HTTPS proxy URL forwarded to **both** the control-plane
    /// client and the volume-content client.
    pub proxy: Option<String>,
}

/// A handle to an E2B persistent volume.
///
/// Holds the resolved connection parameters used to build a content client
/// (`VolumeApiClient`) in Tasks 3–4.
///
/// Obtain a `Volume` via [`Volume::create`] or [`Volume::connect`].
#[derive(Debug, Clone)]
pub struct Volume {
    volume_id: String,
    name: String,
    token: String,
    /// Resolved API/content base URL — stored for Tasks 3–4.
    #[allow(dead_code)]
    api_url: String,
    /// Stored for Tasks 3–4 content-client construction.
    #[allow(dead_code)]
    request_timeout_ms: u64,
    /// Stored for Tasks 3–4 content-client construction.
    #[allow(dead_code)]
    proxy: Option<String>,
}

impl Volume {
    /// The unique identifier for this volume.
    pub fn volume_id(&self) -> &str {
        &self.volume_id
    }

    /// The human-readable name of this volume.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Short-lived Bearer token for the volume content API.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Construct a `Volume` from resolved parts.
    ///
    /// Used internally by [`Volume::create`] and [`Volume::connect`], and by
    /// integration tests that inject a wiremock base URL via `api_url`.
    pub(crate) fn from_parts(
        volume_id: String,
        name: String,
        token: String,
        api_url: String,
        request_timeout_ms: u64,
        proxy: Option<String>,
    ) -> Volume {
        Volume {
            volume_id,
            name,
            token,
            api_url,
            request_timeout_ms,
            proxy,
        }
    }

    /// Build a transient [`ApiClient`] and a resolved [`ConnectionConfig`] from
    /// the caller's [`VolumeOpts`].
    ///
    /// Both `proxy` and `request_timeout_ms` from [`VolumeOpts`] are forwarded
    /// here so that all control-plane calls honour them.
    fn build_api_client(opts: &VolumeOpts) -> Result<(ApiClient, ConnectionConfig)> {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: opts.api_key.clone(),
            domain: opts.domain.clone(),
            api_url: opts.api_url.clone(),
            proxy: opts.proxy.clone(),
            request_timeout_ms: opts.request_timeout_ms,
            ..Default::default()
        });
        let api = ApiClient::new(&config, true)?;
        Ok((api, config))
    }

    /// Create a new volume.
    ///
    /// Sends `POST /volumes` with the given `name` as the request body.
    /// `name` must match the pattern `^[a-zA-Z0-9_-]+$`.
    ///
    /// Returns a [`Volume`] handle that can be used to construct a content
    /// client in Tasks 3–4.
    pub async fn create(name: &str, opts: VolumeOpts) -> Result<Volume> {
        let (api, config) = Volume::build_api_client(&opts)?;
        let request_timeout_ms = opts
            .request_timeout_ms
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);

        let volume_name = api_schema::NewVolumeName::try_from(name.to_string())
            .map_err(|e| Error::InvalidArgument(e.to_string()))?;
        let body = serde_json::to_value(api_schema::NewVolume { name: volume_name })
            .map_err(|e| Error::Internal(e.to_string()))?;

        let res: api_schema::VolumeAndToken = api
            .request(reqwest::Method::POST, "/volumes", &[], Some(&body))
            .await?;

        Ok(Volume::from_parts(
            res.volume_id,
            res.name,
            res.token,
            config.api_url,
            request_timeout_ms,
            opts.proxy,
        ))
    }

    /// List all volumes accessible with the configured API key.
    ///
    /// Sends `GET /volumes` and maps each entry to a public [`VolumeInfo`].
    pub async fn list(opts: VolumeOpts) -> Result<Vec<VolumeInfo>> {
        let (api, _config) = Volume::build_api_client(&opts)?;
        let volumes: Vec<api_schema::Volume> = api
            .request(reqwest::Method::GET, "/volumes", &[], None)
            .await?;
        Ok(volumes.into_iter().map(VolumeInfo::from_wire).collect())
    }

    /// Fetch detailed information (including a fresh bearer token) for a volume.
    ///
    /// Sends `GET /volumes/{volume_id}` and returns a [`VolumeAndToken`]
    /// containing the volume metadata and a short-lived Bearer token for the
    /// content API.
    pub async fn get_info(volume_id: &str, opts: VolumeOpts) -> Result<VolumeAndToken> {
        let (api, _config) = Volume::build_api_client(&opts)?;
        let path = format!("/volumes/{volume_id}");
        let res: api_schema::VolumeAndToken =
            api.request(reqwest::Method::GET, &path, &[], None).await?;
        Ok(VolumeAndToken::from_wire(res))
    }

    /// Connect to an existing volume by ID.
    ///
    /// Sends `GET /volumes/{volume_id}` to obtain a fresh bearer token and
    /// builds a [`Volume`] handle from the result.
    pub async fn connect(volume_id: &str, opts: VolumeOpts) -> Result<Volume> {
        let (api, config) = Volume::build_api_client(&opts)?;
        let request_timeout_ms = opts
            .request_timeout_ms
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT_MS);
        let path = format!("/volumes/{volume_id}");
        let res: api_schema::VolumeAndToken =
            api.request(reqwest::Method::GET, &path, &[], None).await?;
        Ok(Volume::from_parts(
            volume_id.to_string(),
            res.name,
            res.token,
            config.api_url,
            request_timeout_ms,
            opts.proxy,
        ))
    }

    /// Destroy a volume.
    ///
    /// Sends `DELETE /volumes/{volume_id}`.
    ///
    /// Returns `true` if the volume was deleted, `false` if it was not found
    /// (already gone), mirroring the JS SDK's "already gone → false" behaviour.
    pub async fn destroy(volume_id: &str, opts: VolumeOpts) -> Result<bool> {
        let (api, _config) = Volume::build_api_client(&opts)?;
        let path = format!("/volumes/{volume_id}");
        match api
            .request_unit(reqwest::Method::DELETE, &path, &[], None)
            .await
        {
            Ok(()) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Build a [`VolumeOpts`] pointing at the wiremock server with a valid
    /// test API key (mirrors `api_for` in `sandbox::api` tests).
    fn opts_for(server: &MockServer) -> VolumeOpts {
        VolumeOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            api_url: Some(server.uri()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn create_posts_and_builds_volume() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/volumes"))
            .and(header("X-API-KEY", "e2b_0123456789abcdef"))
            .and(body_partial_json(serde_json::json!({ "name": "v" })))
            .respond_with(ResponseTemplate::new(201).set_body_json(
                serde_json::json!({ "volumeID": "vol_1", "name": "v", "token": "tkn" }),
            ))
            .mount(&server)
            .await;

        let vol = Volume::create("v", opts_for(&server))
            .await
            .expect("create ok");
        assert_eq!(vol.volume_id(), "vol_1");
        assert_eq!(vol.token(), "tkn");
        // api_url is not exposed via a getter, but we can verify the volume was
        // built by checking the public fields that rely on it being set correctly.
        assert_eq!(vol.name(), "v");
    }

    #[tokio::test]
    async fn create_rejects_invalid_name() {
        // Names with spaces or special chars violate `^[a-zA-Z0-9_-]+$`.
        let server = MockServer::start().await;
        let err = Volume::create("bad name!", opts_for(&server))
            .await
            .expect_err("invalid name should fail");
        assert!(matches!(err, Error::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn list_maps_to_info() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{ "volumeID": "vol_1", "name": "v" }])),
            )
            .mount(&server)
            .await;

        let infos = Volume::list(opts_for(&server)).await.expect("list ok");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].volume_id, "vol_1");
        assert_eq!(infos[0].name, "v");
    }

    #[tokio::test]
    async fn get_info_returns_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes/vol_1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "volumeID": "vol_1", "name": "v", "token": "tkn" }),
            ))
            .mount(&server)
            .await;

        let info = Volume::get_info("vol_1", opts_for(&server))
            .await
            .expect("get_info ok");
        assert_eq!(info.volume_id, "vol_1");
        assert_eq!(info.name, "v");
        assert_eq!(info.token, "tkn");
    }

    #[tokio::test]
    async fn connect_builds_volume_from_get_info() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes/vol_1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "volumeID": "vol_1", "name": "v", "token": "tkn" }),
            ))
            .mount(&server)
            .await;

        let vol = Volume::connect("vol_1", opts_for(&server))
            .await
            .expect("connect ok");
        assert_eq!(vol.volume_id(), "vol_1");
        assert_eq!(vol.name(), "v");
        assert_eq!(vol.token(), "tkn");
    }

    #[tokio::test]
    async fn destroy_404_is_false() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/volumes/vol_x"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let gone = Volume::destroy("vol_x", opts_for(&server))
            .await
            .expect("destroy ok (idempotent)");
        assert!(!gone);
    }

    #[tokio::test]
    async fn destroy_204_is_true() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/volumes/vol_ok"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let deleted = Volume::destroy("vol_ok", opts_for(&server))
            .await
            .expect("destroy ok");
        assert!(deleted);
    }

    /// Verify that setting `VolumeOpts.proxy` flows into the control-plane
    /// `ConnectionConfigOpts` without error.  We use an unreachable proxy URL
    /// (`http://127.0.0.1:9`) which is syntactically valid — `build_api_client`
    /// must succeed even though no actual proxy is listening at that address
    /// (the proxy is only dialled when a real HTTP request is made, not at
    /// client construction time).
    #[test]
    fn build_api_client_with_proxy_succeeds() {
        let opts = VolumeOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            api_url: Some("http://127.0.0.1:9".to_string()),
            proxy: Some("http://127.0.0.1:9".to_string()),
            ..Default::default()
        };
        // `build_api_client` must succeed: an invalid proxy URL would return
        // `Err(Error::InvalidArgument(...))` here, so any success confirms the
        // proxy field was accepted by `reqwest::ClientBuilder`.
        Volume::build_api_client(&opts).expect("client builds with proxy set");
    }
}
