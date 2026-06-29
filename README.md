# e2b-rs

Rust SDK for [E2B](https://e2b.dev) — cloud sandboxes for AI agents. A 1:1 port
of the official [JavaScript SDK](https://github.com/e2b-dev/e2b/tree/main/packages/js-sdk),
built to feel familiar while reading as idiomatic async Rust.

> **Status:** under active development, built in milestones. The foundation
> layer (configuration, errors, logging, pagination, signatures) is in place;
> sandbox creation, commands, filesystem, git, volumes, and templates follow.

## Installation

```toml
[dependencies]
e2b-rs = "0.1"
```

## Quickstart

```rust
use e2b_rs::Sandbox;

let sandbox = Sandbox::create().template("base").await?;
println!("{}", sandbox.get_host(3000));
sandbox.kill().await?;
```

The library is imported as `e2b_rs`:

```rust
use e2b_rs::{ConnectionConfig, ConnectionConfigOpts};
```

## Design

- **Async-only** on `tokio`.
- **Channels, not callbacks:** streaming output (commands, pty, watch, build
  logs) is delivered through `tokio::sync::mpsc` receivers.
- **Panic-free library code:** `unwrap`/`expect` are denied outside tests.
- **MSRV:** Rust 1.95.0, edition 2024.

See `.super/specs/` for the full design and `docs/parity-checklist.md` for the
JS-to-Rust parity matrix.

## License

MIT
