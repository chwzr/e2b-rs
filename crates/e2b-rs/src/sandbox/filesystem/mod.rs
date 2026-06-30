//! Filesystem API for sandbox file operations.

pub mod types;

pub use types::{EntryInfo, FileType, FilesystemEvent, FilesystemEventType, WriteEntry, WriteInfo};

use crate::connect::client::{ConnectClient, ConnectClientOpts};
use crate::connection_config::{ConnectionConfig, DEFAULT_USERNAME, ENVD_PORT};
use crate::envd::proto::filesystem as pb;
use crate::envd::rest::{EnvdApiClient, EnvdApiClientOpts};
use crate::envd::versions::{ENVD_DEFAULT_USER, version_gte};
use crate::errors::{Error, Result};

/// Map a filesystem `NotFound` to the file-specific [`Error::FileNotFound`]
/// (matching the JS SDK, which raises `FileNotFoundError`).
pub(crate) fn file_not_found_on_missing(err: Error, path: &str) -> Error {
    match err {
        Error::NotFound(_) => Error::FileNotFound(format!("File not found: {path}")),
        other => other,
    }
}

/// The sandbox filesystem: read/write byte I/O over the envd REST `/files`
/// surface plus metadata operations over the Connect `Filesystem` service.
pub struct Filesystem {
    /// Connect-over-JSON client for the envd RPC surface.
    pub(crate) connect: ConnectClient,
    /// REST client for the envd `/files` surface (used by Tasks 3–5).
    #[allow(dead_code)]
    pub(crate) rest: EnvdApiClient,
    /// envd version string; used by version gates (used by Tasks 3–6).
    #[allow(dead_code)]
    pub(crate) envd_version: String,
    /// Legacy default user (set on old envd < 0.4.0, `None` on modern envd).
    pub(crate) default_user: Option<String>,
}

impl Filesystem {
    /// Build a `Filesystem` for a sandbox, resolving the envd base URL from the
    /// connection config (envd port `49983`).
    pub(crate) fn build(
        sandbox_id: &str,
        sandbox_domain: &str,
        envd_version: &str,
        envd_access_token: Option<&str>,
        config: &ConnectionConfig,
    ) -> Result<Filesystem> {
        let base_url = config.get_sandbox_url(sandbox_id, sandbox_domain, ENVD_PORT);
        Self::build_with_base_url(
            base_url,
            sandbox_id,
            envd_version,
            envd_access_token,
            config,
        )
    }

    /// Build directly from a base URL (used by tests + by [`Filesystem::build`]).
    pub(crate) fn build_with_base_url(
        base_url: String,
        sandbox_id: &str,
        envd_version: &str,
        envd_access_token: Option<&str>,
        config: &ConnectionConfig,
    ) -> Result<Filesystem> {
        let user_agent = crate::utils::build_user_agent(config.integration.as_deref());
        let connect = ConnectClient::new(ConnectClientOpts {
            base_url: base_url.clone(),
            access_token: envd_access_token.map(str::to_string),
            sandbox_id: sandbox_id.to_string(),
            envd_port: ENVD_PORT,
            user_agent: user_agent.clone(),
            envd_version: envd_version.to_string(),
            request_timeout_ms: config.request_timeout_ms,
            logger: config.logger.clone(),
            proxy: config.proxy.clone(),
        })?;
        let rest = EnvdApiClient::new(EnvdApiClientOpts {
            base_url,
            access_token: envd_access_token.map(str::to_string),
            sandbox_id: sandbox_id.to_string(),
            envd_port: ENVD_PORT,
            user_agent,
            request_timeout_ms: config.request_timeout_ms,
            logger: config.logger.clone(),
            proxy: config.proxy.clone(),
        })?;
        // Older envd (<0.4.0) has no per-request user; default to the legacy user.
        let default_user =
            (!version_gte(envd_version, ENVD_DEFAULT_USER)).then(|| DEFAULT_USERNAME.to_string());
        Ok(Filesystem {
            connect,
            rest,
            envd_version: envd_version.to_string(),
            default_user,
        })
    }

    /// Resolve the user for a request: explicit `user`, else the legacy default
    /// on old envd, else `None`.
    pub(crate) fn resolve_user(&self, user: Option<&str>) -> Option<String> {
        match user {
            Some(u) => Some(u.to_string()),
            None => self.default_user.clone(),
        }
    }

    /// Get metadata for a path. Errors with [`Error::FileNotFound`] if missing.
    pub async fn get_info(&self, path: &str, user: Option<&str>) -> Result<EntryInfo> {
        let user = self.resolve_user(user);
        let req = pb::StatRequest {
            path: path.to_string(),
        };
        let resp: pb::StatResponse = self
            .connect
            .unary(crate::connect::FS_STAT, &req, user.as_deref())
            .await
            .map_err(|e| file_not_found_on_missing(e, path))?;
        resp.entry
            .and_then(EntryInfo::from_proto)
            .ok_or_else(|| Error::Internal(format!("Stat returned no entry for {path}")))
    }

    /// Whether a path exists.
    pub async fn exists(&self, path: &str, user: Option<&str>) -> Result<bool> {
        match self.get_info(path, user).await {
            Ok(_) => Ok(true),
            Err(Error::FileNotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection_config::{ConnectionConfig, ConnectionConfigOpts};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fs_for(server: &MockServer) -> Filesystem {
        let config = ConnectionConfig::new(ConnectionConfigOpts {
            api_key: Some("e2b_0123456789abcdef".to_string()),
            ..Default::default()
        });
        // Point the envd clients straight at the mock server.
        Filesystem::build_with_base_url(server.uri(), "sbx_fs", "0.6.3", None, &config)
            .expect("filesystem")
    }

    fn entry_json(path: &str) -> serde_json::Value {
        serde_json::json!({
            "name": "f.txt", "type": "FILE_TYPE_FILE", "path": path,
            "size": "12", "mode": 420, "permissions": "-rw-r--r--",
            "owner": "user", "group": "user", "metadata": {}
        })
    }

    #[tokio::test]
    async fn get_info_returns_entry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/Stat"))
            .and(body_partial_json(
                serde_json::json!({ "path": "/home/user/f.txt" }),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "entry": entry_json("/home/user/f.txt") })),
            )
            .mount(&server)
            .await;
        let info = fs_for(&server)
            .get_info("/home/user/f.txt", None)
            .await
            .expect("info");
        assert_eq!(info.path, "/home/user/f.txt");
        assert_eq!(info.r#type, FileType::File);
    }

    #[tokio::test]
    async fn exists_false_on_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/filesystem.Filesystem/Stat"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(
                    serde_json::json!({ "code": "not_found", "message": "missing" }),
                ),
            )
            .mount(&server)
            .await;
        assert!(!fs_for(&server).exists("/nope", None).await.expect("exists"));
    }
}
