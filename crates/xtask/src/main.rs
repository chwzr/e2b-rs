//! Codegen driver for e2b-rs. Run with `cargo xtask codegen`.
//!
//! Generation modules are added in later tasks; this skeleton dispatches the
//! `codegen` subcommand and fails loudly on unknown input.

mod openapi;
mod proto;
mod vendor;

fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("codegen") => {
            let spec_dir = std::path::PathBuf::from(
                std::env::var("E2B_SPEC_DIR").unwrap_or_else(|_| "../E2B/spec".to_string()),
            );
            let sdk_src = std::path::PathBuf::from("crates/e2b-rs/src");
            proto::generate(&spec_dir, &sdk_src)?;
            println!("xtask codegen: wrote envd proto modules");
            openapi::generate_schema_types(
                &spec_dir.join("openapi-volumecontent.yml"),
                &sdk_src.join("volume/gen.rs"),
            )?;
            println!("xtask codegen: wrote volume content types");
            openapi::generate_schema_types(
                &spec_dir.join("openapi.yml"),
                &sdk_src.join("api/gen.rs"),
            )?;
            println!("xtask codegen: wrote control-plane API types");
            openapi::generate_schema_types(
                &spec_dir.join("envd/envd.yaml"),
                &sdk_src.join("envd/rest_gen.rs"),
            )?;
            println!("xtask codegen: wrote envd REST types");
            openapi::generate_json_schema_types(
                &spec_dir.join("mcp-server.json"),
                &sdk_src.join("sandbox/mcp_gen.rs"),
            )?;
            println!("xtask codegen: wrote MCP server types");
            Ok(())
        }
        other => Err(anyhow::anyhow!(
            "unknown xtask command: {other:?} (expected `codegen`)"
        )),
    }
}
