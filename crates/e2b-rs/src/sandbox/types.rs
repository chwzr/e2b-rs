//! Public sandbox lifecycle types.

use std::collections::BTreeMap;

use crate::api::schema as api_schema;

/// Whether a sandbox is currently running or paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxState {
    /// The sandbox is running.
    Running,
    /// The sandbox is paused (snapshotted).
    Paused,
}

/// Metadata and runtime details about a sandbox.
#[derive(Debug, Clone)]
pub struct SandboxInfo {
    /// Sandbox identifier.
    pub sandbox_id: String,
    /// Template the sandbox was created from.
    pub template_id: String,
    /// Optional template alias/name.
    pub name: Option<String>,
    /// User-provided metadata.
    pub metadata: BTreeMap<String, String>,
    /// When the sandbox started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the sandbox will expire.
    pub end_at: chrono::DateTime<chrono::Utc>,
    /// Running or paused.
    pub state: SandboxState,
    /// vCPU count.
    pub cpu_count: u32,
    /// Memory in MB.
    pub memory_mb: u32,
    /// envd version.
    pub envd_version: String,
    /// Whether internet access was explicitly set.
    pub allow_internet_access: Option<bool>,
    /// Base domain serving this sandbox's traffic.
    pub sandbox_domain: Option<String>,
}

impl SandboxInfo {
    /// Build a public [`SandboxInfo`] from the generated control-plane detail.
    #[allow(dead_code)] // used in later milestones; no caller yet in this crate
    pub(crate) fn from_detail(d: api_schema::SandboxDetail) -> SandboxInfo {
        let state = match d.state {
            api_schema::SandboxState::Running => SandboxState::Running,
            api_schema::SandboxState::Paused => SandboxState::Paused,
        };
        SandboxInfo {
            sandbox_id: d.sandbox_id,
            template_id: d.template_id,
            name: d.alias,
            metadata: d
                .metadata
                .map(|m| m.0.into_iter().collect())
                .unwrap_or_default(),
            started_at: d.started_at,
            end_at: d.end_at,
            state,
            cpu_count: d.cpu_count.0.get(),
            memory_mb: u32::try_from(d.memory_mb.0).unwrap_or(0),
            envd_version: d.envd_version.0,
            allow_internet_access: d.allow_internet_access,
            sandbox_domain: d.domain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_info_converts_from_detail_json() {
        // A representative POST /sandboxes / GET /sandboxes/{id} response.
        let json = r#"{
            "sandboxID": "sbx_123",
            "templateID": "base",
            "clientID": "c1",
            "cpuCount": 2,
            "memoryMB": 1024,
            "diskSizeMB": 1024,
            "envdVersion": "0.6.0",
            "state": "running",
            "startedAt": "2026-06-30T10:00:00Z",
            "endAt": "2026-06-30T10:05:00Z",
            "metadata": {"k": "v"},
            "domain": "e2b.app"
        }"#;
        let detail: crate::api::schema::SandboxDetail =
            serde_json::from_str(json).expect("deserialize SandboxDetail");
        let info = SandboxInfo::from_detail(detail);
        assert_eq!(info.sandbox_id, "sbx_123");
        assert_eq!(info.template_id, "base");
        assert_eq!(info.cpu_count, 2);
        assert_eq!(info.memory_mb, 1024);
        assert_eq!(info.envd_version, "0.6.0");
        assert!(matches!(info.state, SandboxState::Running));
        assert_eq!(info.metadata.get("k").map(String::as_str), Some("v"));
        assert_eq!(info.sandbox_domain.as_deref(), Some("e2b.app"));
    }
}
