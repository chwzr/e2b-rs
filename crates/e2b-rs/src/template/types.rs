//! Build-status and template-tag wrapper types for the template builder.
//!
//! These types wrap the generated API schema types (which remain
//! `pub(crate)`) and expose a stable, ergonomic public interface. All
//! constructors go through crate-internal `from_wire` associated functions that
//! accept the generated types and perform any necessary conversions (e.g. UUID
//! to `String`, ANSI stripping via [`LogEntry`]).
//!
//! # Re-exports
//!
//! These types are re-exported at the crate root:
//! - [`BuildStatus`]
//! - [`BuildStatusReason`]
//! - [`TemplateTag`]
//! - [`TemplateBuildStatusResponse`]

use chrono::{DateTime, Utc};

use crate::template::log::LogEntry;

/// Current status of a template build.
///
/// Mirrors the wire `TemplateBuildStatus` enum with idiomatic Rust naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    /// The build is currently in progress.
    Building,
    /// The build is queued and waiting to start.
    Waiting,
    /// The build completed successfully and the template is ready to use.
    Ready,
    /// The build failed. See [`TemplateBuildStatusResponse::reason`] for
    /// details.
    Error,
}

impl BuildStatus {
    /// Converts the generated [`crate::api::schema::TemplateBuildStatus`] into
    /// a public [`BuildStatus`].
    // Called from TemplateBuildStatusResponse::from_wire (Plan 5b callers arrive later).
    #[allow(dead_code)]
    pub(crate) fn from_wire(s: crate::api::schema::TemplateBuildStatus) -> Self {
        use crate::api::schema::TemplateBuildStatus as W;
        match s {
            W::Building => Self::Building,
            W::Waiting => Self::Waiting,
            W::Ready => Self::Ready,
            W::Error => Self::Error,
        }
    }
}

/// Human-readable explanation of why a build reached its current status.
///
/// Usually present when [`BuildStatus`] is [`BuildStatus::Error`].
#[derive(Debug, Clone)]
pub struct BuildStatusReason {
    /// Human-readable description of the status reason.
    pub message: String,
    /// Build step during which the reason occurred, if applicable.
    pub step: Option<String>,
    /// Structured log entries associated with this status reason. Messages
    /// have ANSI escape sequences stripped.
    pub log_entries: Vec<LogEntry>,
}

impl BuildStatusReason {
    /// Converts the generated [`crate::api::schema::BuildStatusReason`] into a
    /// public [`BuildStatusReason`].
    ///
    /// Each [`crate::api::schema::BuildLogEntry`] in `log_entries` is mapped
    /// via [`LogEntry::from_wire`], which strips ANSI sequences from messages.
    // Called from TemplateBuildStatusResponse::from_wire (Plan 5b callers arrive later).
    #[allow(dead_code)]
    pub(crate) fn from_wire(w: crate::api::schema::BuildStatusReason) -> Self {
        Self {
            message: w.message,
            step: w.step,
            log_entries: w.log_entries.into_iter().map(LogEntry::from_wire).collect(),
        }
    }
}

/// A named tag associated with a specific template build.
///
/// Tags allow builds to be referenced by a human-readable label rather than
/// a raw build UUID.
#[derive(Debug, Clone)]
pub struct TemplateTag {
    /// The tag name (e.g. `"latest"` or `"v1.2"`).
    pub tag: String,
    /// String representation of the UUID identifying the build this tag points
    /// to.
    pub build_id: String,
    /// When this tag was assigned.
    pub created_at: DateTime<Utc>,
}

impl TemplateTag {
    /// Converts the generated [`crate::api::schema::TemplateTag`] into a
    /// public [`TemplateTag`].
    ///
    /// The generated `build_id` field is a [`uuid::Uuid`]; it is exposed here
    /// as a `String` via `.to_string()`.
    // Callers arrive in Plan 5b/5c.
    #[allow(dead_code)]
    pub(crate) fn from_wire(w: crate::api::schema::TemplateTag) -> Self {
        Self {
            tag: w.tag,
            build_id: w.build_id.to_string(),
            created_at: w.created_at,
        }
    }
}

/// Combined status and structured logs for an in-progress or completed
/// template build.
///
/// Returned when polling the build status endpoint. Log entries are available
/// both as raw strings ([`logs`][TemplateBuildStatusResponse::logs]) and as
/// structured, ANSI-stripped entries
/// ([`log_entries`][TemplateBuildStatusResponse::log_entries]).
#[derive(Debug, Clone)]
pub struct TemplateBuildStatusResponse {
    /// Identifier of the template being built.
    pub template_id: String,
    /// Identifier of the specific build.
    pub build_id: String,
    /// Current build status.
    pub status: BuildStatus,
    /// Raw build log lines as plain strings.
    pub logs: Vec<String>,
    /// Structured build log entries (ANSI sequences stripped from messages).
    pub log_entries: Vec<LogEntry>,
    /// Reason for the current status; usually present when
    /// [`status`][TemplateBuildStatusResponse::status] is
    /// [`BuildStatus::Error`].
    pub reason: Option<BuildStatusReason>,
}

impl TemplateBuildStatusResponse {
    /// Converts the generated [`crate::api::schema::TemplateBuildInfo`] into a
    /// public [`TemplateBuildStatusResponse`].
    ///
    /// Each [`crate::api::schema::BuildLogEntry`] in `log_entries` is mapped
    /// via [`LogEntry::from_wire`].
    // Callers arrive in Plan 5b/5c.
    #[allow(dead_code)]
    pub(crate) fn from_wire(w: crate::api::schema::TemplateBuildInfo) -> Self {
        Self {
            template_id: w.template_id,
            build_id: w.build_id,
            status: BuildStatus::from_wire(w.status),
            logs: w.logs,
            log_entries: w.log_entries.into_iter().map(LogEntry::from_wire).collect(),
            reason: w.reason.map(BuildStatusReason::from_wire),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::TemplateBuildStatus as WireStatus;
    use crate::template::log::LogEntryLevel;

    #[test]
    fn build_status_from_wire() {
        assert_eq!(
            BuildStatus::from_wire(WireStatus::Building),
            BuildStatus::Building
        );
        assert_eq!(
            BuildStatus::from_wire(WireStatus::Waiting),
            BuildStatus::Waiting
        );
        assert_eq!(
            BuildStatus::from_wire(WireStatus::Ready),
            BuildStatus::Ready
        );
        assert_eq!(
            BuildStatus::from_wire(WireStatus::Error),
            BuildStatus::Error
        );
    }

    #[test]
    fn tag_from_wire() {
        let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000")
            .expect("valid uuid literal");
        let wire = crate::api::schema::TemplateTag {
            build_id: uuid,
            created_at: "2024-06-01T00:00:00Z"
                .parse()
                .expect("valid timestamp literal"),
            tag: "latest".to_string(),
        };
        let tag = TemplateTag::from_wire(wire);
        assert_eq!(tag.tag, "latest");
        assert_eq!(tag.build_id, "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn serde_round_trip_build_info() {
        // Honest wire JSON: lowercase status, camelCase keys, logEntries with
        // lowercase level strings — matching the real API response format.
        let json = r#"{
            "templateID": "tmpl-abc123",
            "buildID": "build-xyz789",
            "status": "building",
            "logs": [],
            "logEntries": [
                {
                    "level": "info",
                    "message": "x",
                    "timestamp": "2024-01-01T00:00:00Z"
                }
            ]
        }"#;

        let wire: crate::api::schema::TemplateBuildInfo =
            serde_json::from_str(json).expect("valid TemplateBuildInfo JSON");
        let response = TemplateBuildStatusResponse::from_wire(wire);

        assert_eq!(response.status, BuildStatus::Building);
        assert_eq!(response.template_id, "tmpl-abc123");
        assert_eq!(response.build_id, "build-xyz789");
        assert_eq!(response.log_entries.len(), 1);
        assert_eq!(response.log_entries[0].level(), LogEntryLevel::Info);
        assert_eq!(response.log_entries[0].message(), "x");
    }

    #[test]
    fn serde_round_trip_with_reason() {
        let json = r#"{
            "templateID": "tmpl-err",
            "buildID": "build-err",
            "status": "error",
            "logs": ["something went wrong"],
            "logEntries": [],
            "reason": {
                "message": "Dockerfile syntax error",
                "step": "build",
                "logEntries": []
            }
        }"#;

        let wire: crate::api::schema::TemplateBuildInfo =
            serde_json::from_str(json).expect("valid TemplateBuildInfo JSON with reason");
        let response = TemplateBuildStatusResponse::from_wire(wire);

        assert_eq!(response.status, BuildStatus::Error);
        let reason = response.reason.expect("reason should be present");
        assert_eq!(reason.message, "Dockerfile syntax error");
        assert_eq!(reason.step.as_deref(), Some("build"));
    }
}
