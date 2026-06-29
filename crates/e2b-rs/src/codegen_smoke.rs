//! TEMPORARY: proves the codegen-output runtime deps (prost, pbjson-types,
//! chrono, uuid, regress, serde) link. Removed in Task 7 once real generated
//! modules exist.

#[cfg(test)]
mod tests {
    #[test]
    fn runtime_codegen_deps_link() {
        // pbjson-types Timestamp (referenced by generated proto structs)
        let ts = pbjson_types::Timestamp {
            seconds: 0,
            nanos: 0,
        };
        assert_eq!(ts.seconds, 0);
        // chrono (referenced by typify date-time fields)
        let _now: chrono::DateTime<chrono::Utc> = chrono::DateTime::UNIX_EPOCH;
        // uuid (referenced by typify uuid fields)
        let _id = uuid::Uuid::nil();
        // serde_json round-trips
        let v: serde_json::Value = serde_json::json!({"ok": true});
        assert_eq!(v["ok"], serde_json::Value::Bool(true));
    }
}
