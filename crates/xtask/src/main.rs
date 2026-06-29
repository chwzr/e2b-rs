//! Codegen driver for e2b-rs. Run with `cargo xtask codegen`.
//!
//! Generation modules are added in later tasks; this skeleton dispatches the
//! `codegen` subcommand and fails loudly on unknown input.

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
            Ok(())
        }
        other => Err(anyhow::anyhow!(
            "unknown xtask command: {other:?} (expected `codegen`)"
        )),
    }
}
