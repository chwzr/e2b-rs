//! Codegen driver for e2b-rs. Run with `cargo xtask codegen`.
//!
//! Generation modules are added in later tasks; this skeleton dispatches the
//! `codegen` subcommand and fails loudly on unknown input.

mod vendor;

fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("codegen") => {
            println!("xtask codegen: no generators wired yet");
            Ok(())
        }
        other => Err(anyhow::anyhow!(
            "unknown xtask command: {other:?} (expected `codegen`)"
        )),
    }
}
