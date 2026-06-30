//! Public sandbox network-update types.

use std::collections::BTreeMap;

/// A single network rule's request transform (e.g. injected headers).
#[derive(Debug, Clone, Default)]
pub struct NetworkRule {
    /// Headers to inject on requests matched by this rule.
    pub transform_headers: BTreeMap<String, String>,
}

/// An atomic update to a sandbox's egress network policy.
///
/// **Replacement semantics:** the update fully replaces the sandbox's policy —
/// any field left empty/`None` is CLEARED on the server, not merged.
#[derive(Debug, Clone, Default)]
pub struct SandboxNetworkUpdate {
    /// Whether the sandbox may reach the public internet.
    pub allow_internet_access: Option<bool>,
    /// Allowed egress destinations (domains/CIDRs).
    pub allow_out: Vec<String>,
    /// Denied egress destinations (domains/CIDRs).
    pub deny_out: Vec<String>,
    /// Per-destination request rules, keyed by destination.
    pub rules: BTreeMap<String, Vec<NetworkRule>>,
}

impl SandboxNetworkUpdate {
    /// Build the `SandboxNetworkUpdateConfig` request body. Field casing matches
    /// the spec: snake_case `allow_internet_access`, camelCase `allowOut`/`denyOut`.
    pub(crate) fn to_wire_body(&self) -> serde_json::Value {
        let rules: serde_json::Map<String, serde_json::Value> = self
            .rules
            .iter()
            .map(|(dest, rules)| {
                let arr: Vec<serde_json::Value> = rules
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "transform": { "headers": r.transform_headers }
                        })
                    })
                    .collect();
                (dest.clone(), serde_json::Value::Array(arr))
            })
            .collect();

        let mut body = serde_json::json!({
            "allowOut": self.allow_out,
            "denyOut": self.deny_out,
            "rules": rules,
        });
        if let Some(allow) = self.allow_internet_access {
            body["allow_internet_access"] = serde_json::Value::Bool(allow);
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_wire_body_uses_correct_casing() {
        let mut update = SandboxNetworkUpdate {
            allow_internet_access: Some(true),
            allow_out: vec!["1.1.1.1".to_string()],
            ..Default::default()
        };
        update
            .rules
            .entry("example.com".to_string())
            .or_default()
            .push(NetworkRule {
                transform_headers: [("X-Test".to_string(), "1".to_string())]
                    .into_iter()
                    .collect(),
            });
        let body = update.to_wire_body();
        // snake_case allow_internet_access, camelCase allowOut/denyOut.
        assert_eq!(body["allow_internet_access"], serde_json::json!(true));
        assert_eq!(body["allowOut"], serde_json::json!(["1.1.1.1"]));
        assert_eq!(body["denyOut"], serde_json::json!([]));
        assert_eq!(
            body["rules"]["example.com"][0]["transform"]["headers"]["X-Test"],
            serde_json::json!("1")
        );
    }
}
