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

The filesystem API lets you read, write, and watch files inside the sandbox:

```rust
use e2b_rs::Sandbox;

let sandbox = Sandbox::create().template("base").await?;
sandbox.files().write("/tmp/hello.txt", b"hi".to_vec(), Default::default()).await?;
let text = sandbox.files().read("/tmp/hello.txt", None).await?;
let mut watch = sandbox.files().watch_dir("/tmp", Default::default()).await?;
while let Some(event) = watch.next().await {
    println!("{:?} {}", event.r#type, event.name);
}
```

Run commands inside the sandbox and stream their output:

```rust
use e2b_rs::{CommandOutput, Sandbox};

let sandbox = Sandbox::create().template("base").await?;
// Foreground: run to completion.
let result = sandbox.commands().run("echo hello", Default::default()).await?;
println!("exit {}: {}", result.exit_code, result.stdout);

// Background: stream output as it arrives.
let mut cmd = sandbox.commands().start("sleep 1; echo done", Default::default()).await?;
while let Some(out) = cmd.next().await {
    if let CommandOutput::Stdout(bytes) = out {
        print!("{}", String::from_utf8_lossy(&bytes));
    }
}
let _ = cmd.wait().await?;
```

Control-plane extras are also available: pause/resume, metrics, snapshots
(create/list/delete), network-rule updates, and signed upload/download URLs.

```rust
use e2b_rs::Sandbox;

let sandbox = Sandbox::create().template("base").await?;
let metrics = sandbox.get_metrics().await?;
let snap = sandbox.create_snapshot(Some("nightly".to_string())).await?;
sandbox.pause().await?;
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
