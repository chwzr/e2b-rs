//! Generate envd protobuf message types (proto3-JSON serde) and vendor them.

use crate::vendor::write_generated;
use prost::Message;
use std::path::Path;

/// Compile `filesystem.proto` and `process.proto` from `spec_dir/envd` and
/// vendor the generated message structs (+ pbjson serde impls) into
/// `sdk_src/envd/proto/{filesystem,process}.rs`.
pub fn generate(spec_dir: &Path, sdk_src: &Path) -> anyhow::Result<()> {
    let proto_root = spec_dir.join("envd");

    // 1. protox: pure-Rust compile to a FileDescriptorSet (bundles google WKTs).
    //    Pass proto paths RELATIVE to the include root (spike-verified form).
    let fds = protox::compile(
        ["filesystem/filesystem.proto", "process/process.proto"],
        [&proto_root],
    )?;
    let fds_bytes = fds.encode_to_vec();

    // 2. Generate into a temp dir so prost/pbjson keep their per-package files.
    let tmp = std::env::temp_dir().join("e2b_rs_proto_gen");
    std::fs::create_dir_all(&tmp)?;

    // prost-build: struct defs. CRITICAL: prost_types_path REPLACES the default
    // `.google.protobuf -> ::prost_types` mapping. Do NOT use extern_path here —
    // it adds a duplicate key and panics ("duplicate extern Protobuf path").
    let mut cfg = prost_build::Config::new();
    cfg.prost_types_path("::pbjson_types");
    cfg.out_dir(&tmp);
    cfg.compile_fds(fds)?;

    // pbjson-build: proto3-JSON serde impls. Writes `{package}.serde.rs` to OUT_DIR.
    // SAFETY: single-threaded codegen; set OUT_DIR for pbjson-build's writer.
    unsafe { std::env::set_var("OUT_DIR", &tmp) };
    pbjson_build::Builder::new()
        .register_descriptors(&fds_bytes)?
        .build(&[".filesystem", ".process"])?;

    // 3. Concatenate each package's struct file + serde file into one vendored
    //    module file, then write with the generated header + rustfmt.
    for pkg in ["filesystem", "process"] {
        let defs = std::fs::read_to_string(tmp.join(format!("{pkg}.rs")))?;
        let serde = std::fs::read_to_string(tmp.join(format!("{pkg}.serde.rs")))?;
        let body = format!("{defs}\n{serde}");
        write_generated(&sdk_src.join(format!("envd/proto/{pkg}.rs")), &body)?;
    }
    Ok(())
}
