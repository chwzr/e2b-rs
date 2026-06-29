//! Generate Rust types from an OpenAPI document's `components.schemas` via
//! typify, and vendor the result. typify consumes JSON Schema, so we lift the
//! schema section into a draft-07 root document and rewrite the `$ref` base.

use crate::vendor::write_generated;
use std::path::Path;
use typify::{TypeSpace, TypeSpaceSettings};

/// Extract `components.schemas` from the OpenAPI doc at `spec_path`, generate
/// Rust types, and vendor them to `out_path`.
pub fn generate_schema_types(spec_path: &Path, out_path: &Path) -> anyhow::Result<()> {
    // 1. Parse OpenAPI YAML into JSON.
    let raw = std::fs::read_to_string(spec_path)?;
    let spec: serde_json::Value = serde_yaml::from_str(&raw)?;

    // 2. Lift components.schemas; OpenAPI `$ref: #/components/schemas/X`
    //    becomes JSON-Schema `$ref: #/definitions/X`.
    let schemas = spec
        .get("components")
        .and_then(|c| c.get("schemas"))
        .ok_or_else(|| anyhow::anyhow!("no components.schemas in {}", spec_path.display()))?;
    let schemas_str =
        serde_json::to_string(schemas)?.replace("#/components/schemas/", "#/definitions/");
    let definitions: serde_json::Value = serde_json::from_str(&schemas_str)?;

    // 3. Build a draft-07 root schema and hand its definitions to typify.
    let root: schemars::schema::RootSchema = serde_json::from_value(serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "definitions": definitions,
    }))?;

    let mut type_space = TypeSpace::new(&TypeSpaceSettings::default());
    type_space.add_ref_types(root.definitions)?;

    // 4. Vendor.
    write_generated(out_path, &type_space.to_stream().to_string())?;
    Ok(())
}

/// Generate Rust types from a standalone JSON Schema document (e.g. the MCP
/// server schema) — no OpenAPI `components` wrapper.
pub fn generate_json_schema_types(schema_path: &Path, out_path: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(schema_path)?;
    let root: schemars::schema::RootSchema = serde_json::from_str(&raw)?;

    let mut type_space = TypeSpace::new(&TypeSpaceSettings::default());
    // Root schema may carry a top-level type plus `$defs`/`definitions`.
    type_space.add_root_schema(root)?;

    write_generated(out_path, &type_space.to_stream().to_string())?;
    Ok(())
}
