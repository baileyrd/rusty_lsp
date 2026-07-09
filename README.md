# rusty_lsp

A small, reusable [Language Server Protocol][lsp] framework for async Rust.

`rusty_lsp` owns the protocol plumbing — JSON-RPC framing, message dispatch, the
`initialize`/`shutdown` lifecycle, and request cancellation — and hands you a
single trait, [`LanguageServer`](src/service.rs), to implement your language's
behaviour.

It is **not** a server for any particular language; it is the reusable engine you
build one on top of.

- **vs. hand-rolled JSON-RPC** — you get typed handlers and lifecycle correctness
  for free.
- **vs. a `tower`-based stack** — the dependency footprint is just `tokio`,
  `serde`, and `serde_json`. No `async-trait`, no `tower`.

## Design at a glance

| Concern | How `rusty_lsp` handles it |
|---|---|
| **Extension point** | One trait, `LanguageServer`. Every method except `initialize` has a default, so a minimal server is just `initialize` returning its capabilities. |
| **Async without `async-trait`** | Trait methods are declared `-> impl Future + Send` (RPITIT). You still write the bodies as ordinary `async fn`. The `+ Send` bound lets request handlers be spawned on a multi-threaded runtime with no boxing layer. |
| **Concurrency** | Notifications run in receipt order (so document state stays consistent — a `didChange` is applied before a later request observes the buffer), while requests are spawned so a slow handler never blocks the loop. |
| **Cancellation** | `$/cancelRequest` aborts the in-flight handler and replies with `RequestCancelled` (`-32800`). The bookkeeping guarantees each request is answered **exactly once**, even when a handler completes at the same instant a cancel arrives. |
| **Lifecycle** | `initialize` is enforced first; requests before it get `ServerNotInitialized` (`-32002`); a second `initialize` is rejected; work after `shutdown` is refused; `exit` (or EOF at a frame boundary) stops the loop cleanly. |
| **Extensibility** | Unmodelled methods reach `handle_request` / `handle_notification`, and `ServerCapabilities::extra` lets you advertise any capability the framework does not type. |

## Architecture

| Module | Responsibility |
|---|---|
| [`transport`](src/transport.rs) | `Content-Length` framing over any async byte stream |
| [`jsonrpc`](src/jsonrpc.rs) | JSON-RPC 2.0 request/response/notification model |
| [`lsp`](src/lsp/mod.rs) | Typed LSP protocol data structures |
| [`text`](src/text.rs) | UTF-16 ↔ byte position conversions for buffer indexing |
| [`service`](src/service.rs) | The `LanguageServer` trait you implement |
| [`client`](src/client.rs) | The `Client` handle for server → client messages |
| [`server`](src/server.rs) | The `Server` runtime: dispatch, lifecycle, cancellation |

## Adding the dependency

`rusty_lsp` distributes via pinned git tags, not crates.io. Depend on a
tagged release:

```toml
[dependencies]
rusty_lsp = { git = "https://github.com/baileyrd/rusty_lsp", tag = "v0.1.0" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

(A path dependency works too during local development: `rusty_lsp = { path = "../rusty_lsp" }`.)

Requires a Rust toolchain supporting **edition 2024** (Rust 1.85+).

## Quick start

A minimal server that advertises hover support:

```rust
use rusty_lsp::error::Result;
use rusty_lsp::lsp::{
    InitializeParams, InitializeResult, ServerCapabilities, ServerInfo, TextDocumentSyncKind,
};
use rusty_lsp::{Client, LanguageServer, Server};

struct Backend {
    client: Client,
}

impl LanguageServer for Backend {
    // Declared `-> impl Future + Send` on the trait; written as plain `async fn`.
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncKind::Full),
                hover_provider: Some(true),
                ..Default::default()
            },
            server_info: Some(ServerInfo { name: "demo".into(), version: None }),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // `serve` hands you a `Client`; store it so handlers can talk back
    // (diagnostics, log/show-message, server → client requests).
    Server::stdio().serve(|client| Backend { client }).await
}
```

### Talking back to the editor

The `Client` is your outbound channel. It is cheap to clone and safe to share
across tasks:

```rust,ignore
// Notifications (fire-and-forget):
client.publish_diagnostics(uri, diagnostics, Some(version))?;
client.log_message(MessageType::Info, "indexing complete")?;

// A server → client request, awaiting the reply:
let config: MyConfig = client
    .send_request("workspace/configuration", params)
    .await?;
```

### Handling methods the framework doesn't model

Common requests (`hover`, `completion`, `definition`) and notifications
(`didOpen`/`didChange`/`didClose`/`didSave`) have typed trait methods. For
anything else, override the escape hatches and advertise the capability through
`ServerCapabilities::extra`:

```rust,ignore
async fn handle_request(&self, method: &str, params: Option<Value>) -> Result<Value> {
    match method {
        "textDocument/formatting" => { /* deserialize params, return a JSON result */ }
        other => Err(Error::method_not_found(other.to_owned())),
    }
}
```

## Example server

[`examples/text_server.rs`](examples/text_server.rs) is a complete, runnable
backend for plain-text documents. It tracks open buffers and provides:

- **diagnostics** — flags `TODO` / `FIXME` / `XXX` markers, republished on each edit;
- **hover** — the word under the cursor and how often it occurs;
- **completion** — the distinct words already present in the buffer.

It dogfoods the [`text`](src/text.rs) module to convert the protocol's UTF-16
columns to byte offsets, so multi-byte characters on a line do not skew ranges.

Run it as an editor would (it speaks LSP over stdio):

```sh
cargo run --example text_server
```

## Testing

```sh
cargo test            # unit + integration + doctests
cargo clippy --all-targets
cargo fmt --check
```

The suite covers JSON-RPC wire shapes, integer/string enum encoding, UTF-16
position math (including multi-byte characters), and end-to-end server behaviour
over in-memory pipes: capability negotiation, lifecycle rejection, diagnostics
publishing, hover/completion dispatch, invalid-params and method-not-found
errors, request cancellation, and clean shutdown/exit/EOF teardown.

## License

Dual-licensed under `MIT OR Apache-2.0` (see the `license` field in
[`Cargo.toml`](Cargo.toml)), at your option.

[lsp]: https://microsoft.github.io/language-server-protocol/
